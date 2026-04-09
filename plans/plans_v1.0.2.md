# clipImg v1.0.2 改进计划

## 改进项

### 1. EXE 体积优化（目标 < 1MB）

**现状**：EXE 体积 2.0MB，image crate 编入了所有图片格式编解码器（PNG/JPEG/GIF/WebP/EXR/TIFF/BMP/ICO 等），chrono crate 用于时间戳格式化。

**改进**：
- `image` crate 只启用 PNG 格式（剪贴板读图片走 arboard 的 RGBA 原始像素，image 只用于 PNG 编解码和加载托盘图标）
- 去掉 `chrono` 依赖，改用 `std::time::SystemTime` + 自定义 `days_to_ymd` 函数计算时间戳
- 在 `logger.rs` 中新增 `now_timestamp()`、`filename_timestamp()` 公开函数
- 预期效果：2.0MB → ~948KB（已验证）

### 2. save_dir 路径解析简化

**现状**：相对路径基于 EXE 向上两级（`exe_dir.parent().parent()`），因为开发时 EXE 在 `clipImg/clipimg-app/` 下，需要跳到 workspace root。这个实现细节暴露给了用户，不够直观。

**改进**：
- 相对路径直接基于 EXE 所在目录（`exe_dir`），用户放到哪里就从哪里算
- 去掉两级 parent 逻辑
- 不做自动迁移：首次运行对话框会展示新的解析路径，用户确认即可

### 3. 托盘菜单新增项目主页链接

**现状**：无入口跳转到开源项目地址。

**改进**：
- 托盘菜单新增「项目主页」选项
- 点击后通过 `cmd /c start` 打开浏览器访问 `https://github.com/Shawlaw/clipimg-for-wsl2`
- 位置：放在「打开图片目录」和分隔线之间

---

## 实施顺序

| 步骤 | 改进项 | 涉及文件 |
|------|--------|----------|
| 1 | EXE 体积优化 | `Cargo.toml`, `src/logger.rs`, `src/clipboard.rs` |
| 2 | save_dir 路径简化 | `src/config.rs` |
| 3 | 项目主页菜单 | `src/main.rs` |
| 4 | 更新文档 | `README.md`, `Cargo.toml` version |
