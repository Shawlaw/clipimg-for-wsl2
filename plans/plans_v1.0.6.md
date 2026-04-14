# v1.0.6 实施计划

## 版本目标

四项功能：CF_HDROP 文件复制、保存时清理过期文件、启动气泡通知、预览快捷键。

---

## 功能一：CF_HDROP 文件复制

### 背景

当前只处理剪贴板中的图片数据（DIB/Bitmap），不支持从资源管理器 Ctrl+C 复制文件。用户有"复制任意文件 → 粘贴出容器内路径"的需求（如让 AI 读取文档）。

### 文件命名规则（统一变更）

将 `latest.png` 改为 `latest_file.xxx`，与历史文件 `clip_YYYYMMDD_HHmmss.xxx` 统一：

| 来源 | latest 命名 | 说明 |
|------|-------------|------|
| 截图（DIB → PNG） | `latest_file.png` | 现有截图流程 |
| CF_HDROP + PNG 文件 | `latest_file.png` | 多格式剪贴板（含 CF_DIB） |
| CF_HDROP + 其他文件（如 PDF） | `latest_file.xxx` | 仅文本路径 |
| CF_HDROP + 无后缀文件 | `latest_file` | 仅文本路径 |

- 目录内始终只保留一个 `latest_file.*`，保存新的前删除旧的
- 历史文件也从 `clip_YYYYMMDD_HHmmss.png` 改为保留原始后缀 `clip_YYYYMMDD_HHmmss.xxx`
- 清理逻辑：扫描所有 `clip_*.*` 文件（不再限定 `.png`）

### 文件类型判断

读文件头（magic bytes）判断是否为 PNG，只有 PNG 能走多格式剪贴板：

```rust
fn is_png_file(data: &[u8]) -> bool {
    data.starts_with(b"\x89PNG")
}
```

### 两条处理路径

| 来源 | 判断结果 | 处理流程 |
|------|----------|----------|
| DIB/Bitmap（截图） | 始终为 PNG | RGBA → PNG → `latest_file.png` → 多格式剪贴板 |
| CF_HDROP + PNG 文件头 | PNG | 复制到 save_dir → `latest_file.png` → 多格式剪贴板（含 CF_DIB） |
| CF_HDROP + 非 PNG 文件 | 其他 | 复制到 save_dir → `latest_file.xxx` → 仅 CF_UNICODETEXT |

### 内存状态

- `ClipboardWatcher` 新增 `latest_container_path: String` 字段
- 记录最近一次操作的容器侧完整路径（如 `/workspace/.clip/latest_file.png` 或 `/workspace/.clip/latest_file.pdf`）
- 热键模式发送 `latest_container_path`，剪贴板模式用它设 CF_UNICODETEXT
- `config.resolved_output_path()` 不再拼死 `latest.png`，改为运行时从 `latest_container_path` 取

### 配置

```json
{
  "max_copy_size_mb": 10
}
```

- `max_copy_size_mb`：CF_HDROP 文件最大允许大小，超过则跳过（默认 10MB）

### 改动范围

- `clipboard.rs`：CF_HDROP 读取 + 文件复制 + `is_png_file()` + `latest_container_path` + 命名规则统一
- `config.rs`：新增 `max_copy_size_mb`；移除 `resolved_output_path()` 中的硬编码 `latest.png`
- `input.rs`：剪贴板模式根据 `latest_container_path` 设置 CF_UNICODETEXT
- `main.rs`：剪贴板变化通知后先检查 CF_HDROP，再走 DIB 流程

### 检测流程

主线程收到剪贴板变化通知后：

```
1. 尝试读取 CF_HDROP（Win32 API）
   ├─ 有文件 → 检查文件大小
   │   ├─ 超过 max_copy_size_mb → 跳过，日志 warn
   │   └─ 大小 OK → 复制到 save_dir（保留原始后缀）
   │       ├─ PNG 文件头 → 更新 latest_file.png，设多格式剪贴板
   │       └─ 非 PNG → 更新 latest_file.xxx，仅设 CF_UNICODETEXT
   └─ 无文件 → 走现有 arboard DIB 流程（保存为 latest_file.png）
```

---

## 功能二：更新 latest 时清理过期文件

### 背景

当前只在启动时清理过期文件，运行期间旧文件会一直累积。

### 改动

- `clipboard.rs`：保存新文件后调用 `clean_old_files()`
- `clean_old_files()` 扫描规则从 `clip_*.png` 改为 `clip_*.*`（匹配所有后缀）
- 频率低（只在有新文件时触发），不影响性能

---

## 功能三：启动气泡通知

### 背景

程序启动后无任何反馈，用户不确定是否成功运行。

### 改动

- `main.rs`：托盘图标创建后，调用 `Shell_NotifyIconW` 显示气泡通知
- 内容格式：`clipImg v{version} 已启动 [{模式}]`，如 `clipImg v1.0.6 已启动 [剪贴板模式]` 或 `clipImg v1.0.6 已启动 [热键模式 Alt+Insert]`
- 3 秒后自动消失（系统默认行为）

---

## 功能四：预览快捷键

### 背景

用户想快速确认最新截图/文件是否正确，需要打开查看。

### 设计

- 新增配置项 `preview_hotkey`（默认 `"Ctrl+Alt+P"`）
- 在热键模式和剪贴板模式下均可使用
- 触发时用 `ShellExecuteW` 打开 `latest_file.*`（用系统默认程序）
- 与现有 `hotkey`（路径输入热键）独立，注册第二个全局热键

### 配置

```json
{
  "preview_hotkey": "Ctrl+Alt+P"
}
```

- 空字符串 `""` 表示不启用预览功能
- `serde default` 默认值 `"Ctrl+Alt+P"`

### 改动范围

- `config.rs`：新增 `preview_hotkey` 字段
- `main.rs`：启动时注册第二个全局热键，消息循环中处理热键事件，调用 `ShellExecuteW` 打开文件
- 托盘菜单：状态行显示预览快捷键信息
- 配置热更新：`preview_hotkey` 变化时重新注册

---

## 实施步骤

### Step 1: config.rs — 新增配置字段 + 命名规则变更
- `max_copy_size_mb`（默认 10）
- `preview_hotkey`（默认 `"Ctrl+Alt+P"`）
- `resolved_output_path()` 改为运行时动态获取
- 旧配置兼容：`output_path` 中含 `latest.png` 的不再特殊处理（已在 v1.0.5 迁移过）

### Step 2: clipboard.rs — CF_HDROP + 文件复制 + 命名统一
- 新增 `is_png_file()` 文件头检测
- 新增 `latest_container_path` 字段
- 新增 CF_HDROP 文件复制逻辑
- `latest.png` → `latest_file.xxx` 命名统一
- `clip_*.png` → `clip_*.*` 清理规则统一
- 保存新文件后调用 `clean_old_files()`

### Step 3: main.rs — 启动气泡 + 预览快捷键 + CF_HDROP 集成
- 托盘图标创建后显示气泡通知
- 注册预览热键
- 消息循环中处理预览热键事件
- 剪贴板变化时先检查 CF_HDROP

### Step 4: 集成测试 + 文档更新
- cargo test + cargo xwin build
- README.md 更新配置说明和版本记录
- config.example.json 更新

---

## 配置兼容性

| 配置项 | v1.0.5 | v1.0.6 | 兼容处理 |
|--------|--------|--------|----------|
| `max_copy_size_mb` | 不存在 | 新增，默认 10 | serde default |
| `preview_hotkey` | 不存在 | 新增，默认 `"Ctrl+Alt+P"` | serde default |

---

## 风险与应对

| 风险 | 应对 |
|------|------|
| CF_HDROP 读取需要直接调用 Win32 剪贴板 API | 参考 input.rs 中已有的 OpenClipboard/SetClipboardData 模式 |
| 大文件复制阻塞剪贴板操作 | 先检查文件大小（`std::fs::metadata`），超限直接跳过 |
| 预览热键与输入热键冲突 | 两个热键独立注册，global-hotkey 支持多个热键 |
| 预览时 latest_file 不存在 | 检查文件存在性，不存在则跳过 |
| 旧版 `latest.png` 残留 | 启动时检测：`latest_file.png` 已存在则删除 `latest.png`（新版本已接管）；`latest_file.png` 不存在则将 `latest.png` 重命名为 `latest_file.png`，保留最新文件 |
