# clipImg — WSL2 / Docker 剪贴板图片工具

> **v1.0.0**

在 Windows 复制图片后，在 WSL2 终端（Claude Code CLI、Codex CLI 等）粘贴即可得到图片路径。

## 功能特性

- **自动监控**：Windows 后台运行，实时检测剪贴板截图并保存
- **双模式输入**：
  - **剪贴板模式**（默认）：截图后自动设置多格式剪贴板，Ctrl+V / Shift+Insert 直接粘贴路径
  - **热键模式**：按自定义热键自动输入路径，不碰剪贴板
- **系统托盘**：右键菜单可打开配置、打开图片目录、退出
- **智能去重**：文件大小 + MD5 两级去重，相同图片不重复保存
- **历史清理**：自动清理超过指定天数的旧图片
- **单 EXE**：无运行时依赖，1.9MB，双击即用

---

## 快速开始

### 1. 获取 EXE

从 [Releases](https://github.com/Shawlaw/clipimg-for-wsl2/releases) 下载 `clipimg.exe`，放到任意目录。

### 2. 运行

双击 `clipimg.exe`，任务栏出现托盘图标即表示运行中。

首次运行会在 EXE 同目录自动生成 `config.json`：

```json
{
  "hotkey": "",
  "output_path": "/workspace/.clip/latest.png",
  "save_dir": ".clip",
  "poll_interval_ms": 800,
  "max_history_days": 7
}
```

### 3. 使用

| 步骤 | 操作 |
|------|------|
| 1 | 在 Windows 里截图或 Ctrl+C 复制图片 |
| 2 | 程序自动检测并保存图片（约 1 秒） |
| 3 | **剪贴板模式**：在 WSL 终端里 Ctrl+V 或 Shift+Insert，粘贴出图片路径 |
| 3 | **热键模式**：按配置的热键（如 Alt+Insert），自动输入路径 |

---

## 配置说明

配置文件 `config.json` 放在 EXE 同目录，首次运行自动生成。

| 字段 | 默认值 | 说明 |
|------|--------|------|
| `hotkey` | `""` | 全局热键。**空字符串 = 剪贴板模式**，设置值则启用热键模式（如 `"Alt+Insert"`、`"Ctrl+Shift+V"`） |
| `output_path` | `/workspace/.clip/latest.png` | 粘贴/输入到终端的路径（容器侧路径） |
| `save_dir` | `.clip` | 图片在 Windows 侧的保存目录。相对路径基于 EXE 向上两级（`clipImg/clipimg-app/` → workspace root），也支持绝对路径如 `E:\workspace\.clip` |
| `poll_interval_ms` | `800` | 剪贴板轮询间隔（毫秒） |
| `max_history_days` | `7` | 历史图片最大保留天数 |

两个路径的关系：`save_dir` 是 Windows 文件系统上的实际写入位置，`output_path` 是 WSL 容器内能识别的路径，两者通过目录挂载映射到同一个物理文件。

---

## 输入模式详解

### 剪贴板模式（默认，`"hotkey": ""`）

截图保存后，程序自动设置多格式剪贴板：

| 粘贴到哪里 | 得到什么 |
|-----------|---------|
| WSL 终端（Ctrl+V / Shift+Insert） | 文件路径字符串（如 `/workspace/.clip/latest.png`） |
| 画图等图片应用（Ctrl+V） | 截图图片 |
| 资源管理器 / 文件对话框（Ctrl+V） | PNG 文件 |

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

# 产出: target/x86_64-pc-windows-msvc/release/clipimg.exe (~1.9MB)
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
│   ├── main.rs             # 入口：事件循环 + 托盘 + 双模式分发
│   ├── config.rs           # 配置文件加载/保存/校验
│   ├── clipboard.rs        # 剪贴板轮询 + 图片保存 + MD5 去重 + 历史清理
│   ├── input.rs            # 路径输入：热键模式（SendInput + IME 切换）+ 剪贴板模式（多格式设置）
│   └── logger.rs           # 文件 + 控制台双写日志 + panic handler
├── Cargo.toml
├── config.example.json
└── plans/                  # 方案设计文档
    └── plan0407_v1.md
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
- 在 EXE 所在目录打开 PowerShell 或 cmd，执行 `.\clipimg.exe`，可以看到错误信息
- 常见原因：`config.json` 格式错误、`save_dir` 路径无效、热键被占用
- 程序启动后会生成日志文件 `<save_dir>/.clipimg.log`，可查看详细运行记录

**截图后粘贴/按键没有路径**
- 确认托盘图标存在
- 确认 `config.json` 格式正确
- 确认 `.clip/latest.png` 存在：先在 Windows 里复制一张图，等 1-2 秒再试

**粘贴出来的路径在容器内找不到文件**
- 确认 WSL 挂载路径正确
- 容器内检查：`ls -la /workspace/.clip/`

**热键模式在中文输入法下不生效**
- 热键模式会自动临时切换到英文输入法，如果仍不生效可切换到剪贴板模式（`"hotkey": ""`）

**SendInput 在 UAC 提权窗口无效**
- 这是 Windows 安全限制，非管理员进程无法向管理员进程发送输入

---

## 许可

MIT
