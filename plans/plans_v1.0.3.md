# clipImg v1.0.3 改进计划

## 改进项

### 1. 剪贴板轮询去盲写磁盘

**现状**：`poll_with_data()` 每次轮询都先将 RGBA 数据编码为 PNG 写入 `_tmp_clip.png`，再做 MD5 去重比对。即使剪贴板内容没变，每 800ms 就写一次磁盘。

**改进**：
- 在内存中直接对 RGBA 数据计算 MD5，与上一次保存的 MD5 比对
- `ClipboardWatcher` 新增 `last_md5: Option<Vec<u8>>` 字段，缓存上次保存图片的 MD5
- MD5 相同则跳过，不写磁盘；不同才执行现有的保存流程
- 去掉 `_tmp_clip.png` 中间文件，直接写入最终的历史文件名

**涉及文件**：`src/clipboard.rs`

### 2. 日志循环写（Log Rotation）

**现状**：`.clipimg.log` 无限追加，长期运行可能撑爆磁盘。

**改进**：
- `FileAndConsoleLogger` 每次 `log()` 时检查日志文件大小
- 新增配置项 `max_log_size_mb`（默认 1，最小值 1）
- 超过限制时：将当前日志重命名为 `.clipimg.log.old`（覆盖旧的），创建新日志文件
- 简单实现：不保留多个历史日志，只保留一份 `.old`
- 旧配置文件自动补全该字段

**涉及文件**：`src/logger.rs`, `src/config.rs`, `Cargo.toml`（version → 1.0.3）

### 3. 多实例运行防护

**现状**：可以同时启动多个 `clipimg.exe`，互相干扰（写同一个 `latest.png`、重复注册热键、重复轮询剪贴板）。

**改进**：
- 启动时创建 Win32 命名互斥体（Named Mutex）：`Global\clipimg`
- 如果互斥体已存在，说明已有实例在运行，弹窗提示后退出
- 使用 `windows-sys` 的 `CreateMutexW` + `GetLastError` 检测
- 互斥体在进程退出时自动释放，无需手动清理

**涉及文件**：`src/main.rs`, `Cargo.toml`（windows-sys 新增 `Win32_Security` feature）

---

## 实施顺序

| 步骤 | 改进项 | 涉及文件 |
|------|--------|----------|
| 1 | 盲写磁盘优化 | `src/clipboard.rs` |
| 2 | 日志循环写 | `src/logger.rs`, `src/config.rs` |
| 3 | 多实例防护 | `src/main.rs`, `Cargo.toml` |
| 4 | 更新版本号和 README | `Cargo.toml`, `README.md` |
