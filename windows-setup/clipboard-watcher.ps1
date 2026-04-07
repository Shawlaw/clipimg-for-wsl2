# clipboard-watcher.ps1 — Windows 剪贴板守护进程
# 由 start-daemon.ps1 启动，也可以直接运行
#
# 自动检测 Windows 剪贴板中的图片，保存到 workspace\.clip\ 目录

# 根据脚本自身位置推算 .clip 目录（不需要硬编码路径）
# 脚本位于 workspace\clipImg\windows-setup\，上溯两级即 workspace\
$workspaceDir = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$clipDir = Join-Path $workspaceDir ".clip"
$logFile = Join-Path $clipDir ".daemon.log"

if (-not (Test-Path $clipDir)) {
    New-Item -ItemType Directory -Force -Path $clipDir | Out-Null
}

function Write-Log($msg) {
    $line = "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] $msg"
    Add-Content -Path $logFile -Value $line
    Write-Host $line
}

# 写入 PID 供 stop-daemon.ps1 使用
$pidFile = Join-Path $clipDir ".daemon.pid"
$PID | Set-Content $pidFile -NoNewline

Write-Log "启动，监听目录: $clipDir  PID: $PID"

try {
    Add-Type -Assembly 'System.Windows.Forms'
} catch {
    Write-Log "❌ 无法加载 System.Windows.Forms: $_"
    exit 1
}


try {
    while ($true) {
        # 检查停止信号文件
        $stopFile = Join-Path $clipDir ".stop"
        if (Test-Path $stopFile) {
            Remove-Item $stopFile -Force
            Write-Log "收到停止信号，退出。"
            break
        }

        try {
            $img = [System.Windows.Forms.Clipboard]::GetImage()

            if ($img -ne $null) {
                # 先写临时文件，再与 latest.png 做文件级 MD5 比对
                # 不用内存变量 $lastHash，重启后也不会误判同一张图为新图
                $tmpPath = Join-Path $clipDir "_tmp_clip.png"
                $img.Save($tmpPath, [System.Drawing.Imaging.ImageFormat]::Png)
                $img.Dispose()

                $latestPath = Join-Path $clipDir "latest.png"
                $isNew = $true

                if (Test-Path $latestPath) {
                    # 先比文件大小（快），再比 MD5（准）
                    $newSize    = (Get-Item $tmpPath).Length
                    $latestSize = (Get-Item $latestPath).Length
                    if ($newSize -eq $latestSize) {
                        $newHash    = (Get-FileHash $tmpPath    -Algorithm MD5).Hash
                        $latestHash = (Get-FileHash $latestPath -Algorithm MD5).Hash
                        $isNew = ($newHash -ne $latestHash)
                    }
                }

                if ($isNew) {
                    $ts      = Get-Date -Format 'yyyyMMdd_HHmmss'
                    $outPath = Join-Path $clipDir "clip_$ts.png"
                    Rename-Item -Path $tmpPath -NewName (Split-Path $outPath -Leaf)
                    Copy-Item -Path $outPath -Destination $latestPath -Force
                    Write-Log "新图片: clip_$ts.png"
                } else {
                    Remove-Item $tmpPath -Force
                }
            }
            # 剪贴板无图片时不做任何事（不重置状态，避免短暂清空导致下次误判）
        } catch {
            Write-Log "⚠ 剪贴板访问异常（通常可忽略）: $_"
            Remove-Item (Join-Path $clipDir "_tmp_clip.png") -Force -ErrorAction SilentlyContinue
        }

        Start-Sleep -Milliseconds 800
    }
} finally {
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
    Write-Log "进程退出。"
}
