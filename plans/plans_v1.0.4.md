# v1.0.4 实施计划：剪贴板监听替代轮询

## 背景

当前每 800ms 轮询一次 `arboard::Clipboard::get_image()`，虽然 v1.0.3 的内存 MD5 去重避免了盲写磁盘，但剪贴板中有图片内容时 CPU 占用仍然偏高，任务管理器里排序靠前。

Windows 提供了 `AddClipboardFormatListener` API（Vista+），允许注册窗口接收 `WM_CLIPBOARDUPDATE` 消息，剪贴板内容变化时系统主动通知，无需轮询。

## 技术方案

### 核心思路

创建独立线程 + Message-Only Window 接收 `WM_CLIPBOARDUPDATE`，通过 `PostThreadMessageW` 通知主线程的 Win32 消息循环。

```
┌─────────────────────────┐  PostThreadMessageW  ┌──────────────────────────┐
│  Clipboard Listener     │ ──── WM_CLIP_CHANGED─>│  Win32 Message Loop      │
│  Thread                 │                       │  (main thread)           │
│                         │                       │                          │
│  MessageOnlyWindow      │                       │  收到消息后检查：         │
│  AddClipboardFormat     │                       │  1. wm_clip_changed →    │
│  Listener               │                       │     arboard 读图片+去重  │
│  GetMessageW loop       │                       │  2. GlobalHotKeyEvent →  │
│                         │                       │     热键处理              │
│  wnd_proc 通过          │                       │  3. MenuEvent →          │
│  thread_local 访问      │                       │     托盘菜单处理          │
│  main_thread_id         │                       │                          │
└─────────────────────────┘                       └──────────────────────────┘
```

### 关键架构决策：为什么干掉 tao 事件循环

最初的方案保留 tao 事件循环，用 `ControlFlow::Wait` + channel 替代轮询。实测发现 **tao 事件循环会产生大量 `DeviceEvent`（鼠标原始输入事件），约 400-500 次/秒**，即使设置了 `ControlFlow::Wait` 也不会真正休眠，CPU 占用依然偏高。

诊断日志证据：
```
[诊断] 事件循环: 2479次 / 5096ms (486次/秒) Top: DeviceEvent:1192, MainEventsCleared:429, NewEvents:429
```

最终方案：**完全移除 tao 事件循环**，改用原生 Win32 `GetMessageW` 消息循环。`tray-icon`、`global-hotkey`、`muda` 都在主线程创建各自的内部窗口，`GetMessageW` + `DispatchMessageW` 自然分发这些窗口的消息，不需要 tao 作为中间层。

### 为什么不子类化 tao 窗口

- `tao` 不暴露底层 WndProc 钩子，强行子类化侵入性强、维护成本高
- 独立线程的 MessageOnlyWindow 方案完全解耦，不依赖 tao 内部实现
- listener 线程只做"通知"，不做业务逻辑，极其轻量

### 通知机制：PostThreadMessageW 替代 mpsc channel

listener 线程通过 `PostThreadMessageW(main_thread_id, wm_clip_changed, 0, 0)` 发送自定义消息到主线程。主线程的 `GetMessageW` 收到后直接处理，不需要额外轮询 channel。

`wm_clip_changed` 通过 `RegisterWindowMessageW("clipImgClipboardChanged")` 注册，保证消息 ID 唯一不冲突。

listener 线程的 `wnd_proc` 通过 `thread_local!` 访问 `(main_thread_id, notify_msg)`，避免全局变量。

### 自触发循环问题

剪贴板模式（方案 C）下，保存图片后会调 `set_multi_format_clipboard` 写回剪贴板 → 再次触发 `WM_CLIPBOARDUPDATE`。但 **v1.0.3 的内存 MD5 去重可以兜住**：

1. 用户截图 → 通知 → 读剪贴板 → MD5 新 → 保存 → 写回剪贴板
2. 写回触发通知 → 读剪贴板 → MD5 与 `last_md5` 相同 → 跳过

不会无限循环，只会多一次无意义的 MD5 计算。如需进一步优化可在后续版本加 `suppress_notifications` 标志位。

## 改动范围

### 1. 新增 `src/clipboard_listener.rs`

Windows-only 模块，职责单一：监听剪贴板变化，通知主线程。

```rust
#[cfg(target_os = "windows")]
pub struct ClipboardListener {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl ClipboardListener {
    /// 启动监听线程
    /// main_thread_id: 主线程 ID，用于 PostThreadMessageW
    /// notify_msg: 自定义消息 ID（RegisterWindowMessageW 生成）
    pub fn start(main_thread_id: u32, notify_msg: u32) -> Result<Self, String>;
}
```

监听线程内部逻辑：
1. 设置 `thread_local!` 存储 `(main_thread_id, notify_msg)`
2. `RegisterClassW` + `CreateWindowExW` 创建 Message-Only Window（`HWND_MESSAGE`）
3. `AddClipboardFormatListener(hwnd)` 注册通知
4. `GetMessageW` 阻塞循环，收到 `WM_CLIPBOARDUPDATE` 时 `PostThreadMessageW` 通知主线程
5. 收到 `WM_QUIT` 时退出循环并 `RemoveClipboardFormatListener`
6. `Drop` 时 `PostThreadMessageW(WM_QUIT)` + `join()` 等待线程退出

### 2. 重写 `src/main.rs` 事件循环

**移除 tao 事件循环**，改用 Win32 原生消息循环：

```rust
// 注册自定义消息
let wm_clip_changed = unsafe { RegisterWindowMessageW(name.as_ptr()) };
let main_thread_id = unsafe { GetCurrentThreadId() };

// 启动监听线程
let _clip_listener = ClipboardListener::start(main_thread_id, wm_clip_changed)?;

// Win32 消息循环
let mut msg: MSG = unsafe { std::mem::zeroed() };
loop {
    let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
    if ret == 0 { break; } // WM_QUIT

    // 剪贴板变化
    if msg.message == wm_clip_changed { /* arboard 读图片 + 保存 */ }

    // 热键事件（GlobalHotKeyEvent::receiver()）
    // 托盘菜单事件（MenuEvent::receiver()）

    unsafe { TranslateMessage(&msg); DispatchMessageW(&msg); }
}
```

关键变化：
- 移除 `tao::EventLoop`、`ControlFlow`、`Event` 等
- 移除 `mpsc::channel`，改用 `PostThreadMessageW` + `GetMessageW`
- 移除 `last_poll` / `poll_interval` 定时器逻辑
- 退出改用 `PostQuitMessage(0)`
- `GetMessageW` 无消息时真正阻塞休眠 → 零 CPU

### 3. 修改 `src/config.rs`

- `poll_interval_ms` 字段标注 `#[serde(default)]`，保留不删除（向后兼容）
- 加载配置时检查原始 JSON 是否包含 `poll_interval_ms` key，存在则打印废弃警告
- 实现方式：反序列化前检查 `content.contains("poll_interval_ms")`

### 4. `Cargo.toml`

- 版本号 → 1.0.4
- windows-sys features 不变（`AddClipboardFormatListener` 在 `Win32_System_DataExchange`，`CreateWindowExW` 等在 `Win32_UI_WindowsAndMessaging`，均已包含）
- tao 保留为依赖（tray-icon 可能间接需要），但不再作为事件循环驱动

## 配置兼容性

| 配置项 | v1.0.3 | v1.0.4 | 说明 |
|--------|--------|--------|------|
| `poll_interval_ms` | 轮询间隔 | 保留但不再使用，加载时 warn 提示废弃 | 不删字段，旧配置文件不报错 |
| 其他字段 | 不变 | 不变 | — |

## 实施结果

### 已完成

1. **新增 `clipboard_listener.rs`** — 独立线程 + MessageOnlyWindow + PostThreadMessageW 通知
2. **重写 `main.rs` 事件循环** — Win32 GetMessageW 替代 tao EventLoop，彻底消除 DeviceEvent 噪音
3. **修改 `config.rs`** — poll_interval_ms 废弃提示
4. **版本号更新** — Cargo.toml → 1.0.4

### 测试结果

- 28 个单元测试全部通过
- Release EXE 952KB（< 1MB）
- 零编译错误、零编译 warning
- Windows 实测：空闲 CPU 占用归零，截图即时响应

## 风险与应对

| 风险 | 应对 |
|------|------|
| `AddClipboardFormatListener` 在某些旧 Windows 版本不可用 | 最低要求 Vista（2006 年），实际可忽略 |
| listener 线程 panic | panic 时日志记录，主线程不受影响（只是不再收到通知） |
| tao 仍作为依赖但未使用事件循环 | 不影响功能，后续可考虑移除 tao 依赖 |
