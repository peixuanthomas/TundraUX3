ui-workspace-title = 窗口管理
ui-workspace-left-right = 左右
ui-workspace-top-bottom = 上下
ui-workspace-grid = 网格
ui-workspace-add = 新增窗口
ui-workspace-switch = 切换
ui-workspace-background = 转到后台
ui-workspace-show = 加入分屏
ui-workspace-minimize = 最小化
ui-workspace-maximize = 全屏
ui-workspace-restore = 还原
ui-workspace-split = 分屏
ui-workspace-taskbar = 运行窗口
ui-workspace-scroll-left = 向左滚动
ui-workspace-scroll-right = 向右滚动
ui-workspace-close = 关闭窗口
ui-workspace-force-kill = 强制终止
ui-workspace-dismiss = 返回
ui-workspace-scroll-up = 向上滚动
ui-workspace-scroll-down = 向下滚动
ui-workspace-no-windows = 没有打开的窗口。
ui-workspace-window-minimum = 最小 { $width }×{ $height }
ui-workspace-capacity = 可见窗口上限：{ $capacity }
ui-workspace-terminal-size = 当前终端：{ $width }×{ $height }
ui-workspace-resize-required = 请将终端扩大到至少 { $width }×{ $height }。
ui-workspace-focused = 当前窗口
ui-workspace-visible = 前台
ui-workspace-backgrounded = 后台
ui-workspace-unresponsive = 无响应
ui-workspace-starting = 正在启动…
ui-workspace-closing = 正在关闭…

workspace-launch-failed = 无法打开窗口。{ $reason }
workspace-stop-failed = 无法清理窗口 { $window }。{ $reason }
workspace-close-task-title = 停止正在运行的操作并关闭窗口？
workspace-close-task-message = 关闭时会停止此窗口中的操作，并等待清理完成。
workspace-close-task-confirm = 停止并关闭
workspace-close-wait-title = 正在关闭 { $window }
workspace-close-wait-message = 窗口尚未完成清理。强制终止可能导致未保存的内容丢失。
workspace-close-size-message = 此窗口需要更大的终端才能显示保存或运行任务确认。请扩大终端或继续等待。强制终止可能丢失未保存的内容。
workspace-close-wait-continue = 继续等待
workspace-close-wait-force = 强制终止
workspace-services-unavailable = 共享服务无响应。请稍后重试。
workspace-services-conflict = 其他窗口已更新数据。请刷新后重试。
workspace-services-forbidden = 权限不足，无法执行此操作。

watchdog-cli-help = 用法：tundra-cli watchdog <list|kill>
    tundra-cli watchdog list
    tundra-cli watchdog kill --run <run> --session <session>
    tundra-cli watchdog kill --run <run> --shell
    从 list 输出中复制运行 ID 和应用会话编号。命令直接连接当前系统用户的 watchdog。
    强制终止可能导致未保存的内容丢失。--shell 会结束整次运行，包括其中的应用和服务。
    JSON 状态 kill_requested 表示 watchdog 已接受请求。killed 表示已确认退出。
watchdog-cli-invalid-arguments = watchdog 参数无效。kill 需要运行 ID，并且只能指定 --session 或 --shell 之一。
watchdog-cli-unavailable = 无法连接 watchdog。{ $reason }
watchdog-cli-kill-unconfirmed = 无法确认 watchdog 对强制终止请求的回复。{ $reason }
watchdog-cli-run-missing = 受管运行 { $run } 不可用。
watchdog-cli-target-missing = 指定的受管进程已不可用。请重新运行 tundra-cli watchdog list。
watchdog-cli-protocol = watchdog 返回了不兼容的回复。
watchdog-cli-request-refused = watchdog 无法完成请求。
watchdog-cli-output-error = 无法输出 watchdog 结果。{ $reason }
