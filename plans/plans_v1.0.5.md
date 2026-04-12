# v1.0.5 实施计划

## 版本目标

三项改进：移除 tao 依赖、配置路径优化（双路径引导 + output_path 目录级）、配置热更新。

---

## 功能一：移除 tao 依赖（事项 13）

### 背景

v1.0.4 已将事件循环从 tao 切换为 Win32 原生 `GetMessageW`，tao 不再被使用。经 `cargo tree` 确认，tao 只被 clipimg 直接依赖，`tray-icon`、`global-hotkey`、`muda` 均不依赖 tao，可安全移除。

### 改动

- `Cargo.toml`：移除 `tao = "0.3"` 依赖
- `src/main.rs`：移除所有 `use tao::*` 引用（v1.0.4 已清理，需确认无残留）
- 移除后验证：`cargo xwin build --target x86_64-pc-windows-msvc --release` 编译通过

### 预期收益

- 减小 EXE 体积（tao 及其依赖链占用不小）
- 减少编译时间

---

## 功能二：配置路径优化（事项 2）

### 背景

首次启动只确认 `save_dir`，`output_path` 用硬编码默认值 `/workspace/.clip/latest.png`。用户挂载路径不是 `/workspace` 时，粘贴出的路径在容器内找不到文件。且 `output_path`（文件级）与 `save_dir`（目录级）概念不对等。

### 改动范围

#### 1. `config.rs` — output_path 改为目录级

- `output_path` 语义从 `/workspace/.clip/latest.png` 改为 `/workspace/.clip`（不含文件名）
- 新增 `resolved_output_path()` 方法，返回 `format!("{}/latest.png", output_path)`
- 所有使用 `config.output_path` 的地方改为调用 `config.resolved_output_path()`
  - `main.rs`：热键模式发送路径、剪贴板模式设置多格式剪贴板
  - `input.rs`：`set_multi_format_clipboard` 的 text_path 参数
- 旧配置兼容：加载时检测 `output_path` 以 `/latest.png` 结尾则自动截断，warn 日志提示，并将截断后的值回写配置文件
- `poll_interval_ms`：加载时如果存在则从配置中删除并回写配置文件（不再保留该字段）
- `Default` 默认值从 `/workspace/.clip/latest.png` 改为 `/workspace/.clip`

#### 2. `first_run.rs` — 双输入框对话框

当前使用 Win32 内存对话框（`DialogBoxIndirectParamW`），改为双输入框布局：

```
┌──────────────────────────────────────────────┐
│  clipImg 首次运行 — 路径配置                   │
├──────────────────────────────────────────────┤
│                                              │
│  以下两个路径指向同一个物理目录（WSL2 挂载）    │
│                                              │
│  Windows 侧（程序实际写入）：                  │
│  [E:\WorkingProjects\workspace\.clip       ] │
│              ↕ 挂载映射                       │
│  容器侧（粘贴到终端的路径）：                   │
│  [/workspace/.clip                         ] │
│                                              │
│              [确定]    [取消]                  │
└──────────────────────────────────────────────┘
```

- Windows 侧输入框：展示解析后的绝对路径（resolved_save_dir）
- 容器侧输入框：默认 `/workspace/.clip`
- 中间标注 `↕ 挂载映射` 强调对应关系
- 用户确认后保存到 config.json

#### 3. `config.example.json` — 更新示例值

- `output_path` 从 `/workspace/.clip/latest.png` 改为 `/workspace/.clip`

#### 4. `README.md` — 更新配置说明

- 配置表格中 `output_path` 说明改为目录级
- 移除表格中的 `latest.png` 说明

---

## 功能三：配置热更新（事项 5）

### 背景

修改 config.json 后需要重启 EXE 才能生效。使用轮询 mtime 方案会引入 CPU 开销，违背 v1.0.4 零 CPU 的目标。

### 方案：B（菜单手动）+ C（文件监控自动）

#### B. 托盘菜单"重新加载配置"

- 在托盘菜单中新增"重新加载配置"选项（放在"打开配置文件"下方）
- 点击后重新加载 config.json 并应用配置

#### C. Win32 文件监控自动重载

- 新增 `src/config_watcher.rs`（或合并到现有模块）
- 使用 `ReadDirectoryChangesW` 监听 config.json 所在目录的文件变化
- 和剪贴板监听同样的模式：独立线程 + `PostThreadMessageW` 通知主线程
- 通过 `RegisterWindowMessageW("clipImgConfigChanged")` 注册自定义消息
- 防抖：短时间内多次变化只触发一次重载（如编辑器保存时可能触发多次）

#### 运行时重载逻辑（`main.rs`）

收到配置重载通知后：

1. 重新加载 config.json
2. 比较新旧配置差异，按需应用：
   - `output_path`、`save_dir`、`max_history_hours`、`max_log_size_mb` → 直接更新内存值
   - `hotkey` 变化 → 反注册旧热键 + 注册新热键，热键模式/剪贴板模式切换
   - `poll_interval_ms` → 已废弃，加载时从配置文件中删除并回写
3. 日志记录重载结果
4. 更新托盘菜单状态行（显示新模式名称）

### 涉及文件

- `config.rs`：配置加载/保存/迁移逻辑（旧版兼容）
- `main.rs`：托盘菜单 + 文件监控线程（内联 `ConfigWatcher`） + 重载逻辑（`do_reload_config`） + 共享状态（`Rc<RefCell<...>>`）

---

## 实施步骤

### Step 1: 移除 tao 依赖
- Cargo.toml 移除 tao
- 确认代码无 tao 引用残留
- 编译验证

### Step 2: config.rs — output_path 改目录级
- `output_path` 语义改为目录级
- 新增 `resolved_output_path()` 方法
- 旧配置兼容（截断 + 回写）
- 更新所有使用方

### Step 3: first_run.rs — 双输入框对话框
- 改造首次启动对话框
- Windows 侧展示绝对路径，容器侧默认 `/workspace/.clip`
- 映射关系标注

### Step 4: 配置热更新
- config.rs 新增 reload 方法
- 新增文件监控线程
- 托盘菜单新增"重新加载配置"
- 主线程重载逻辑 + 热键重注册

### Step 5: 更新文档 + 编译验证
- README.md 配置说明更新
- config.example.json 更新
- `cargo test` + `cargo xwin build --release`

---

## 配置兼容性

| 配置项 | v1.0.4 | v1.0.5 | 兼容处理 |
|--------|--------|--------|----------|
| `output_path` | `/workspace/.clip/latest.png`（文件级） | `/workspace/.clip`（目录级） | 自动截断 `/latest.png` + 回写 |
| `poll_interval_ms` | 废弃，warn 提示 | 加载时自动删除并回写 | 彻底移除该字段 |
| `hotkey` | 重启生效 | 运行时热更新 | 新增能力 |
| 其他字段 | 不变 | 不变 | — |

---

## 风险与应对

| 风险 | 应对 |
|------|------|
| 移除 tao 后 tray-icon 等库是否正常工作 | 已通过 cargo tree 确认无间接依赖，编译验证即可 |
| first_run.rs 双输入框对话框复杂度增加 | Win32 内存对话框支持多控件，参考现有实现扩展 |
| 配置热更新期间热键重注册失败 | 保留旧热键，日志 error 提示用户，不中断运行 |
| 文件监控线程误触发（编辑器临时文件等） | 只关注 config.json 的变化，加防抖（100ms 内去重） |
