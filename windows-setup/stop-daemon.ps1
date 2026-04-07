# stop-daemon.ps1 — 停止剪贴板守护进程
# 用法：
#   .\stop-daemon.ps1                  # 停止守护进程
#   .\stop-daemon.ps1 -RemoveAutoStart # 同时移除登录启动项

param(
    [switch]$RemoveAutoStart
)

$workspaceDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$clipDir  = Join-Path $workspaceDir ".clip"
$pidFile  = Join-Path $clipDir ".daemon.pid"

if (Test-Path $pidFile) {
    $existingPid = (Get-Content $pidFile -Raw).Trim()
    $proc = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
    if ($proc) {
        Stop-Process -Id $existingPid -Force
        Write-Host "✅ clipboard-watcher (PID: $existingPid) 已停止"
    } else {
        Write-Host "ℹ️  进程 $existingPid 不存在（已停止或被其他方式关闭）"
    }
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
} else {
    Write-Host "ℹ️  未找到 PID 文件，守护进程可能未在运行"
}

if ($RemoveAutoStart) {
    $regKey  = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    $regName = "ClipboardWatcher_clipImg"
    Remove-ItemProperty -Path $regKey -Name $regName -ErrorAction SilentlyContinue
    Write-Host "✅ 已移除登录启动项: $regName"
}
