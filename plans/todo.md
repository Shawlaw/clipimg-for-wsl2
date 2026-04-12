# clipImg 待办事项 & 优化方向

## 用户反馈的问题

### 1. 剪贴板轮询盲写磁盘

**现状**：每次轮询都会将剪贴板图片写入 `_tmp_clip.png`，然后再做去重比对。即使内容没变，也在反复写磁盘。

**思路**：先在内存中计算剪贴板图片的 MD5，与 `latest.png` 的 MD5 比对，不同才写磁盘。需要保留上一次的 MD5 值避免重复读文件。

### 2. 配置路径优化：首次启动双路径引导 + output_path 改目录级

**现状**：
- 首次启动只确认 `save_dir`，`output_path` 用默认值 `/workspace/.clip/latest.png`
- 用户挂载路径不是 `/workspace` 时，粘贴出的路径在容器内找不到文件
- `output_path` 是文件级路径（含 `latest.png`），与 `save_dir`（目录级）概念不对等

**思路**：

**config 层改动：**
- `output_path` 语义从文件级改为目录级（`/workspace/.clip`，不含 `latest.png`）
- 新增 `config.resolved_output_path()` 方法，返回 `format!("{}/latest.png", output_path)`
- 所有使用 `output_path` 的地方改为调用该方法（main.rs、input.rs）
- 旧配置兼容：加载时检测 `output_path` 以 `/latest.png` 结尾则自动截断，warn 日志提示，并将截断后的值回写配置文件

**首次启动对话框改动：**
- 改为双输入框，上方说明"两个路径指向同一个物理目录（WSL2 挂载）"
- Windows 侧（程序实际写入）：展示解析后的绝对路径，如 `E:\...\workspace\.clip`
- 中间用 `↕ 挂载映射` 标注对应关系
- 容器侧（粘贴到终端的路径）：默认 `/workspace/.clip`
- 用户确认后保存两个路径到 config.json

**涉及文件：** config.rs、first_run.rs、main.rs、input.rs、README.md、config.example.json

### 3. 支持直接复制文件（CF_HDROP）

**现状**：只处理剪贴板中的图片数据（DIB/Bitmap），不支持从资源管理器 Ctrl+C 复制图片文件。

**思路**：
- 检测剪贴板是否包含 CF_HDROP 格式（文件引用）
- 如果是图片文件（png/jpg/bmp/gif/webp），直接复制到 save_dir 并设置多格式剪贴板
- 非图片文件忽略
- 需要 arboard 或直接 Win32 API 读取 CF_HDROP 数据

### 4. 日志循环写（Log Rotation）

**现状**：日志文件无限追加，长期运行可能撑爆磁盘。

**思路**：
- 新增配置项 `max_log_size_mb`（默认 1MB）
- 超过限制时截断：保留后半部分，或重命名为 `.log.old` 后重新创建
- 简单实现即可，不需要复杂的日志框架

---

### 4.5 剪贴板监听替代轮询

**现状**：每 800ms 轮询一次 `arboard::Clipboard::get_image()`，虽然内存 MD5 去重避免了盲写磁盘，但剪贴板有图片内容时 CPU 占用仍然偏高，任务管理器里排序靠前。

**思路**：
- 使用 Win32 `AddClipboardFormatListener` 注册窗口接收 `WM_CLIPBOARDUPDATE` 消息
- 剪贴板变化时系统主动回调，无需轮询，零空闲 CPU
- 两种实现路径：
  - A. 子类化 tao 窗口的 WndProc，拦截消息（侵入性强）
  - B. 创建独立 MessageOnlyWindow + 专用线程接收通知，通过 channel 通知主线程（更干净）
- 收到通知后再调 `arboard` 读取图片数据，现有去重逻辑不变

**收益**：响应延迟从 ~800ms 降至近乎即时，空闲时 CPU 占用归零。

### 12. GitHub Actions 构建 EXE 被 SmartScreen 拦截

**现状**：GitHub Actions 编译产出的 `clipimg.exe` 下载到 Windows 后运行时，Microsoft Defender SmartScreen 弹出"无法识别的应用"警告，需要用户点击"更多信息"→"仍要运行"才能执行。影响首次使用体验。

**思路**：
- 根本原因是 EXE 没有代码签名证书，SmartScreen 对未签名/未知 EXE 会拦截
- 方案 A：购买代码签名证书（EV 或 OV），CI 中用 `signtool` 签名 EXE（成本高，EV 证书约 $300-400/年）
- 方案 B：提交到 Microsoft Defender for Endpoint 排除名单（需要 Microsoft 合作伙伴计划）
- 方案 C：发布时附带 SHA256 校验和，README 中说明如何验证文件完整性并添加信任
- 方案 D：发布到 Microsoft Store（自动受信任，但需开发者账号 $19）
- 方案 E：打包为 MSI 安装包并签名，安装后的 EXE 不再被拦截
- 短期可接受：README 中加说明提示用户这是正常现象

---

## 可考虑的优化方向

### 5. 配置文件热更新

**现状**：修改 config.json 后需要重启 EXE 才能生效。

**思路（B + C 双保险）：**

**B. 托盘菜单"重新加载配置"：**
- 手动触发，作为兜底方案
- 用户编辑配置后点一下立即生效

**C. Win32 文件监控自动重载：**
- 使用 `ReadDirectoryChangesW` 监听 config.json 所在目录的文件变化，事件驱动零 CPU
- 检测到 config.json 变化后通过 `PostThreadMessageW` 通知主线程重载
- 和剪贴板监听线程同样的模式

**运行时重载范围：**
- `output_path`、`save_dir`、`max_history_hours`、`max_log_size_mb` → 直接更新内存值
- `hotkey` → 反注册旧热键 + 注册新热键，热键模式/剪贴板模式切换也在这里处理
- `poll_interval_ms` → 已废弃，加载时从配置文件中删除并回写

**涉及文件：** config.rs（reload 方法）、main.rs（菜单项 + 文件监控线程 + 重载逻辑）、可能新增 config_watcher.rs

### 6. 截图成功托盘通知

**现状**：截图保存成功后无反馈，用户不确定是否生效。

**思路**：保存新图片后，托盘图标显示气泡通知（Balloon Tooltip），显示文件名和路径，3 秒后自动消失。可配置开关（`show_notification`）。

### 7. EXE 不可知自身版本号的功能

**现状**：版本号硬编码在 Cargo.toml，托盘菜单和日志能读到，但用户无法从外部快速确认。

**思路**：版本号已经通过 build.rs 写入 EXE 资源，属性面板可查看。也可以在托盘菜单的状态行显示（已实现）。

### 8. 支持 GIF 动图

**现状**：只保存静态 PNG。如果剪贴板中有 GIF 动图（如浏览器复制），只会保存第一帧。

**思路**：优先级低。需要检测剪贴板是否包含多帧数据，用 image crate 的 GIF 编解码保存为 `.gif`。但实际使用场景较少，且 `image` crate 已精简为只支持 PNG。

### 9. 多实例运行防护

**现状**：可以同时启动多个 clipimg.exe，会互相干扰（写同一个 latest.png、重复注册热键）。

**思路**：启动时通过命名互斥体（Named Mutex）检测是否已有实例运行，如果有则提示并退出。

### 10. 便携模式 vs 安装模式区分

**现状**：配置和日志都在 EXE 旁边的 `.clip` 目录，属于便携模式。如果用户放到 Program Files 下，可能没有写入权限。

**思路**：检测 EXE 所在目录是否可写，不可写时回退到 `%APPDATA%/clipImg/` 存放配置和日志。

### 11. GitHub Actions 自动构建发布

**现状**：每次发布需要本地 `cargo xwin build`，手动上传 EXE 到 GitHub Releases。

**思路**：
- 创建 `.github/workflows/release.yml`
- 触发条件：推送 `v*` tag（如 `v1.0.2`）
- 使用 `cargo-xwin` + `x86_64-pc-windows-msvc` target 交叉编译
- 编译成功后自动创建 GitHub Release，附带 `clipimg.exe`
- 本地只需 `git tag v1.0.2 && git push origin v1.0.2` 即可触发

### 13. 移除 tao 依赖

**现状**：v1.0.4 已将事件循环从 tao 切换为 Win32 原生 `GetMessageW`，tao 不再参与事件循环驱动。但 `Cargo.toml` 中仍保留 `tao = "0.3"` 依赖（`tray-icon` 可能间接需要）。

**思路**：
- 确认 `tray-icon` 是否真的依赖 tao（查看编译依赖树 `cargo tree`）
- 如果 tray-icon 不强制依赖 tao，从 `Cargo.toml` 移除 tao
- 如果 tray-icon 依赖 tao，评估是否可以换用不依赖 tao 的托盘图标方案，或接受现状
- 移除后可减小 EXE 体积和编译时间

---

## 优先级建议

| 优先级 | 项目 | 原因 | 状态 | 实现版本 | 实现日期 |
|--------|------|------|------|----------|----------|
| P0 | 1. 盲写磁盘 | 影响磁盘寿命和性能 | **已实现** | v1.0.3 | 2026-04-10 |
| P0 | 4. 日志循环写 | 长期运行必须 | **已实现** | v1.0.3 | 2026-04-10 |
| P0 | 11. GitHub Actions 自动发布 | 释放本地构建负担 | **已实现** | - | 2026-04-10 |
| P1 | 9. 多实例防护 | 避免用户误操作 | **已实现** | v1.0.3 | 2026-04-10 |
| P1 | 4.5 剪贴板监听替代轮询 | 降低 CPU 占用，即时响应 | **已实现** | v1.0.4 | 2026-04-12 |
| P1 | 6. 截图通知 | 改善用户体验 | 待实现 | | |
| P2 | 2. 路径检测 | 帮助用户排错 | **已实现** | v1.0.5 | 2026-04-10 |
| P2 | 3. 支持复制文件 | 扩展使用场景 | 待实现 | | |
| P3 | 5. 配置热更新 | 便利性提升 | **已实现** | v1.0.5 | 2026-04-10 |
| P3 | 10. 便携/安装模式 | 边缘场景 | 待实现 | | |
| P1 | 12. SmartScreen 拦截 | 影响首次使用体验 | 待实现 | | |
| P2 | 13. 移除 tao 依赖 | 减小体积和编译时间 | **已实现** | v1.0.5 | 2026-04-10 |
