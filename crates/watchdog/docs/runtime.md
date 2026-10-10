# Watchdog 的退出、恢复与报告

宿主初始化与任务 API 见 [crate README](../README.md)。

## 运行约束

每个进程只创建一个 `WatchdogRuntime`。它提供进程级 panic 边界、受管理任务/线程、恢复策略、运行 journal 和事故报告；`ManagedTaskGroup` 统一管理线程与 Tokio 任务。所有可能 panic 的生产后台工作都应进入 managed task group，并声明是否可安全重放。

长期运行的线程通过 `spawn_thread_with_cancellation` 接收停止请求，并在等待期间检查 `ThreadCancellation`。Linux 的 logind 休眠和关机监听在连接、订阅、等待信号及断线重试时均可取消；正常退出会结束监听，不会因一直等不到系统信号而触发 watchdog 关闭超时。

重启策略受重放安全性约束：只有 `Idempotent`，或具备恢复处理器的 `Checkpointed` 任务允许重启；`Never + RestartTask` 组合会被拒绝。

`OperationGuard` 在下列目录以原子方式维护操作 journal：

```text
<data>/watchdog/operations/<app-id>/
```

操作 commit 时删除 journal，未提交的 `Drop` 标记为 `interrupted`。若无法安全恢复，会保留 journal 并阻止同类变更，不能悄然继续执行。

活动运行标记位于 `<data>/watchdog/runs/`，让下次启动能记录本进程未及观察的异常退出。每起事故生成 JSON 和文本报告：主报告目录失败时依次尝试 fallback，再写入 stderr。报告会集中脱敏并限制大小：文本最多 4,096 bytes，数组最多 256 项；默认保留最近 30 起、30 天、总量不超过 50 MiB。调用方也不得将密码、token、剪贴板内容或原始用户输入写入事故上下文。

正常退出和大多数 panic 都应恢复 raw mode、鼠标捕获、备用屏幕、颜色与光标。若进程被强制终止，先重置当前终端，再检查 crashes 报告；下一次启动会利用未关闭的运行标记生成“原因未知”的事故记录。
