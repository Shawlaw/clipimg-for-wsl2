# v1.0.7 实施计划

> 基于两份代码审查报告（[reviewByOpus4.6.md](v1.0.6_review/reviewByOpus4.6.md)、[reviewByGpt5.4.md](v1.0.6_review/reviewByGpt5.4.md)）整理，去重后共 13 项修复。

## 优先级分类

| 优先级 | 编号 | 问题 |
|--------|------|------|
| P0 功能错误 | 1 | 注册表 UTF-16 转换错误，非 ASCII 路径下开机自启失效 |
| P0 功能错误 | 2 | 非 PNG 文件 CF_HDROP 指向源文件而非保存副本 |
| P0 功能错误 | 3 | 启动时未从磁盘恢复 latest_container_path，热键可能输入错误路径 |
| P0 功能错误 | 4 | 配置热重载未同步 watcher 内部 config 副本 |
| P0 功能错误 | 5 | 切换到剪贴板模式时释放热键管理器，预览热键连带失效 |
| P1 配置兼容 | 6 | 旧配置 max_history_days 未迁移为 max_history_hours |
| P1 命名错误 | 7 | 同秒冲突文件名格式错误（clip_xxx.png_1 应为 clip_xxx_1.png） |
| P1 健壮性 | 8 | 反馈环防护改为 500ms 时间窗口 |
| P1 配置边界 | 9 | max_history_hours = 0 定义为"不清理" |
| P1 预览热更新 | 10 | 配置热重载时补充预览热键的重新注册 |
| P2 健壮性 | 11 | 配置监控线程 overlapped I/O 超时处理 + 退出机制优化 |
| P2 健壮性 | 12 | 剪贴板设置函数部分失败时返回错误而非静默成功 |
| P2 性能 | 13 | is_png_file 只读文件头而非整个文件 |
| P2 时区 | 14 | logger.rs UTC+8 硬编码改为 GetLocalTime API |

---

## P0-1：注册表 UTF-16 转换错误

**文件**：`main.rs` `is_autostart_enabled()`

**问题**：`c as u8 as char` 截断高字节，非 ASCII 路径下比对永远失败。

**修复**：
```rust
let char_len = (buf_len as usize / 2).saturating_sub(1);
let stored = String::from_utf16_lossy(&buf[..char_len]);
let exe_str = exe_path.to_string_lossy();
stored.contains(exe_str.as_ref())
```

同时将固定 512 u16 buffer 改为动态分配（先查长度再读）。

---

## P0-2：CF_HDROP 统一指向源文件

**文件**：`main.rs` 剪贴板变化处理（PNG 文件复制分支）

**问题**：PNG 文件复制时 `set_multi_format_clipboard(&container_path, &win_path)` 的 `win_path` 指向了 `save_dir/latest_file.png`（副本），而 HDROP 应统一指向用户复制的源文件。非 PNG 分支已经正确指向 `first_file`。

**设计决策**：CF_HDROP 是给 Windows 资源管理器粘贴用的，指向原始文件最合理；WSL2 终端粘贴走 CF_UNICODETEXT（容器路径），与 HDROP 无关。PNG 多格式剪贴板的意义在于 CF_DIB（图片应用粘贴为图片数据）。

**修复**：PNG 文件复制分支的 `win_path` 改为 `first_file`（与 DIB 截图流程区分，截图流程没有源文件，HDROP 指向保存文件是正确的）。

---

## P0-3：启动时未恢复 latest_container_path

**文件**：`clipboard.rs` `ClipboardWatcher::new()`

**问题**：`latest_container_path` 硬编码为 `resolved_output_path_for("png")`，启动后未扫描磁盘上实际存在的 `latest_file.*`。

**修复**：新增 `sync_latest_from_disk()` 方法，在 `new()` 后调用：
1. 扫描 save_dir 中 `latest_file.*`
2. 解析扩展名
3. 更新 `latest_container_path`

---

## P0-4：配置热重载未同步 watcher

**文件**：`main.rs` `do_reload_config()`

**问题**：watcher 内部 config 副本未更新，max_history_hours、max_copy_size_mb、output_path 等修改不生效。

**修复**：
- 将 `watcher: &Rc<RefCell<ClipboardWatcher>>` 传入 `do_reload_config()`
- 重载成功后同步 `watcher.borrow_mut().config = new_config.clone()`
- 同时刷新 `watcher.save_dir`（如果 save_dir 变了）

---

## P0-5 + P1-10：热键管理器与预览热键热更新

**文件**：`main.rs` `do_reload_config()`

**问题**：
- 切换到剪贴板模式时 `*hotkey_manager.borrow_mut() = None` 释放了整个管理器，预览热键连带失效
- 预览热键变更时没有反注册旧键、注册新键

**修复**：
1. 切换到剪贴板模式时只反注册输入热键，不释放管理器（如果预览热键存在）
2. 只有当输入热键和预览热键都不存在时才释放管理器
3. 在 `do_reload_config()` 中补充预览热键的差异比较和重新注册逻辑

---

## P1-6：max_history_days → max_history_hours 迁移

**文件**：`config.rs` `migrate_config()`

**问题**：旧配置 `max_history_days` 被忽略，直接回退到默认 1 小时。以前配 7 天的用户升级后会变成 1 小时清理。

**修复**：在 `migrate_config()` 中增加转换逻辑：
```rust
if !obj.contains_key("max_history_hours") {
    if let Some(days) = obj.remove("max_history_days").and_then(|v| v.as_u64()) {
        obj.insert("max_history_hours".into(), serde_json::json!(days * 24));
        changed = true;
    }
}
```
更新对应测试：`7 天 → 168 小时`。

---

## P1-7：同秒冲突文件名格式错误

**文件**：`clipboard.rs` `unique_history_path()`

**问题**：`clip_20260415_120000.png_1` 应为 `clip_20260415_120000_1.png`。

**修复**：
```rust
format!("clip_{}_{}.{}", timestamp, i, extension)
```
补充单元测试覆盖。

---

## P1-8：反馈环防护改为时间窗口

**文件**：`main.rs` 消息循环

**问题**：布尔值只能跳过一次通知，多格式写入理论上可触发多次。

**修复**：将 `clipboard_self_triggered: bool` 改为 `last_self_set_time: Option<Instant>`，500ms 冷却期内跳过。

---

## P1-9：max_history_hours = 0 定义为不清理

**文件**：`clipboard.rs` `clean_old_files()` + `config.rs` `validate()`

**问题**：`max_history_hours = 0` 时 cutoff = now，会立刻删除所有历史文件。

**修复**：
- `clean_old_files()` 中 `max_hours == 0` 时直接 return 0
- 不在 validate 中禁止 0（允许用户显式禁用清理）

---

## P2-11：配置监控线程优化

**文件**：`main.rs` `start_config_watcher()`

**问题**：
- WaitForSingleObject 超时后未取消 pending 的 overlapped I/O
- 退出时 PostThreadMessageW 需要等最多 1 秒

**修复**：
- 超时分支调用 `CancelIoEx(dir_handle, &overlapped)`
- 用专用退出 Event 替代 PostThreadMessageW，`WaitForMultipleObjects` 同时等目录变化和退出信号

---

## P2-12：剪贴板设置函数错误处理

**文件**：`input.rs` `set_multi_format_clipboard()` / `set_text_and_file_clipboard()`

**问题**：GlobalAlloc/GlobalLock 失败时静默跳过，函数仍返回 Ok(())。

**修复**：
- 关键格式（CF_UNICODETEXT）写入失败时返回 Err
- 次要格式失败时记录 warn 日志
- 在 main.rs 中根据返回值决定是否设 clipboard_self_triggered

---

## P2-13：is_png_file 只读文件头

**文件**：`clipboard.rs` `is_png_file()`

**问题**：`fs::read()` 读取整个文件到内存，大文件浪费。

**修复**：改为只读前 8 字节：
```rust
let mut buf = [0u8; 8];
matches!(file.read_exact(&mut buf), Ok(())) && buf.starts_with(b"\x89PNG")
```

---

## 不纳入 v1.0.7 的审查建议（记录备忘）

| 建议 | 原因 |
|------|------|
| `fatal_error` 中 title 字符串的 `\0` 风格统一 | 纯代码风格，不影响功能 |
| `find_latest_file` 多文件时排序 | remove_latest_file 保证只有一个，竞态极低 |
| `send_unicode_char` BMP 外字符 | 文件路径不含此类字符，极低概率 |
| `days_to_ymd` 重复代码去重 | build.rs 无法引用 src/，需要 include! 方案，收益低 |
| `copy_file` 对无文件名路径加 debug log | 当前返回 None 行为正确 |
| `ClipboardWatcher` 标注 `!Send` | 当前单线程使用，无实际风险 |
| `logger.rs` UTC+8 硬编码 | 已纳入 P2-14 |

---

## 实施步骤

### Step 1：clipboard.rs 修复
- P0-3：`sync_latest_from_disk()` + 启动调用
- P1-7：`unique_history_path()` 文件名格式
- P1-9：`clean_old_files()` max_hours=0 处理
- P2-13：`is_png_file()` 只读文件头
- `copy_file()` 返回值扩展（为 P0-2 准备）

### Step 2：config.rs 修复
- P1-6：`migrate_config()` max_history_days → hours 转换
- 更新相关测试

### Step 3：input.rs 修复
- P2-12：剪贴板设置函数错误处理

### Step 4：main.rs 修复（最大改动）
- P0-1：`is_autostart_enabled()` UTF-16 转换
- P0-2：非 PNG 文件 CF_HDROP 路径
- P0-4：`do_reload_config()` 同步 watcher
- P0-5 + P1-10：热键管理器 + 预览热键热更新
- P1-8：反馈环时间窗口

### Step 5：配置监控线程优化
- P2-11：overlapped I/O 取消 + 退出 Event

### Step 6：logger.rs 时区修复
- P2-14：`now_timestamp()` / `filename_timestamp()` 改用 `GetLocalTime` API
- 保留手动 `days_to_ymd` 用于非 Windows 平台

### Step 7：测试 + 文档
- 补充单元测试覆盖所有修复项
- 更新 README 版本记录
- 更新 todo.md

---

## 补充测试清单

1. `is_autostart_enabled()` 非 ASCII 路径测试（需 mock 或集成测试）
2. 非 PNG 文件复制后 CF_HDROP 指向 latest_file.xxx
3. 启动时磁盘已有 latest_file.pdf，热键应输出 .pdf 路径
4. max_history_days=7 迁移为 max_history_hours=168
5. 同秒冲突文件名格式 `clip_xxx_1.ext`
6. max_history_hours=0 不清理任何文件
7. 配置热重载后 watcher 的 max_copy_size_mb 生效
8. 预览热键热更新（修改后无需重启）
9. 切换到剪贴板模式后预览热键仍可用
