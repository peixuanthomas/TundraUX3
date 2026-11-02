# Watchdog 运行、恢复与报告

宿主初始化与任务 API 见 [crate README](../README.md)。

## 运行约束

每个进程创建一个 `WatchdogRuntime`，提供进程级 panic 边界、托管任务、恢复策略、操作 journal 和事故报告。`ManagedTaskGroup` 统一管理线程与 Tokio 任务。

长期线程通过 `spawn_thread_with_cancellation` 接收停止请求，并在等待期间检查 `ThreadCancellation`。系统信号监听、断线重试和阻塞读取都需要响应取消，避免超出关闭期限。

重启策略由 `ReplaySafety` 决定：只有 `Idempotent` 或具有恢复处理器的 `Checkpointed` 任务可以重启，`Never + RestartTask` 会被拒绝。

## 操作 journal

`OperationGuard` 在以下目录原子维护 journal：

```text
<data>/watchdog/operations/<app-id>/
```

操作 commit 后删除 journal，未提交的 Drop 标记为 `interrupted`。无法安全恢复时保留 journal 并阻止同类变更，等待人工处理。

## 事故报告与保留

活动运行标记位于 `<data>/watchdog/runs/`。每起事故生成 JSON 和文本报告，主报告目录不可用时尝试 fallback，最后写入 stderr。

持久化前集中脱敏并限制大小：文本最多 4,096 bytes，数组最多 256 项。默认保留最近 30 起、30 天、总量不超过 50 MiB。调用方只提供必要的结构化上下文，不写入密码、令牌、剪贴板或原始输入。

正常退出和可捕获的 panic 都应恢复 raw mode、鼠标、备用屏幕、颜色与光标。进程被强制终止后，先重置当前终端，再检查 crashes 报告；下次启动利用未关闭的运行标记补充“原因未知”的记录。

## 验证

```sh
cargo test --locked -p watchdog
```

测试覆盖任务取消、关闭等待、panic 边界、journal、资源清理、报告脱敏与保留策略。真实终端恢复由对应平台的 PTY/ConPTY 测试验证。
