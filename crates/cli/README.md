# cli

`cli` 提供 `tundra-cli`：公开配置、诊断、日志、Linux 运维命令和交互命令行。它也承载更新及授权助手入口；这些内部入口不作为普通脚本接口。

`src/commands/` 按命令分组；参数解析、REPL 和程序入口留在 crate 根模块，Linux 管理连接使用共享的 platform 接口。

| 用法 | 示例 |
| --- | --- |
| 外部终端 | `tundra-cli debug doctor` |
| 内建 Command Line 或 `repl` 中的 UX 命令 | `/debug doctor` |
| 内建 Command Line 或 `repl` 中的系统命令 | `ls -la` 或 `dir` |
| 查询帮助 | `tundra-cli help`、`tundra-cli users --help` |

普通输入交给系统命令解释器，UX 命令必须加 `/`。命令行保留当前会话的目录和已导出环境变量，不改变主进程或系统配置。

- [命令、配置与 REPL](docs/commands.md)
- [运维命令、系统授权和退出码](docs/operations.md)
- [日志查询、跟随和导出](../runtime-log/docs/logs.md)
- [界面预览](../shell/docs/previews.md)

运行 `cargo build --locked -p shell -p cli` 后再验证内嵌命令行。CLI 使用 Shell 提供的界面预览，不能把它描述为完全不依赖界面代码。普通帮助和 Command Line 提示使用英文。

验证：`cargo test --locked -p cli`。
