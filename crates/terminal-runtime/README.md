# terminal-runtime

`terminal-runtime` 管理宿主终端模式和隔离的子终端。命令行、外部程序、Vim 和脚本共用这套 PTY/ConPTY、输入编码、终端解析和进程清理实现。

## 模块

| 模块 | 职责 |
| --- | --- |
| `session` | `TerminalGuard`、宿主终端模式、颜色与图形能力探测 |
| `pty` | 子进程启动、尺寸调整、输出读取和进程树清理 |
| `emulator` | WezTerm 内存终端、协议回复、同步刷新和图片状态 |
| `input` | 按子终端模式编码按键、粘贴和鼠标 |
| `input_queue` | 有界输入队列和受 watchdog 监督的写入线程 |
| `snapshot` | 将解析结果转换为可绘制的单元格和共享快照 |
| `ansi` | 前景颜色的 ANSI 编码 |

Shell 决定何时进入或交还终端，本 crate 执行设置与恢复，UI 只绘制快照。

## 宿主终端

`TerminalGuard` 管理 raw mode、备用屏幕、鼠标、焦点、括号粘贴和增强按键报告。正常退出、临时交还终端和可捕获的异常都会恢复由程序开启的模式。

启动探测可读取终端基础色、默认前景/背景和真彩色能力。查询支持分段回复与常见结束符，渲染只读取缓存结果。Windows Terminal 中的 WSL 优先使用 win32-input-mode，其他终端按能力选择 Kitty 或 xterm 按键报告。

## 子终端

`PtyConfig`、`PtyProcess`、`PtyExitStatus` 是通用进程接口。默认配置不添加应用参数或私有环境变量，内置 CLI 的启动方式由 Shell 决定。

- `send_key`、`send_paste` 和 `send_mouse` 按发送时的子终端模式编码输入。
- 鼠标坐标由宿主内容区转换为子终端内部坐标，并按 Default、UTF-8 或 SGR 协议发送。
- 输入队列上限为 4 MiB，唯一写入线程由 watchdog 管理，按键、协议回复、鼠标和粘贴按序写入。
- `resize` 在尺寸不变时直接返回，避免重复通知和画面版本变化。
- `into_output` 清理进程树并收完输出，返回支持回滚、缩放和快照的 `TerminalOutput`。
- 输出接收函数可在读取线程中逐块保存完整任务历史，不受终端回滚行数限制。

Unix 使用进程组，Windows 使用 Job Object 清理对应子进程树。macOS 还会结束同一终端会话内的后台进程组，不影响其他终端实例。

## 终端解析与快照

子程序输出进入 WezTerm 内存终端，不直接操作宿主。剪贴板、标题、未知 OSC 和文件下载不会传递给宿主终端。

`ui_snapshot_with_revision`、`ui_snapshot_if_changed` 和 `ui_snapshot_after_exit` 可直接生成 UI 快照。静止画面复用 `Arc`，输出、缩放或退出刷新时更新。快照保留逐格样式、斜体和 DECSCUSR 光标形状。

Kitty 键盘协商支持按下、重复、松开和扩展按键。Kitty、Sixel、iTerm2 图片解析为共享 RGBA 数据与裁切矩形，图片解码预算为 2,000 万像素和 8,000 万字节；文本任务历史不保存图片载荷。同步刷新在结束序列或 200ms 超时后发布画面。

## Command Line 宿主

Shell 从自身二进制目录启动 `tundra-cli repl --embedded`，使用 `xterm-256color` 和 2,000 行回滚。每个页面实例拥有独立 PTY，覆盖时继续读取输出。

Ctrl+C、Esc 和 Ctrl+Shift+X 交给子 CLI；右上角返回按钮确认后只清理当前实例。正常 `exit` 返回原 Launcher 或 Explorer，内置 CLI 的特殊退出码由命令行宿主解释。

## 验证

```sh
cargo test --locked -p terminal-runtime
```

输入编码与协议单元测试位于 `tests/unit/input.rs`、`tests/unit/pty.rs` 和 `tests/unit/terminal_keyboard_pty.rs`。宿主模式恢复和组合键行为还需真实 Unix PTY 或 Windows ConPTY 验证。

相关文档：[Shell 会话](../shell/docs/session.md)、[Command Line](../cli/docs/commands.md)、[crossterm 本地补丁](../../third_party/crossterm/TUNDRA_PATCH.md)、[WezTerm 本地补丁](../../third_party/wezterm-term/TUNDRA_PATCH.md)。
