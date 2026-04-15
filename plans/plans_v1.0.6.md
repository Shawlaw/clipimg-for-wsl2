# v1.0.6 实施计划（修订版）

> 原计划在实施过程中有调整，本文档反映最终实现。

## 版本目标

六项功能：CF_HDROP 文件复制、保存时清理过期文件、启动提示弹窗、预览快捷键、可执行文件预览拦截、配置自动迁移。

---

## 功能一：CF_HDROP 文件复制

### 背景

当前只处理剪贴板中的图片数据（DIB/Bitmap），不支持从资源管理器 Ctrl+C 复制文件。用户有"复制任意文件 → 粘贴出容器内路径"的需求（如让 AI 读取文档）。

### 文件命名规则

将 `latest.png` 改为 `latest_file.xxx`，与历史文件 `clip_YYYYMMDD_HHmmss.xxx` 统一：

| 来源 | latest 命名 | 说明 |
|------|-------------|------|
| 截图（DIB → PNG） | `latest_file.png` | 现有截图流程 |
| CF_HDROP + PNG 文件 | `latest_file.png` | 多格式剪贴板（含 CF_DIB） |
| CF_HDROP + 其他文件（如 PDF） | `latest_file.pdf` | 文本+文件剪贴板 |
| CF_HDROP + 无后缀文件 | `latest_file` | 文本+文件剪贴板 |

- 目录内始终只保留一个 `latest_file.*`，保存新的前删除旧的
- 历史文件保留原始后缀 `clip_YYYYMMDD_HHmmss.xxx`
- 清理逻辑：扫描所有 `clip_*` 文件（不限定后缀）

### 两条处理路径

主线程收到剪贴板变化通知后：

```
1. 尝试读取 CF_HDROP（Win32 API）
   ├─ 有文件 → 检查文件大小
   │   ├─ 超过 max_copy_size_mb → 跳过，日志 warn
   │   └─ 大小 OK → 复制到 save_dir（保留原始后缀）
   │       ├─ PNG 文件头 → 更新 latest_file.png，设多格式剪贴板（CF_UNICODETEXT + CF_DIB + CF_HDROP）
   │       └─ 非 PNG → 更新 latest_file.xxx，设文本+文件剪贴板（CF_UNICODETEXT + CF_HDROP）
   └─ 无文件 → 走现有 arboard DIB 流程（保存为 latest_file.png，设多格式剪贴板）
```

### 内存状态

- `ClipboardWatcher` 新增 `latest_container_path: RefCell<String>` 字段
- 记录最近一次操作的容器侧完整路径（如 `/workspace/.clip/latest_file.png`）
- 热键模式发送 `latest_container_path`，剪贴板模式用它设 CF_UNICODETEXT

### 实际改动

- `clipboard.rs`：新增 `copy_file()`、`is_png_file()`、`latest_container_path`、`update_latest_from_history()`、`remove_latest_file()`、`migrate_old_latest()`；`poll_with_data()` 保存后调用 `clean_old_files()`
- `input.rs`：新增 `set_text_and_file_clipboard()`（非图片文件的 CF_UNICODETEXT + CF_HDROP）
- `main.rs`：剪贴板变化通知后先检查 CF_HDROP（`read_clipboard_files()`），再走 DIB 流程
- `config.rs`：新增 `max_copy_size_mb`（默认 10）、`resolved_output_path_for(extension)` 方法

---

## 功能二：保存时清理过期文件

### 实现

- `clipboard.rs`：`poll_with_data()` 和 `copy_file()` 保存新文件后均调用 `clean_old_files()`
- `clean_old_files()` 匹配所有 `clip_*` 文件（不限定 `.png`）
- 频率低（只在有新文件时触发），不影响性能

---

## 功能三：启动提示弹窗

### 背景

程序启动后无任何反馈，用户不确定是否成功运行。

### 实现方案

- 使用 `MessageBoxW`（Win32 API，零额外依赖）显示启动提示
- 内容格式：`clipImg v{version} 已启动 [{模式}]`，并提示可通过配置关闭
- 通过独立线程弹窗，避免阻塞主消息循环
- 新增配置项 `show_startup_notification`（默认 `true`），用户可在配置文件中关闭

### 为什么不用 Toast 通知

尝试过 WinRT `ToastNotification` API（通过 `windows` crate），发现：
- `ToastGeneric` 模板对未打包桌面应用（unpackaged desktop app）静默丢弃通知，无论 AUMID 如何注册
- 经典 Toast 模板（`ToastText02`）可用但无法显示应用图标
- 注册表 AUMID IconUri、Start Menu 快捷方式等方案均未解决图标问题
- 最终决定放弃 Toast，改用 `MessageBoxW`，零依赖且始终可用

### 实际改动

- `main.rs`：`show_notification()` 改用 `MessageBoxW`，条件由 `show_startup_notification` 控制
- `config.rs`：新增 `show_startup_notification` 字段 + serde default + 迁移逻辑
- 移除了 `windows` crate 依赖（减小 EXE 体积约 100KB）

---

## 功能四：预览快捷键

### 设计

- 新增配置项 `preview_hotkey`（默认 `"Ctrl+Alt+P"`）
- 在热键模式和剪贴板模式下均可使用
- 触发时用 `cmd /c start` 打开 `latest_file.*`（用系统默认程序）
- 与现有 `hotkey`（路径输入热键）独立，注册第二个全局热键

### 可执行文件拦截

- 预览时检查文件后缀，可执行文件不允许通过预览打开（防止误运行恶意文件）
- 内置黑名单：exe, bat, cmd, ps1, vbs, js, msi, scr, com, sh, py, pl 等
- 新增配置项 `blocked_preview_ext`（默认空数组），用户可扩展黑名单
- 两者取并集

### 实际改动

- `config.rs`：新增 `preview_hotkey`、`blocked_preview_ext` 字段
- `main.rs`：启动时注册预览热键；消息循环中区分输入热键和预览热键事件；托盘菜单显示预览快捷键信息
- 托盘菜单新增 `preview_item` 显示预览快捷键状态

### 未实现

- 配置热更新时不支持预览热键的重新注册（需重启生效）

---

## 功能五：配置自动迁移

### 实现

- `config.rs` 的 `migrate_config()` 方法自动为旧配置文件补充 v1.0.6 新字段：
  - `max_copy_size_mb` → `10`
  - `preview_hotkey` → `"Ctrl+Alt+P"`
  - `blocked_preview_ext` → `[]`
  - `show_startup_notification` → `true`
- 继续清理 v1.0.5 已废弃的 `poll_interval_ms`

---

## 依赖变更

| 依赖 | 旧版本 | 新版本 | 原因 |
|------|--------|--------|------|
| `global-hotkey` | 0.6 | 0.7 | 版本升级 |
| `tray-icon` | 0.21 | 0.22 | 版本升级 |
| `windows-sys` | 0.59 | 0.60 | 版本升级，新增 `Win32_UI_Shell` feature |
| `windows` | 0.60+ | 移除 | Toast 通知方案放弃，不再需要 |

---

## 配置兼容性

| 配置项 | v1.0.5 | v1.0.6 | 兼容处理 |
|--------|--------|--------|----------|
| `max_copy_size_mb` | 不存在 | 新增，默认 10 | serde default + 迁移 |
| `preview_hotkey` | 不存在 | 新增，默认 `"Ctrl+Alt+P"` | serde default + 迁移 |
| `blocked_preview_ext` | 不存在 | 新增，默认 `[]` | serde default + 迁移 |
| `show_startup_notification` | 不存在 | 新增，默认 `true` | serde default + 迁移 |

---

## 实施步骤（实际执行顺序）

1. `config.rs` — 新增配置字段 + `resolved_output_path_for(extension)`
2. `clipboard.rs` — CF_HDROP 文件复制 + 命名统一 + 清理逻辑
3. `input.rs` — 新增 `set_text_and_file_clipboard()`
4. `main.rs` — CF_HDROP 集成 + 预览热键 + 启动弹窗 + 可执行文件拦截
5. 依赖升级 + 移除 `windows` crate
6. 文档更新（README、config.example.json、计划文档）
