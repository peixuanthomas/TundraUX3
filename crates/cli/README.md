# cli

`cli` 提供 `tundra-cli`：配置、诊断、日志、Linux 运维命令、Shell 界面控制和交互命令行。它也承载更新与授权助手入口，这些内部入口不属于普通脚本接口。

## 常用入口

| 用法 | 示例 |
| --- | --- |
| 外部终端命令 | `tundra-cli doctor` |
| 查看帮助 | `tundra-cli help`、`tundra-cli users --help` |
| 查看版本 | `tundra-cli --version` |
| 内建 Command Line 中的 UX 命令 | `/doctor`、`/config get` |
| 内建 Command Line 中的系统命令 | `ls -la` 或 `dir` |

普通 REPL 输入交给系统命令解释器，Tundra 命令使用 `/` 前缀。会话保留工作目录和已导出的环境变量，不改变 Tundra 主进程或系统环境配置。

## 模块与职责

- `src/arguments.rs` 解析公开参数和配置字段。
- `src/commands/` 按 asset、config、debug、doctor、launcher、logs、management、ui 等命令分组。
- `src/management_client.rs` 复用 platform 的管理连接。
- `src/repl/` 提供交互命令行、补全和嵌入宿主协议。
- `src/runner.rs` 安排配置、存储、watchdog 和命令执行。

帮助与版本查询直接输出内置文本，不启动 watchdog 或读取配置。`doctor` 是顶层命令，原顶层调试入口统一放在 `debug` 下。

## Shell 界面控制

`tundra-cli ui` 可列出当前用户的 Shell 会话、读取界面快照，并发送点击、移动、拖动、滚动、按键和文字。功能默认关闭，在“设置 → 系统 → 外部 CLI 控制”中开启。

```sh
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui wake
```

结果使用协议版本 1 的 JSON。多个会话需要 `--session`，真实用户操作后需要重新读取快照，输入超时不会自动重发。详见 [UI 控制](docs/ui-control.md)。

## 文档

- [命令、配置与 REPL](docs/commands.md)
- [运维命令、授权与退出码](docs/operations.md)
- [UI 控制与 JSON 协议](docs/ui-control.md)
- [日志查询、跟随与导出](../runtime-log/docs/logs.md)
- [调试界面预览](../shell/docs/previews.md)

## 构建与验证

```sh
cargo build --locked -p shell -p cli
cargo test --locked -p cli
```

Shell 的内置 Command Line 使用同目录 `tundra-cli`，更新后同时构建 Shell 和 CLI。UI 控制的真实 Linux PTY 验证使用 `python3 scripts/tests/shell-control.py target/debug/tundra-shell`。
