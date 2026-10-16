# cli

`cli` 提供 `tundra-cli`：公开配置、诊断、日志、Linux 运维命令和交互命令行。它也承载更新及授权助手入口；这些内部入口不作为普通脚本接口。

`src/commands/` 按命令分组；参数解析、REPL 和程序入口留在 crate 根模块，Linux 管理连接使用共享的 platform 接口。

运维命令包括 `firewall`（监听端口与放行规则）、`ssh`（生效设置、公钥和新连接确认）、`backups`（集中差异与恢复）和 `scheduled-tasks`（既有任务及持久 service/timer）。需要系统权限的只读查询也使用同一授权流程；查询及预览支持 JSON。

| 用法 | 示例 |
| --- | --- |
| 外部终端 | `tundra-cli debug doctor` |
| 内建 Command Line 或 `repl` 中的 UX 命令 | `/debug doctor` |
| 内建 Command Line 或 `repl` 中的系统命令 | `ls -la` 或 `dir` |
| 查询帮助 | `tundra-cli help`、`tundra-cli users --help` |
| 查询版本 | `tundra-cli --version` 或 `tundra-cli -V` |

帮助（含子命令帮助和无参数启动）与版本查询无需启动 watchdog 或读取配置，日志和数据目录不可写时也能使用。

普通输入交给系统命令解释器，UX 命令必须加 `/`。命令行保留当前会话的目录和已导出环境变量，不改变主进程或系统配置。

Linux 内建 Command Line 会继续管理脱离终端会话的后代进程；正常 `exit` 先请求它们结束并等清理完成，忽略 TERM 的进程需要通过右上角按钮确认终止。

- [命令、配置与 REPL](docs/commands.md)
- [运维命令、系统授权和退出码](docs/operations.md)
- [日志查询、跟随和导出](../runtime-log/docs/logs.md)
- [界面预览](../shell/docs/previews.md)

运行 `cargo build --locked -p shell -p cli` 后再验证内嵌命令行。CLI 使用 Shell 提供的界面预览，不能把它描述为完全不依赖界面代码。`firewall`、`ssh`、`backups`、`scheduled-tasks` 的帮助和参数提示使用当前语言，提供英文、简体中文资源；无语言快照时使用内置英文，帮助查询仍不读取配置。

验证：`cargo test --locked -p cli`。
