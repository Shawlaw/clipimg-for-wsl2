# 剪贴板图片粘贴工具 — 使用说明

在 Windows 复制图片后，Docker 容器（Claude Code 等 CLI）内按 **Alt+Insert**，自动在光标处插入图片路径。

---

## 工作原理

```
Windows 剪贴板（复制截图/图片）
    ↓ clipboard-watcher.ps1（Windows 后台进程，每 800ms 轮询一次）
    ↓ 写入 workspace\.clip\latest.png
    ↓ Windows 工作目录 ↔ /workspace（已挂载）
/workspace/.clip/latest.png（容器内可见）

用户按 Alt+Insert
    ↓ AutoHotkey（系统层面拦截）读取路径
    ↓ SendText 逐字符打入当前活动窗口
Claude Code / bash / 任意 TUI 应用
```

> **为什么用 AutoHotkey 而不用 bash readline 绑定或 Windows Terminal sendInput**：
> - `bind -x` readline 绑定：只在 bash 提示符下生效，Claude Code 等 TUI 应用有自己的输入循环，escape 序列会被当成乱码
> - Windows Terminal `sendInput`：对 Alt+Insert 这类修饰键组合在部分 WT 版本不稳定，会穿透发出原生 VT 序列
> - AutoHotkey：系统层面拦截，`SendText` 直接模拟键盘输入，任何窗口都能正确接收

---

## 一次性安装步骤

### 第一步：安装并启动 AutoHotkey 脚本

1. 安装 AutoHotkey：[https://www.autohotkey.com/](https://www.autohotkey.com/)（下载 v2，安装完成即可）
2. 双击运行 `clipImg\windows-setup\paste-image-path.ahk`
   - 任务栏托盘出现绿色 H 图标表示运行中
   - 如果你装的是 AutoHotkey **v1.x**，改用 `paste-image-path-v1.ahk`
3. 可选——开机自启：`Win+R` 输入 `shell:startup` 回车，把 `.ahk` 文件的**快捷方式**拖进去

---

### 第二步：启动 Windows 剪贴板守护进程

在 **Windows PowerShell**（不是 WSL2，不是 Docker 内）里执行：

```powershell
# 进入本目录（替换为你的实际路径）
cd C:\path\to\workspace\clipImg\windows-setup

# 首次运行，允许执行 PS 脚本（如果提示需要）
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned

# 启动守护进程（最小化窗口后台运行）
.\start-daemon.ps1

# 可选：注册为 Windows 登录自启（写入注册表 HKCU，无需管理员权限）
.\start-daemon.ps1 -AutoStart
```

> **说明**：守护进程只读取剪贴板，不修改剪贴板内容，不影响正常复制粘贴功能。
> 停止命令：`.\stop-daemon.ps1`（同时移除自启：`.\stop-daemon.ps1 -RemoveAutoStart`）

---

### 第三步：容器内（可选）

如果想在 bash 脚本里方便地引用图片路径，可以加入 `lastclip` 辅助函数：

```bash
echo 'source /workspace/clipImg/shell-integration/clip-paste.bash' >> ~/.bashrc
source ~/.bashrc
# 之后可用：lastclip  → 打印 /workspace/.clip/latest.png
```

---

## 日常使用

| 操作 | 说明 |
|------|------|
| 在 Windows 里截图或 Ctrl+C 复制图片 | 守护进程自动保存，约 1 秒内完成 |
| 在容器终端按 **Alt+Insert** | 当前光标处自动键入图片路径 |

### 容器内管理命令（clipimg）

```bash
# 添加到 PATH 后可直接使用（否则用完整路径）
sudo ln -s /workspace/clipImg/scripts/clipimg /usr/local/bin/clipimg

clipimg           # 输出最新图片路径：/workspace/.clip/latest.png
clipimg list      # 列出所有历史图片（含时间）
clipimg clean     # 清除 7 天前的旧图片
clipimg help      # 显示用法
```

---

## 文件结构

```
workspace/
├── .clip/                          # 剪贴板图片存储目录（自动创建）
│   ├── latest.png                  # 最新图片（守护进程覆盖更新）
│   └── clip_20260406_214500.png    # 历史记录
└── clipImg/
    ├── README.md                   # 本文件
    ├── windows-setup/
    │   ├── clipboard-watcher.ps1   # Windows 守护进程主体
    │   ├── start-daemon.ps1        # 启动脚本（含 -Test 诊断模式）
    │   ├── stop-daemon.ps1         # 停止脚本
    │   ├── paste-image-path.ahk    # AutoHotkey v2：Alt+Insert 路径输入
    │   └── paste-image-path-v1.ahk # AutoHotkey v1 兼容版本
    ├── scripts/
    │   └── clipimg                 # 容器内 CLI：list/clean 等管理命令
    └── shell-integration/
        └── clip-paste.bash         # 可选：lastclip() 辅助函数
```

---

## 故障排查

**按 Alt+Insert 没有输入路径**
- 确认 AutoHotkey 托盘图标存在（绿色 H）
- 确认 `.clip\latest.png` 存在：在 Windows 里先复制一张图，等守护进程日志出现"新图片"后再试

**图片路径插入但容器内文件找不到**
- 确认容器启动时已挂载 `-v <workspace>:/workspace`
- 容器内检查：`ls -la /workspace/.clip/`

**守护进程日志反复出现同一张图片**
- 升级到最新版 `clipboard-watcher.ps1`（已改用 MD5 哈希，修复此问题）
- 重启守护进程：`.\stop-daemon.ps1` → `.\start-daemon.ps1`

**守护进程启动后立刻退出**
- 用诊断模式查看报错：`.\start-daemon.ps1 -Test`
- 查看日志：`Get-Content ..\..\.clip\.daemon.log`
