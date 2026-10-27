# terminal-runtime

`terminal-runtime` 管理宿主终端和子终端。PTY 是供子进程使用的终端连接，让命令行程序可以读取按键、移动光标并输出完整终端画面。

`PtyConfig`、`PtyProcess`、`PtyExitStatus` 供命令行、Vim 和脚本共用；配置默认不添加参数或私有环境变量。内置 CLI 的启动参数、主题和特殊退出码由 Shell 的命令行宿主管理。

`PtyProcess::send_key` 和 `send_paste` 在发送时读取子终端当前模式，分别处理按键与粘贴。`input::mouse_to_terminal` 将宿主坐标换算为从零开始的终端内部坐标，保留按钮、拖动和修饰键，排除内容区之外的事件；发送时再按子程序选用的鼠标协议编码。

| 模块 | 职责 |
| --- | --- |
| `session` | `TerminalGuard`：进入和恢复 raw mode、备用屏幕、鼠标、焦点、粘贴和增强按键报告 |
| `pty` | 子进程启动、尺寸调整、输出读取和进程清理 |
| `input` | 按子终端模式编码按键与粘贴 |
| `snapshot` | 将解析结果转换成可安全绘制的单元格和快照 |
| `ansi` | 前景颜色的 ANSI 编码 |

Shell 决定何时进入或交还终端，本 crate 执行具体设置；UI 只绘制快照。正常退出、临时交还终端和可捕获的异常均恢复由本程序开启的模式。

子终端尺寸没有变化时，`resize` 直接返回，不重复发送尺寸通知或增加画面版本；脚本工作区持续轮询时也能保持空闲画面稳定。

`PtyProcess::into_output` 清理进程树、关闭 PTY 并收完输出后返回 `TerminalOutput`。它只保留终端解析结果，支持调整尺寸、回滚和快照，让已结束脚本仍可查看早期输出。

可注册输出接收函数，在读取线程中逐块转交完整输出，供任务历史保存；不把输入事件写入历史，也不依赖终端回滚的行数保存全部结果。

启动时借鉴 [Microsoft Edit 的颜色查询](https://github.com/microsoft/edit/blob/1be69c85a63bcd475a0b397196d0be3dbed60023/crates/edit/src/bin/edit/main.rs)，在已有图形探测中一并发送 OSC 4、10、11，读取终端的 16 个基础色及默认文字、背景色。另用 XTGETTCAP RGB 查询真彩色支持；能报告色表不等于能显示真彩色。没有回复或不支持查询时，保留环境变量判断及默认颜色。回复支持分段到达、BEL/ST 两种结束符和 1 至 4 位十六进制通道；错误条目不会覆盖已读到的有效颜色。渲染只读取缓存，不与输入线程争抢终端数据。真彩色模式下，主题先把命名色解析成终端实际颜色，再计算派生颜色。

Command Line 的输出先进入内存终端，不直接写入宿主；OSC 控制串（包括剪贴板请求）被过滤。强制退出需要清理子进程树，不能只隐藏页面。

macOS 等非 Linux Unix 平台清理已退出的子终端时，将不存在的进程组视为已清理；Vim 的 `:q!`、`:wq` 和正常命令行退出不会因此触发严重错误。

强制终止和析构统一使用 `ManagedResource::stop()`，等待子进程回收后再释放终端句柄；成功清理只执行一次，避免关闭仍在运行或暂停显示的 Vim 时重复发送信号而误报权限错误。

macOS 关闭子终端时，使用系统进程查询先结束同一终端会话内的后台进程组，再结束 Shell 进程组；关闭一个终端不会终止其他终端的任务。

Windows Terminal 下的 WSL 优先使用 win32-input-mode 区分 Ctrl+Enter 与 Ctrl+J；tmux/screen 中不启用该模式。其他 Linux 终端尝试 Kitty 报告，再尝试 xterm modifyOtherKeys。不支持的终端按实际事件处理，不能猜测组合键。

验证：`cargo test --locked -p terminal-runtime`。模式开关、退出恢复和输入编码还需真实 Unix PTY 或 Windows ConPTY；单元测试通过不能代替实机输入验收。

相关文档：[Shell 会话](../shell/docs/session.md)、[Command Line](../cli/docs/commands.md)、[crossterm 本地补丁](../../third_party/crossterm/TUNDRA_PATCH.md)。

## Command Line 宿主

Shell 从自身二进制目录启动 `tundra-cli repl --embedded`，使用 `xterm-256color` 和带 2,000 行回滚的 vt100 内存屏幕，再把单元格画在 Shell 内容区。每个页面实例拥有独立 PTY，覆盖时仍读取输出。Ctrl+C、Esc 和 Ctrl+Shift+X 发给子 CLI；右上角按钮确认后仅结束当前实例的子进程树（Windows Job Object、Unix 进程组和后代进程）。`exit` 返回原来的 Launcher 或 Explorer；退出码 75 通知 Shell 重置存储并重启。

输入模式的拆包、Unicode、粘贴和鼠标验证位于 `third_party/crossterm/tests/escape_input.rs`；模式开关及组合键区分位于 `crates/terminal-runtime/tests/unit/terminal_keyboard_pty.rs`。

子终端快照保留逐格斜体和子进程通过 DECSCUSR 选择的光标形状、闪烁状态；形状由 vt100 解析，不按应用名称或文字推断。按键保留 Ctrl+W/N/P/R/V、F1–F12 和带修饰键的导航序列；松键事件不重复发送字符，粘贴继续遵守子终端的 bracketed-paste 模式。

`PtyProcess::send_mouse` 按子进程当前启用的鼠标跟踪模式和 Default、UTF-8、SGR 编码发送物理按下、释放、拖动和滚轮事件，并将坐标转换为内容区内的坐标。没有启用子终端鼠标或事件落在内容区外时返回 `false`，宿主可以继续处理自己的控件和回滚；子终端已接管内容区时返回 `true`，不会把 Vim 等终端程序的滚轮再送入宿主回滚。合成的 Click/DoubleClick 不重复生成已有的按下/释放报告。协议、边界和快照回归位于 `tests/unit/input.rs`、`tests/unit/pty.rs`。
