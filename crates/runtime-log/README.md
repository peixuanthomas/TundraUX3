# runtime-log

`runtime-log` 保存和查询结构化运行事件，处理脱敏、轮转、保留期限、容量限制和安全清理。Shell 的 Logs 页面及 CLI 共用这些数据；Linux journal 和指定日志文件的读取由 `platform` 提供。

| 模块 | 职责 |
| --- | --- |
| `model` | 事件、级别、查询和状态类型 |
| `writer` | 有界写入队列、去重、刷新、轮转和容量管理 |
| `reader` | 查询、过滤和有界读取 |
| `privacy` | 敏感数据与控制序列清理 |
| `clear` | 预览和清理受管理日志文件 |

日志正文使用固定语言和稳定事件代码，不能用可编辑的翻译文件生成。密码、令牌、剪贴板、文件正文和命令内容不得作为日志上下文。

[日志指南](docs/logs.md)集中说明权限、TUI、查询、跟随、导出、清理、退出码和磁盘配额。事故报告及任务编号由 [watchdog](../watchdog/README.md) 管理，普通操作失败不自动成为事故。

验证：`cargo test --locked -p runtime-log`，再运行受影响的 Shell/CLI 日志测试；Linux 数据源必须在 Linux 检查。
