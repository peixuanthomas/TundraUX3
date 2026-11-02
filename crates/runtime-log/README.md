# runtime-log

`runtime-log` 保存和查询结构化运行事件，处理脱敏、轮转、保留期限、容量限制和安全清理。Shell 的 Logs 页面与 CLI 共用这些数据，Linux journal 和指定文件读取由 `platform` 提供。

## 模块

| 模块 | 职责 |
| --- | --- |
| `model` | 事件、级别、查询和状态类型 |
| `writer` | 有界队列、去重、刷新、轮转和容量管理 |
| `reader` | 查询、过滤、导出和有界读取 |
| `privacy` | 敏感数据与控制序列清理 |
| `clear` | 预览和清理受管理日志文件 |

UX 故障阶段 `Incident` 在查询时统一映射到最高等级 `Critical`，原始文件保持不变，恢复结果继续使用原等级。

日志正文使用固定语言和稳定事件代码，不由可编辑翻译文件生成。密码、令牌、剪贴板、文件正文和命令内容不作为日志上下文。

## 文档与验证

[日志指南](docs/logs.md)说明来源、权限、TUI、CLI、跟随、导出、清理和磁盘配额。事故身份与任务编号由 [watchdog](../watchdog/README.md) 管理。

```sh
cargo test --locked -p runtime-log
```

修改页面或命令后同时运行受影响的 Shell/CLI 测试，Linux journal/dmesg 数据源在 Linux 环境验证。
