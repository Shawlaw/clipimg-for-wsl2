# v1.0.4 实施计划：剪贴板监听替代轮询

## 背景

当前每 800ms 轮询一次 `arboard::Clipboard::get_image()`，虽然 v1.0.3 的内存 MD5 去重避免了盲写磁盘，但剪贴板中有图片内容时 CPU 占用仍然偏高，任务管理器里排序靠前。

Windows 提供了 `AddClipboardFormatListener` API（Vista+），允许注册窗口接收 `WM_CLIPBOARDUPDATE` 消息，剪贴板内容变化时系统主动通知，无需轮询。

## 技术方案

### 核心思路

创建独立线程 + Message-Only Window 接收 `WM_CLIPBOARDUPDATE`，通过 `std::sync::mpsc` channel 通知主线程的 tao 事件循环。

```
┌─────────────────────┐     channel      ┌──────────────────────┐
│  Clipboard Listener │ ──── notify ────> │   tao Event Loop     │
│  Thread             │                  │   (main thread)      │
│                     │                  │                      │
│  hidden window      │                  │  收到通知后：         │
│  AddClipboardFormat │                  │  1. arboard 读图片   │
│  Listener           │                  │  2. MD5 去重         │
│  GetMessage loop    │                  │  3. 保存 + 设置剪贴板│
└─────────────────────┘                  └──────────────────────┘
```

### 为什么不子类化 tao 窗口

- `tao` 不暴露底层 WndProc 钩子，强行子类化侵入性强、维护成本高
- 独立线程的 MessageOnlyWindow 方案完全解耦，不依赖 tao 内部实现
- listener 线程只做"通知"，不做业务逻辑，极其轻量

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
    tx: Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl ClipboardListener {
    /// 启动监听线程，返回 ClipboardListener 实例
    /// 传入的 Sender 用于通知主线程剪贴板发生了变化
    pub fn start(tx: Sender<()>) -> Result<Self, String>;
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardListener {
    /// 析构时 PostThreadMessage WM_QUIT 通知线程退出
    fn drop(&mut self);
}
```

监听线程内部逻辑：
1. `RegisterClassW` 注册一个极简窗口类
2. `CreateWindowExW` 创建 Message-Only Window（`HWND_MESSAGE` 父窗口）
3. `AddClipboardFormatListener(hwnd)` 注册通知
4. `GetMessageW` 循环，收到 `WM_CLIPBOARDUPDATE` (0x031D) 时 `tx.send(())`
5. 收到 `WM_QUIT` 时退出循环并 `RemoveClipboardFormatListener`

### 2. 修改 `src/main.rs`

事件循环改动：

```rust
// 之前：定时器轮询
*control_flow = ControlFlow::Poll;
if last_poll.elapsed() >= poll_interval {
    let has_new = watcher.poll(&mut clipboard);
    // ...
    last_poll = Instant::now();
}

// 之后：事件驱动
*control_flow = ControlFlow::Wait;  // 无事件时休眠，零 CPU
if let Ok(()) = clip_rx.try_recv() {
    let has_new = watcher.poll(&mut clipboard);
    if has_new && !config.is_hotkey_mode() {
        // 设置多格式剪贴板...
    }
}
```

关键变化：
- `ControlFlow::Poll` → `ControlFlow::Wait`：无事件时线程休眠
- 删除 `last_poll` / `poll_interval` 定时器逻辑
- 用 channel 接收通知替代定时轮询
- 热键事件、菜单事件仍然通过 tao 的事件分发正常工作（`ControlFlow::Wait` 会在有事件时唤醒）

### 3. 修改 `src/config.rs`

- `poll_interval_ms` 字段保留不删除（保持配置文件向后兼容）
- 加载配置时检测到 `poll_interval_ms` 字段存在，打印 `log::warn!("poll_interval_ms 字段已废弃，建议从配置文件中删除")`
- 实现方式：用 `serde` 的 `#[serde(default)]` 无法区分"字段缺失"和"字段存在但等于默认值"，改为加载 JSON 后先检查原始 JSON 是否包含该 key，再反序列化
- 日志中打印 "剪贴板监听模式" 而非 "轮询间隔: 800ms"

### 4. `Cargo.toml` windows-sys features

新增 `Win32_System_LibraryLoader`（`LoadLibraryW` 等，如需要）和确认 `AddClipboardFormatListener` 所需的 feature。`AddClipboardFormatListener` 在 `Win32_UI_WindowsAndMessaging` 中，`CreateWindowExW` 和 `RegisterClassW` 也在同一模块，当前 features 已包含。

## 配置兼容性

| 配置项 | v1.0.3 | v1.0.4 | 说明 |
|--------|--------|--------|------|
| `poll_interval_ms` | 轮询间隔 | 保留但不再使用，加载时 warn 提示废弃 | 不删字段，旧配置文件不报错 |
| 其他字段 | 不变 | 不变 | — |

## 测试策略

### 现有测试不受影响

`clipboard.rs` 中的 `poll_with_data` 及其测试不变，因为核心保存逻辑没有改动。

### 新增测试

**`clipboard_listener.rs` 测试（`#[cfg(target_os = "windows")]`）：**
- 仅 Windows 编译，Linux 上跳过
- 可在集成测试中标记 `#[ignore]`

**`main.rs` 事件循环改动验证：**
- 编译验证：`cargo xwin build --target x86_64-pc-windows-msvc --release`
- 功能验证需在 Windows 环境手动测试

## 实施步骤

### Step 1: 新增 `clipboard_listener.rs`
- 实现 `ClipboardListener` struct + 监听线程
- MessageOnlyWindow + `AddClipboardFormatListener`
- channel 通知
- Drop 时优雅退出

### Step 2: 修改 `main.rs`
- 创建 channel，启动 `ClipboardListener`
- 事件循环改为 `ControlFlow::Wait` + channel try_recv
- 移除定时器轮询逻辑
- 启动日志更新

### Step 3: 编译验证
- `cargo test` 确保现有 28 个测试通过
- `cargo xwin build --target x86_64-pc-windows-msvc --release` 交叉编译

### Step 4: 更新 README + 版本号
- 版本号 → 1.0.4
- README 版本记录新增 v1.0.4 条目
- README 配置说明表格：移除 `poll_interval_ms` 行，标注为已废弃
- todo.md 更新状态

## 风险与应对

| 风险 | 应对 |
|------|------|
| `AddClipboardFormatListener` 在某些旧 Windows 版本不可用 | 最低要求 Vista（2006 年），实际可忽略 |
| listener 线程 panic | 用 `JoinHandle` 监控，panic 时 log::error 并降级回轮询 |
| channel 通知丢失（主线程繁忙时 try_recv 可能漏掉） | 不影响正确性，最多延迟一次通知，下次变化时会再次触发 |
| tao `ControlFlow::Wait` 是否能正常唤醒处理热键/菜单事件 | 需验证。tao 内部使用 PostMessage 唤醒，理论上 `Wait` 模式仍能收到所有窗口消息 |
