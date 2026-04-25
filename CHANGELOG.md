# Changelog

All notable changes to this project will be documented in this file.

## v1.0.12

- **新增 debug 构建模式**：`cargo xwin build --features debug_build --release --bin clipimg_debug` 编译 `clipimg_debug.exe`，启动时自动终止 release 版本进程，托盘显示 `(debug)` 标记，方便迭代测试
- **release 自动清理 debug 版本**：启动时检测并终止同目录下的 `clipimg_debug.exe` 进程后删除文件，实现测试闭环

## v1.0.11

- **托盘菜单新增"打开程序目录"**：方便用户下载新版后快速定位 EXE 所在目录覆盖替换

## v1.0.10

- 切换公共基础能力到公开的 [DeskFoundry](https://github.com/Shawlaw/DeskFoundry) monorepo GitHub 依赖
- `desktop-logger`、`desktop-config`、`desktop-i18n`、`desktop-fs` 现在统一从共享 SDK 获取
- 版本号提升到 `1.0.10`

## v1.0.9

- **修复 CF_HDROP 指向源文件**：v1.0.8 重构多文件支持时丢失了 v1.0.7 的修复，导致从资源管理器复制文件后粘贴得到的是 `.clip` 副本而非源文件。新增 `build_file_clipboard_params` 辅助函数文档化不变式（CF_UNICODETEXT 用容器路径，CF_HDROP 用源文件路径），并补充回归测试

## v1.0.8

- **去掉 latest_file 机制，支持连续粘贴**：每次截图/复制产生唯一路径（`clip_<timestamp>.<ext>`），不再覆盖历史路径
- **文件名时间戳精度提升到毫秒级**：`clip_YYYYMMDD_HHmmSSmmm.<ext>`，进一步降低冲突概率
- **多文件 CF_HDROP 粘贴**：从资源管理器复制多个文件，一次性保存并写入多行路径到剪贴板（每行一个路径 + 末尾空行）
- **max_copy_files 配置项**：限制单次最多处理的文件数（默认 10），防止误复制大量文件导致卡顿
- **UNC 路径支持**：`\\wsl$\...` 和 `\\wsl.localhost\...` 格式的 save_dir 可正常使用
- **UNC 不可用容错**：WSL 未启动时截图/复制不崩溃，自动跳过并弹窗提示；WSL 启动后自动恢复并通知用户
- **升级兼容**：启动时自动将旧版 `latest_file.*` / `latest.png` 按 mtime 重命名为 `clip_*` 格式

## v1.0.7

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

## v1.0.6

- 文件复制支持（CF_HDROP）：从资源管理器 Ctrl+C 复制文件，自动保存并设置多格式剪贴板
- 文件命名统一：`latest.png` → `latest_file.xxx`，支持任意文件类型保留原始后缀
- 预览快捷键：新增 `preview_hotkey` 配置（默认 `Ctrl+Alt+P`），用系统默认程序打开最新文件
- 启动通知：启动时弹出提示框（可通过 `show_startup_notification` 配置关闭）
- 可执行文件预览拦截：内置黑名单 + 用户自定义后缀，防止误运行 exe/bat 等文件
- 依赖升级：global-hotkey 0.7、tray-icon 0.22、windows-sys 0.60，移除 windows crate 减小体积
- 配置自动迁移：旧配置文件自动补充新字段（`max_copy_size_mb`、`preview_hotkey`、`blocked_preview_ext`、`show_startup_notification`）

## v1.0.5

- 移除 tao 依赖：从 Cargo.toml 移除不再使用的 tao crate，减小编译时间和产物体积
- 配置路径优化：`output_path` 从文件级改为目录级（如 `/workspace/.clip`，不含 `latest.png`），旧配置自动兼容
- 首次运行双路径引导：对话框同时展示 Windows 侧和容器侧路径，说明挂载映射关系
- 配置热更新：支持文件监控自动重载（`ReadDirectoryChangesW`）+ 托盘菜单手动重载，修改配置后无需重启
- 热键热切换：配置重载时自动反注册旧热键、注册新热键，支持运行时切换热键模式/剪贴板模式
- `poll_interval_ms` 配置项自动清理：加载时从配置文件中删除并回写

## v1.0.4

- 剪贴板监听替代轮询：使用 Win32 `AddClipboardFormatListener` 事件驱动，空闲时 CPU 占用归零，截图即时响应
- 移除 tao 事件循环：改用原生 Win32 `GetMessageW` 消息循环，消除 `DeviceEvent` 导致的无效唤醒
- `poll_interval_ms` 配置项废弃：旧配置文件中保留的字段会在 v1.0.5 加载时自动删除

## v1.0.3

- 剪贴板轮询去盲写磁盘：先在内存中比对 MD5，内容没变不写磁盘
- 日志循环写：超过配置大小（`max_log_size_mb`，默认 1MB）自动轮转，防止撑爆磁盘
- 多实例防护：启动时检测互斥体，已有实例运行则弹窗提示并退出

## v1.0.2

- EXE 体积从 2.0MB 缩减至不到 1MB（image crate 只保留 PNG 编解码，移除 chrono 依赖）
- save_dir 路径解析简化：相对路径直接基于 EXE 所在目录，不再向上跳两级
- 托盘菜单新增「项目主页」选项，点击打开 GitHub 仓库地址

## v1.0.1

- 去掉控制台黑框：默认以 Windows 子系统运行，双击即用无黑窗口；编译时加 `--features console` 可保留控制台用于调试
- 应用图标：EXE 文件图标、系统托盘图标、Windows 属性面板版本信息（产品名称、版本号、文件版本）
- 开机自启：托盘菜单新增勾选框开关，通过读写注册表 `HKCU\...\Run` 实现
- 首次运行引导：弹出路径确认对话框，用户可修改图片保存目录后确认生成 `config.json`
- 历史保留改为小时级：`max_history_days` → `max_history_hours`（默认 1 小时），旧配置文件自动兼容
- 启动失败弹窗提示：配置错误、热键被占用等问题会弹出错误信息，不再闪退无反馈
- 建立 `assets/` 目录管理 UI 资源源文件

## v1.0.0

虽然很简陋，但的确是可用的第一个版本。
