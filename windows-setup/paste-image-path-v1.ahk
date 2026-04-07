; paste-image-path-v1.ahk — AutoHotkey v1（兼容旧版）
;
; 如果你安装的是 AutoHotkey v1.x，使用本文件代替 paste-image-path.ahk

ContainerPath := "/workspace/.clip/latest.png"
WinLatestPng := A_ScriptDir . "\..\..\.clip\latest.png"

!Insert::
    if !FileExist(WinLatestPng) {
        ToolTip, % "⚠ 暂无图片，请先在 Windows 复制一张图"
        SetTimer, ClearTooltip, 3000
        return
    }
    ; SendInput {Raw} 原样输入文本，不解释特殊符号
    SendInput, {Raw}%ContainerPath%
    return

ClearTooltip:
    SetTimer, ClearTooltip, Off
    ToolTip
    return
