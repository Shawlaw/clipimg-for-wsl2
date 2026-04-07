# clip-paste.bash — 剪贴板图片路径辅助工具
# source 到容器的 ~/.bashrc：
#   echo 'source /workspace/clipImg/shell-integration/clip-paste.bash' >> ~/.bashrc
#
# 注意：Alt+Insert 的路径插入现在由 Windows Terminal 的 sendInput 直接完成，
#       不再依赖 bash readline 绑定，在 Claude Code 等 TUI 应用里同样有效。

# lastclip：打印最新图片路径，方便手动确认或在脚本里引用
lastclip() {
    local f="/workspace/.clip/latest.png"
    if [ -f "$f" ]; then
        echo "$f"
    else
        echo "⚠  暂无图片。请先在 Windows 里复制一张图，并确认 clipboard-watcher 守护进程在运行。" >&2
        return 1
    fi
}
