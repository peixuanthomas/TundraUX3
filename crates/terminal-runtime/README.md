# terminal-runtime

`terminal-runtime` 管理宿主终端和子终端。PTY 是供子进程使用的终端连接，让命令行程序可以读取按键、移动光标并输出完整终端画面。

| 模块 | 职责 |
| --- | --- |
| `session` | `TerminalGuard`：进入和恢复 raw mode、备用屏幕、鼠标、焦点、粘贴和增强按键报告 |
| `pty` | 子进程启动、尺寸调整、输出读取和进程清理 |
| `input` | 按子终端模式编码按键与粘贴 |
| `snapshot` | 将解析结果转换成可安全绘制的单元格和快照 |
| `ansi` | 前景颜色的 ANSI 编码 |

Shell 决定何时进入或交还终端，本 crate 执行具体设置；UI 只绘制快照。正常退出、临时交还终端和可捕获的异常均恢复由本程序开启的模式。

Command Line 的输出先进入内存终端，不直接写入宿主；OSC 控制串（包括剪贴板请求）被过滤。强制退出需要清理子进程树，不能只隐藏页面。

Windows Terminal 下的 WSL 优先使用 win32-input-mode 区分 Ctrl+Enter 与 Ctrl+J；tmux/screen 中不启用该模式。其他 Linux 终端尝试 Kitty 报告，再尝试 xterm modifyOtherKeys。不支持的终端按实际事件处理，不能猜测组合键。

验证：`cargo test --locked -p terminal-runtime`。模式开关、退出恢复和输入编码还需真实 Unix PTY 或 Windows ConPTY；单元测试通过不能代替实机输入验收。

相关文档：[Shell 会话](../shell/docs/session.md)、[Command Line](../cli/docs/commands.md)、[crossterm 本地补丁](../../third_party/crossterm/TUNDRA_PATCH.md)。

## Command Line 宿主

Shell 从自身二进制目录启动 `tundra-cli repl --embedded`，使用 `xterm-256color` 和带 2,000 行回滚的 vt100 内存屏幕，再把单元格画在 Shell 内容区。Ctrl+C 发给子 CLI；Ctrl+Shift+X 清理子进程树（Windows Job Object、Unix 进程组）。`exit` 返回原来的 Launcher 或 Explorer；退出码 75 通知 Shell 重置存储并重启。

输入模式的拆包、Unicode、粘贴和鼠标验证位于 `third_party/crossterm/tests/escape_input.rs`；模式开关及组合键区分位于 `crates/terminal-runtime/tests/unit/terminal_keyboard_pty.rs`。
