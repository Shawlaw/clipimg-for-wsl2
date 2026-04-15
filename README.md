<p align="center">
  <img src="clipimg-app/icons/icon_128.png" width="128">
</p>

<h1 align="center">clipImg</h1>

<p align="center">WSL2 / Docker 剪贴板图片工具</p>

> 当前版本：**v1.0.7**

在 Windows 截取图片后（目前自测PrintScreen键系统级截屏、QQ快捷键截屏、微信快捷键截屏均有效），在 WSL2 终端（Claude Code CLI、Codex CLI 等）里粘贴即可让多模态模型“看到”图片。

## 功能特性

- **自动监控**：Windows 后台运行，实时检测剪贴板截图并保存
- **双模式输入**：
  - **剪贴板模式**（默认）：截图后自动设置多格式剪贴板，Ctrl+V / Shift+Insert 直接粘贴路径
  - **热键模式**：按自定义热键自动输入路径，不碰剪贴板
- **系统托盘**：右键菜单可打开配置/日志、打开图片目录、开机自启开关、退出
- **文件复制**：支持从资源管理器 Ctrl+C 复制文件，自动保存并设置剪贴板路径
- **预览快捷键**：按快捷键（默认 `Ctrl+Alt+P`）用系统默认程序打开最新文件
- **智能去重**：文件大小 + MD5 两级去重，相同图片不重复保存
- **历史清理**：自动清理超过指定小时数的旧文件（`latest_file.*` 始终保留）
- **单 EXE**：无运行时依赖，约 1MB，双击即用（无控制台黑框）

---

## 快速开始

### 1. 获取 EXE

从 [Releases](https://github.com/Shawlaw/clipimg-for-wsl2/releases) 下载 `clipimg.exe`，放到任意目录。

### 2. 运行

双击 `clipimg.exe`，任务栏出现托盘图标即表示运行中。

首次运行会弹出路径确认对话框，分别确认 Windows 侧和容器侧路径后自动生成 `config.json`：

```json
{
  "hotkey": "",
  "output_path": "/workspace/.clip",
  "save_dir": ".clip",
  "max_history_hours": 1,
  "max_log_size_mb": 1,
  "max_copy_size_mb": 10,
  "preview_hotkey": "Ctrl+Alt+P",
  "blocked_preview_ext": [],
  "show_startup_notification": true
}
```

### 3. 使用

| 步骤 | 操作 |
|------|------|
| 1 | 在 Windows 里截图，或在资源管理器里 Ctrl+C 复制文件 |
| 2 | 程序自动检测并保存（约 1 秒） |
| 3 | **剪贴板模式**：在 WSL 终端里 Ctrl+V 或 Shift+Insert，粘贴出文件路径 |
| 3 | **热键模式**：按配置的热键（如 Alt+Insert），自动输入路径 |
| 4 | 按 `Ctrl+Alt+P`（可配置）预览最新文件 |

---

## 配置说明

配置文件 `config.json` 放在 EXE 同目录，首次运行自动生成。

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `hotkey` | `""` | 全局热键。**空字符串 = 剪贴板模式**，设置值则启用热键模式（如 `"Alt+Insert"`、`"Ctrl+Shift+V"`） |
| `output_path` | `/workspace/.clip` | 粘贴/输入到终端的目录路径（容器侧，自动拼接 `/latest_file.xxx`） |
| `save_dir` | `.clip` | 图片在 Windows 侧的保存目录。相对路径基于 EXE 所在目录，也支持绝对路径（需转义符号"\\"）如 `E:\\workspace\\.clip` |
| `max_history_hours` | `1` | 历史文件最大保留小时数（`latest_file.*` 始终保留） |
| `max_log_size_mb` | `1` | 日志文件最大大小（MB），超过后自动轮转 |
| `max_copy_size_mb` | `10` | Ctrl+C 复制文件的最大允许大小（MB），超过则跳过 |
| `preview_hotkey` | `"Ctrl+Alt+P"` | 预览快捷键，打开最新文件。空字符串 `""` 关闭预览功能 |
| `blocked_preview_ext` | `[]` | 预览时拦截的文件后缀名列表（与内置黑名单取并集），如 `["dll", "reg"]` |
| `show_startup_notification` | `true` | 启动时是否显示提示弹窗 |

**两个路径的关系：`save_dir` 是 Windows 文件系统上的实际写入位置，`output_path` 是 WSL/Docker 容器内能识别的路径，两者通过目录挂载映射到同一个物理文件。**

---

## 输入模式详解

### 剪贴板模式（默认，`"hotkey": ""`）

截图保存后，程序自动设置多格式剪贴板：

| 粘贴到哪里 | 得到什么 |
|-----------|---------|
| WSL 终端（Ctrl+V / Shift+Insert） | 文件路径字符串（如 `/workspace/.clip/latest_file.png`） |
| 画图等图片应用（Ctrl+V） | 截图图片 |
| 资源管理器 / 文件对话框（Ctrl+V） | 文件副本 |

> 不需要自定义热键，不需要键盘模拟，最简单可靠。

### 热键模式（`"hotkey": "Alt+Insert"`）

按热键后，程序临时切换到英文输入法，通过 SendInput + KEYEVENTF_UNICODE 逐字符输入路径，然后恢复原始输入法。整个过程不碰剪贴板。

> 适合需要保留剪贴板内容的场景。

---

## 构建源码

需要 Rust 工具链和 `cargo-xwin`（用于从 Linux 交叉编译 Windows EXE）：

```bash
# 安装交叉编译工具
cargo install cargo-xwin

# 构建
cd clipimg-app/
cargo xwin build --target x86_64-pc-windows-msvc --release

# 产出: target/x86_64-pc-windows-msvc/release/clipimg.exe (~1MB)
```

也可以在 Windows 上直接编译：

```bash
cargo build --release
```

---

## 文件结构

```
clipimg-app/
├── src/
│   ├── main.rs             # 入口：事件循环 + 托盘 + 双模式分发 + 配置热更新 + 预览快捷键
│   ├── config.rs           # 配置文件加载/保存/校验/旧配置迁移
│   ├── clipboard.rs        # 剪贴板图片保存 + 文件复制 + MD5 去重 + 历史清理
│   ├── clipboard_listener.rs # 剪贴板变化监听（Win32 事件驱动，替代轮询）
│   ├── input.rs            # 路径输入：热键模式（SendInput + IME 切换）+ 剪贴板模式（多格式设置）
│   ├── first_run.rs        # 首次运行路径确认对话框（Win32 内存对话框）
│   └── logger.rs           # 文件 + 控制台双写日志 + panic handler
├── assets/                 # UI 资源源文件（不打包进程序，用于后续调整）
│   ├── icon_source.png     # 应用图标设计稿（1024x1024），所有尺寸从此图生成
│   └── icon_raw.png        # 图标草稿/备用版本
├── icons/                  # 编译用图标文件（由 assets/ 生成）
├── examples/
│   ├── gen_icon.rs         # 程序生成简约图标
│   └── convert_icon.rs     # 从设计稿生成各尺寸图标
├── Cargo.toml
├── build.rs                # Windows 资源编译（EXE 图标 + 版本信息）
└── config.example.json
```

---

## 改进方案致谢

本项目在路径输入方案的调研和实现过程中，参考了以下开源项目的思路：

- [**Nailuu/wsl-screenshot-cli**](https://github.com/Nailuu/wsl-screenshot-cli) — 多格式剪贴板方案（CF_UNICODETEXT + CF_DIB + CF_HDROP 同时设置），实现了"同一剪贴板在不同应用粘贴得到不同内容"的效果。本项目的剪贴板模式（方案 C）参考了此方案。

- [**unclejimao/WSL-Image-Clipboard-Helper**](https://github.com/unclejimao/WSL-Image-Clipboard-Helper) — SendInput + KEYEVENTF_UNICODE 配合 IME 临时切换（先切英文输入法，发送文字，再恢复），以及逐字符分开调用 SendInput 修复批量发送 bug 的关键发现。本项目的热键模式（方案 A）参考了此方案。

---

## 经典方案（PowerShell + AutoHotkey）

早期的 PowerShell 守护进程 + AutoHotkey 热键脚本方案已从主分支移除。如需使用该方案，可查看历史提交 [`ac7ccb6`](https://github.com/Shawlaw/clipimg-for-wsl2/tree/ac7ccb6)，其中包含完整的源码、安装步骤和使用说明。

该方案需要分别启动两个进程（PowerShell 守护进程 + AutoHotkey 脚本），并依赖 AutoHotkey 运行时。新用户建议直接使用当前的 Rust 单 EXE 方案。

---

## 故障排查

**程序闪退 / 双击运行没反应**
- 启动失败时会弹窗显示错误信息（如配置文件格式错误、热键被占用等）
- 常见原因：`config.json` 格式错误、`save_dir` 路径无效、热键被占用
- 程序启动后会生成日志文件 `<save_dir>/.clipimg.log`，可通过托盘菜单「打开日志文件」查看
- 如需调试控制台输出，可用 `cargo build --features console` 编译带控制台的版本

**截图后粘贴/按键没有路径**
- 确认托盘图标存在
- 确认 `config.json` 格式正确
- 确认 `.clip/latest_file.*` 存在：先在 Windows 里复制一张图，等 1-2 秒再试

**粘贴出来的路径在容器内找不到文件**
- 确认 WSL 挂载路径正确
- 容器内检查：`ls -la /workspace/.clip/`

**热键模式在中文输入法下不生效**
- 热键模式会自动临时切换到英文输入法，如果仍不生效可切换到剪贴板模式（`"hotkey": ""`）

**SendInput 在 UAC 提权窗口无效**
- 这是 Windows 安全限制，非管理员进程无法向管理员进程发送输入

---

## 版本记录

### v1.0.7

- 修复非 ASCII 路径下开机自启状态检测失效（注册表 UTF-16 转换错误）
- 修复 CF_HDROP 统一指向源文件（与用户复制行为一致）
- 修复重启后热键输入错误路径（启动时从磁盘恢复 latest_file 扩展名）
- 修复配置热重载未同步 ClipboardWatcher 内部配置副本
- 修复切换到剪贴板模式时预览热键连带失效
- 支持预览热键热更新（修改配置后无需重启）
- 反馈环防护从布尔值改为 500ms 时间窗口
- 修复旧配置 max_history_days 未迁移为 max_history_hours（7 天 → 168 小时）
- 修复同秒冲突文件名格式（clip_xxx.png_1 → clip_xxx_1.png）
- max_history_hours = 0 定义为不清理（而非立即删除所有历史）
- 剪贴板设置函数关键格式失败时返回错误（不再静默成功）
- is_png_file 改为只读文件头（不再加载整个文件到内存）
- 配置监控线程改用退出 Event + CancelIoEx（修复 overlapped I/O 泄漏）
- 日志时区从 UTC+8 硬编码改为 GetLocalTime API（自动适配系统时区）

### v1.0.6

- 文件复制支持（CF_HDROP）：从资源管理器 Ctrl+C 复制文件，自动保存并设置多格式剪贴板
- 文件命名统一：`latest.png` → `latest_file.xxx`，支持任意文件类型保留原始后缀
- 预览快捷键：新增 `preview_hotkey` 配置（默认 `Ctrl+Alt+P`），用系统默认程序打开最新文件
- 启动通知：启动时弹出提示框（可通过 `show_startup_notification` 配置关闭）
- 可执行文件预览拦截：内置黑名单 + 用户自定义后缀，防止误运行 exe/bat 等文件
- 依赖升级：global-hotkey 0.7、tray-icon 0.22、windows-sys 0.60，移除 windows crate 减小体积
- 配置自动迁移：旧配置文件自动补充新字段（`max_copy_size_mb`、`preview_hotkey`、`blocked_preview_ext`、`show_startup_notification`）

### v1.0.5

- 移除 tao 依赖：从 Cargo.toml 移除不再使用的 tao crate，减小编译时间和产物体积
- 配置路径优化：`output_path` 从文件级改为目录级（如 `/workspace/.clip`，不含 `latest.png`），旧配置自动兼容
- 首次运行双路径引导：对话框同时展示 Windows 侧和容器侧路径，说明挂载映射关系
- 配置热更新：支持文件监控自动重载（`ReadDirectoryChangesW`）+ 托盘菜单手动重载，修改配置后无需重启
- 热键热切换：配置重载时自动反注册旧热键、注册新热键，支持运行时切换热键模式/剪贴板模式
- `poll_interval_ms` 配置项自动清理：加载时从配置文件中删除并回写

### v1.0.4

- 剪贴板监听替代轮询：使用 Win32 `AddClipboardFormatListener` 事件驱动，空闲时 CPU 占用归零，截图即时响应
- 移除 tao 事件循环：改用原生 Win32 `GetMessageW` 消息循环，消除 `DeviceEvent` 导致的无效唤醒
- `poll_interval_ms` 配置项废弃：旧配置文件中保留的字段会在 v1.0.5 加载时自动删除

### v1.0.3

- 剪贴板轮询去盲写磁盘：先在内存中比对 MD5，内容没变不写磁盘
- 日志循环写：超过配置大小（`max_log_size_mb`，默认 1MB）自动轮转，防止撑爆磁盘
- 多实例防护：启动时检测互斥体，已有实例运行则弹窗提示并退出

### v1.0.2

- EXE 体积从 2.0MB 缩减至不到 1MB（image crate 只保留 PNG 编解码，移除 chrono 依赖）
- save_dir 路径解析简化：相对路径直接基于 EXE 所在目录，不再向上跳两级
- 托盘菜单新增「项目主页」选项，点击打开 GitHub 仓库地址

### v1.0.1

- 去掉控制台黑框：默认以 Windows 子系统运行，双击即用无黑窗口；编译时加 `--features console` 可保留控制台用于调试
- 应用图标：EXE 文件图标、系统托盘图标、Windows 属性面板版本信息（产品名称、版本号、文件版本）
- 开机自启：托盘菜单新增勾选框开关，通过读写注册表 `HKCU\...\Run` 实现
- 首次运行引导：弹出路径确认对话框，用户可修改图片保存目录后确认生成 `config.json`
- 历史保留改为小时级：`max_history_days` → `max_history_hours`（默认 1 小时），旧配置文件自动兼容
- 启动失败弹窗提示：配置错误、热键被占用等问题会弹出错误信息，不再闪退无反馈
- 建立 `assets/` 目录管理 UI 资源源文件

### v1.0.0

虽然很简陋，但的确是可用的第一个版本。

---

## 许可

MIT
