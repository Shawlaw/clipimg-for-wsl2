# v1.0.8 实施计划

> 解决用户反馈的核心痛点：连续粘贴多个图片/文件时路径互相覆盖；评估多文件粘贴与 UNC 路径支持。

## 背景

用户反馈不支持连续粘贴多个图片/文件。经分析，根因是 `latest_file` 机制——每次截图/复制都覆盖同一个固定文件名，导致历史路径全部失效：

```
截图1 → /workspace/.clip/latest_file.png ✅
截图2 → /workspace/.clip/latest_file.png ❌ 内容已变，截图1路径失效
```

---

## Feature 1：去掉 `latest_file`，支持连续粘贴（P0）

### 核心改动

**干掉 `latest_file` 机制：**
- 删除 `update_latest_from_history()`、`remove_latest_file()`、`migrate_old_latest()`、`sync_latest_from_disk()`
- 截图/复制后，直接将 `clip_<timestamp>.<ext>` 的唯一路径写入剪贴板
- 去重逻辑不变：内存存 `last_md5`，比对剪贴板内容，与文件名无关

**需要适配的功能：**

| 功能 | 现状（依赖 latest_file） | 改后 |
|------|--------------------------|------|
| 截图写入剪贴板 | 写 `latest_file.<ext>` 路径 | 写 `clip_<ts>.<ext>` 路径 |
| 去重比对 | 内存 `last_md5`，不变 | 不变 |
| 热键模式 | 模拟输入 `latest_container_path` | 模拟输入最新 `clip_*` 路径 |
| 预览热键 | 打开 `latest_file` | 打开最新的 `clip_*` 文件 |
| 历史清理 | 不删 `latest_file`，只删旧 `clip_*` | 只删超时 `clip_*`，逻辑更简单 |
| 启动恢复 | 扫描 `latest_file.*` | 无需恢复（`find_latest_clip()` 运行时按需查找） |

### 效果

每次截图/复制产生唯一路径（`clip_20260416_103000123.png`、`clip_20260416_103500456.png`），用户可以在同一会话中连续粘贴多张图，路径不会互相覆盖。

### 文件名策略（已有，需调整）

截图和 Explorer 文件复制已统一使用 `clip_<timestamp>.<ext>` 命名，本次升级时间戳精度从秒级提升到毫秒级：
- 格式变更：`clip_YYYYMMDD_HHmmSS.<ext>` → `clip_YYYYMMDD_HHmmSSmmm.<ext>`
- 示例：`clip_20260416_103000123.png`
- 改动点：`logger.rs` 中 `filename_timestamp()` 拼接毫秒部分（Windows 侧 `SYSTEMTIME.wMilliseconds` 已可用）
- 冲突后缀不变（`_1`、`_2`…），毫秒精度下冲突概率极低，现有的文件存在性检测兜底

### 涉及文件

- `clipboard.rs`：删除 `latest_file` 相关方法和 `latest_container_path` 字段，`poll()` / `copy_file()` 返回实际路径，新增 `find_latest_clip()` 方法
- `input.rs`：`set_multi_format_clipboard()` 路径参数变化
- `main.rs`：剪贴板模式接收返回路径、热键/预览改用 `find_latest_clip()`、删除 `find_latest_file()` 和所有 `latest_file` 引用、删除 `sync_latest_from_disk()` / `migrate_old_latest()` 调用
- `config.rs`：删除 `resolved_output_path_for()`、`resolved_output_path()`、`latest_file_path()`、`latest_png_path()`，新增 `container_path_for()`
- `first_run.rs`：无需改动

### 实施步骤

#### Step 1：clipboard.rs 改造
- 删除 `update_latest_from_history()`、`remove_latest_file()`、`migrate_old_latest()`、`sync_latest_from_disk()`
- 删除 `latest_container_path` 字段（所有需要路径的地方改为从方法返回值或 `find_latest_clip()` 获取）
- 新增 `find_latest_clip()` 方法：扫描 save_dir，按 **mtime** 降序排列，返回最新的 `clip_*` 文件路径
- `poll()`：返回类型改为 `Option<String>`，返回实际保存的 `clip_<ts>.<ext>` 完整路径（而非 `latest_file` 路径）；无新内容时返回 `None`
- `copy_file()`：返回 `Option<String>`，返回实际保存的文件完整路径
- `clean_old_files()`：无需改动（原来只匹配 `clip_*` 前缀，不涉及 `latest_file` 的显式排除）

#### Step 1.5：升级兼容——迁移旧版 `latest_file.*` / `latest.png`
- 启动时扫描 save_dir，将 `latest_file`、`latest_file.*`、`latest.png` 按 mtime 重命名为 `clip_<timestamp>.<ext>` 格式，保留文件
- 每次启动都执行，无残留时秒过无开销；有残留时重命名后下次不再命中
- 重命名冲突处理：若目标文件名已存在，在时间戳后追加 `_1`、`_2`… 后缀

#### Step 2：config.rs 适配
- 删除 `resolved_output_path_for()`、`resolved_output_path()`、`latest_file_path()`、`latest_png_path()`
- 新增 `container_path_for(filename)` 方法：给定 save_dir 下的文件名，返回 `output_path/filename`（即 `format!("{}/{}", output_path, filename)`）

#### Step 3：main.rs 适配
- 剪贴板模式：接收 `poll()` / `copy_file()` 返回的实际路径，调用 `config.container_path_for()` 得到容器路径，传入 `set_multi_format_clipboard()`
- 热键模式：调用 `find_latest_clip()` 获取最新 `clip_*` 的容器路径，模拟输入
- 预览热键：调用 `find_latest_clip()` 获取最新 `clip_*` 的磁盘路径，打开文件
- 启动恢复：`sync_latest_from_disk()` 不再需要，删除调用；`migrate_old_latest()` 同步删除
- 删除 `find_latest_file()` 辅助函数
- 删除所有 `latest_container_path`、`latest_file` 相关引用

#### Step 4：测试 + 文档
- 补充单元测试：`find_latest_clip()`、连续截图路径唯一性、启动恢复
- 更新 CHANGELOG.md
- 更新 README.md 版本号

---

## Feature 2：支持多文件 CF_HDROP（P1）

### 核心改动

- `read_clipboard_files()` 处理所有文件（目前只处理第一个）
- 所有文件复制到 save_dir，上限由配置项 `max_copy_files` 控制，默认 10；超出部分跳过并 warn 日志
- 剪贴板 CF_UNICODETEXT 写入多行路径（每行一个），末尾追加一个空行，方便用户继续输入
- 示例：复制 2 个文件后，剪贴板文本为：
  ```
  /home/user/.clip/clip_20260416_103000.png
  /home/user/.clip/clip_20260416_103500.pdf

  ```

### 设计说明

- Agent 收到多个文件路径后会自行查看所有文件，无需特殊处理
- 末尾空行确保用户粘贴后可以直接继续输入，无需手动 Shift+Enter 换行
- 单文件粘贴行为不受影响（仍然只有一行路径 + 一个空行）
- `max_copy_files` 配置项防止用户误复制大量文件（如整个目录）导致卡顿，默认 10

---

## Feature 3：支持 `\\wsl$` / `\\wsl.localhost` UNC 路径

### 背景

用户可能将 save_dir 配置为 WSL 实例内的路径，通过 UNC 路径从 Windows 侧访问：
```
\\wsl$\debian\home\user\.clip          → output_path: /home/user/.clip
\\wsl.localhost\debian\home\user\.clip  → output_path: /home/user/.clip
```

文件实际存储在 WSL 实例内部，Windows 通过 9P 协议访问，两端指向同一位置。

### 核心改动

1. **`is_windows_absolute()` 扩展**：识别 `\\` 开头的 UNC 路径（目前只认 `<盘符>:\`），使其在 `resolved_save_dir()` 中被当作 Windows 路径直接使用，不触发 Linux→Windows 路径转换
2. **首次运行对话框**：适配说明 UNC 路径映射关系（`output_path` 不自动推导，仍由用户手动填写，因为无法预知目录在容器/WSL 内的挂载路径）
3. **CF_HDROP 中 UNC 路径文件**：从 `\\wsl$` 目录复制的文件路径能正确处理

### UNC 路径可用性保障

UNC 路径依赖 WSL 实例先启动，clipImg 开机自启时可能早于 WSL 就绪：

**启动时容忍失败：**
- `ClipboardWatcher::new()` 中 `create_dir_all(&save_dir)` 失败 → warn 日志，不 panic
- 本地路径几乎不会出现此问题，但 UNC 路径在 WSL 未启动时会触发

**运行时懒重试：**
- `poll()` / `copy_file()` 保存文件前，检查 save_dir 是否可访问
- 不可访问 → warn 日志 + 跳过本次操作
- 下次剪贴板事件触发时自然重试，WSL 启动后自动恢复

**用户通知：**
- 每次保存失败都弹出对话框（⚠️ warning 图标）提示用户（不能用气泡通知，高版本 Windows 已废弃）
- 对话框两个按钮：
  - 「确定」— 关闭对话框，下次失败继续提醒
  - 「不再提醒」— 置位 `suppress_unavailable_notify: bool`，后续失败不再弹窗
- 恢复可用时弹出标准 `MessageBox`（ℹ️ info 图标，仅「确定」按钮）通知用户，同时重置 `suppress_unavailable_notify`- 措辞：
  - 不可用：「存储目录暂不可用，请在WSL2启动后再尝试重新截图」（根据本次实际操作类型显示"截图"或"复制文件"）
  - 恢复：「存储目录已恢复」
- 实现方式：不可用对话框需使用 `TaskDialog` 或自定义对话框（参考 `first_run.rs` 现有模式）以支持自定义按钮文字；恢复通知直接用标准 `MessageBox`

### 涉及文件

- `config.rs`：`is_windows_absolute()` 增加 UNC 路径识别
- `clipboard.rs`：`new()` 容忍目录创建失败，`poll()` / `copy_file()` 增加目录可用性检查
- `first_run.rs`：路径配置提示文案适配

### 注意事项

- UNC 路径性能可能略低于本地盘（跨 9P 协议），但图片文件影响不大
- 不依赖 `wslpath` 工具做路径转换，避免用户环境差异
- `std::fs` 在 Windows 上原生支持 UNC 路径读写
