; paste-image-path.ahk — AutoHotkey v2
;
; 功能：按下 Alt+Insert，自动向当前活动窗口输入最新剪贴板图片的容器内路径。
;       适用于 Claude Code、bash、vim 等任何应用（系统层面拦截，无需终端配置）。
;
; 安装：
;   1. 安装 AutoHotkey v2：https://www.autohotkey.com/
;   2. 双击本文件启动，任务栏托盘会出现绿色 H 图标
;   3. 可选：把本文件快捷方式放入 Windows 启动文件夹实现开机自启
;      启动文件夹路径：Win+R → shell:startup → 回车
;
; 停止：右键任务栏托盘图标 → Exit

#Requires AutoHotkey v2.0

; 容器内的固定路径（与 /workspace 挂载点对应，通常不需要修改）
ContainerPath := "/workspace/.clip/latest.png"

; Windows 侧 latest.png 的路径，用于判断守护进程是否已保存图片
; 脚本位于 clipImg\windows-setup\，上两级即 workspace 根目录
WinLatestPng := A_ScriptDir "\..\..\.clip\latest.png"

!Insert:: {
    if !FileExist(WinLatestPng) {
        ToolTip("⚠ 暂无图片`n请先在 Windows 复制一张图`n并确认 clipboard-watcher 守护进程在运行")
        SetTimer(() => ToolTip(), -3000)
        return
    }
    ; SendText 逐字符发送，不解释特殊符号，兼容各种键盘布局
    SendText(ContainerPath)
}
