ui-workspace-title = Windows
ui-workspace-left-right = Side by side
ui-workspace-top-bottom = Stacked
ui-workspace-grid = Grid
ui-workspace-add = Add window
ui-workspace-switch = Switch to
ui-workspace-background = Move to background
ui-workspace-show = Add to split
ui-workspace-minimize = Minimize
ui-workspace-maximize = Full screen
ui-workspace-restore = Restore
ui-workspace-split = Split
ui-workspace-taskbar = Running windows
ui-workspace-scroll-left = Scroll left
ui-workspace-scroll-right = Scroll right
ui-workspace-close = Close window
ui-workspace-force-kill = Force kill
ui-workspace-dismiss = Back
ui-workspace-scroll-up = Scroll up
ui-workspace-scroll-down = Scroll down
ui-workspace-no-windows = No open windows.
ui-workspace-window-minimum = Min. { $width }×{ $height }
ui-workspace-capacity = Visible windows: up to { $capacity }
ui-workspace-terminal-size = Current terminal: { $width }×{ $height }
ui-workspace-resize-required = Enlarge the terminal to at least { $width }×{ $height }.
ui-workspace-focused = Focused
ui-workspace-visible = Visible
ui-workspace-backgrounded = In background
ui-workspace-unresponsive = Unresponsive
ui-workspace-starting = Starting…
ui-workspace-closing = Closing…

workspace-launch-failed = Could not open the window. { $reason }
workspace-stop-failed = Could not clean up { $window }. { $reason }
workspace-close-task-title = Stop running operations and close this window?
workspace-close-task-message = Closing stops the operations in this window and waits for cleanup.
workspace-close-task-confirm = Stop and close
workspace-close-wait-title = Closing { $window }
workspace-close-wait-message = Cleanup is still running. Force killing can lose unsaved work.
workspace-close-size-message = This window needs a larger terminal to show its save or running-task confirmation. Enlarge the terminal or keep waiting. Force killing can lose unsaved work.
workspace-close-wait-continue = Keep waiting
workspace-close-wait-force = Force kill
workspace-services-unavailable = Shared services are unresponsive. Try again shortly.
workspace-services-conflict = Another window made a change. Refresh and try again.
workspace-services-forbidden = You do not have permission to perform this operation.

watchdog-cli-help = Usage: tundra-cli watchdog <list|kill>
    { "  " }tundra-cli watchdog list
    { "  " }tundra-cli watchdog kill --run <run> --session <session>
    { "  " }tundra-cli watchdog kill --run <run> --shell
    Copy the run ID and application session number from list. Commands contact the current OS user's watchdog directly.
    Force killing can lose unsaved work. The --shell option shuts down the entire run, including its applications and services.
    JSON status kill_requested means the watchdog accepted the request. Status killed confirms exit.
watchdog-cli-invalid-arguments = Invalid watchdog arguments. Supply a run ID and either --session or --shell for kill.
watchdog-cli-unavailable = Could not contact the watchdog. { $reason }
watchdog-cli-kill-unconfirmed = Could not confirm the watchdog's reply to the force-kill request. { $reason }
watchdog-cli-run-missing = Managed run { $run } is unavailable.
watchdog-cli-target-missing = The selected managed process is no longer available. Run tundra-cli watchdog list again.
watchdog-cli-protocol = The watchdog returned an incompatible reply.
watchdog-cli-request-refused = The watchdog could not complete the request.
watchdog-cli-output-error = Could not write the watchdog result. { $reason }
