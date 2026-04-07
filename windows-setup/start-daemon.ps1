# start-daemon.ps1 — 以后台方式启动剪贴板守护进程
# 在 Windows PowerShell 或 Windows Terminal 里运行：
#   .\start-daemon.ps1              # 正常启动（隐藏窗口）
#   .\start-daemon.ps1 -Test        # 可见窗口运行，用于排查问题
#   .\start-daemon.ps1 -AutoStart   # 同时注册为登录启动项

param(
    [switch]$AutoStart,   # 注册为 Windows 登录时自动启动
    [switch]$Test         # 以可见窗口运行，方便排查错误
)

$scriptDir    = $PSScriptRoot
$watcherPath  = Join-Path $scriptDir "clipboard-watcher.ps1"
$workspaceDir = Split-Path -Parent (Split-Path -Parent $scriptDir)
$clipDir      = Join-Path $workspaceDir ".clip"
$pidFile      = Join-Path $clipDir ".daemon.pid"
$logFile      = Join-Path $clipDir ".daemon.log"

if (-not (Test-Path $clipDir)) {
    New-Item -ItemType Directory -Force -Path $clipDir | Out-Null
}

# 检查是否已在运行
if (Test-Path $pidFile) {
    $existingPid = (Get-Content $pidFile -Raw).Trim()
    $proc = Get-Process -Id $existingPid -ErrorAction SilentlyContinue
    if ($proc) {
        Write-Host "✅ clipboard-watcher 已在运行 (PID: $existingPid)"
        Write-Host "   日志: $logFile"
        exit 0
    }
    # PID 文件残留但进程已死，清除后重启
    Remove-Item $pidFile -Force
}

if ($Test) {
    # 诊断模式：直接在当前窗口运行，可以看到完整报错
    Write-Host "🔍 诊断模式：直接运行 clipboard-watcher.ps1（Ctrl+C 退出）"
    & $watcherPath
    exit 0
}

# 修复：ArgumentList 数组中不要对路径额外加引号，Start-Process 会自动处理
# 错误写法："`"$watcherPath`"" 会让 powershell.exe 收到带字面引号的路径，导致找不到文件
$proc = Start-Process powershell.exe `
    -ArgumentList @("-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-File", $watcherPath) `
    -PassThru

# 等待最多 3 秒，确认 PID 文件被写入（由 clipboard-watcher.ps1 负责写入）
$waited = 0
while (-not (Test-Path $pidFile) -and $waited -lt 30) {
    Start-Sleep -Milliseconds 100
    $waited++
}

if (-not (Test-Path $pidFile)) {
    Write-Host "❌ 守护进程启动失败（3 秒内未写入 PID 文件）"
    Write-Host ""
    Write-Host "排查方法："
    Write-Host "  1. 运行诊断模式查看详细错误：  .\start-daemon.ps1 -Test"
    Write-Host "  2. 查看日志文件：               Get-Content '$logFile'"
    exit 1
}

Write-Host "✅ clipboard-watcher 已启动 (PID: $(Get-Content $pidFile -Raw))"
Write-Host "   监听目录: $clipDir"
Write-Host "   日志文件: $logFile"
Write-Host "   停止命令: .\stop-daemon.ps1"

# 注册登录启动项（写入注册表 HKCU Run，仅当前用户，无需管理员权限）
if ($AutoStart) {
    $regKey  = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    $regName = "ClipboardWatcher_clipImg"
    $cmd     = "powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -File `"$watcherPath`""
    Set-ItemProperty -Path $regKey -Name $regName -Value $cmd
    Write-Host "✅ 已注册登录启动项: $regName"
    Write-Host "   如需取消自启: .\stop-daemon.ps1 -RemoveAutoStart"
}
