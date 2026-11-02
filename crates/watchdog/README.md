# watchdog

`watchdog` 是进程级监督运行时，负责托管任务与线程、panic 边界、操作 journal、资源清理和事故报告。可执行程序创建一个 `WatchdogRuntime`，库通过 `AppWatchdog` 使用已有实例。

## 宿主初始化

```rust,no_run
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let (runtime, process) = WatchdogRuntime::start(WatchdogConfig::new(
    "logs", "fallback", "data", "host", env!("CARGO_PKG_VERSION"),
))?;
let process = process.install_global()?;
let app = process.register_app(AppDescriptor::new(
    AppId::from_static("example"),
    "Example",
    env!("CARGO_PKG_VERSION"),
    AppCriticality::Optional,
))?;

// 将 app 或包含它的 AppRuntimeContext 交给应用代码。
runtime.shutdown()?;
# Ok(())
# }
```

进程宿主负责事故展示与去重。Shell 使用独立 panic 页面，其他宿主可使用 stderr 或原生错误提示。`watchdog` 不依赖 `platform`。

## 任务与恢复

- 生产后台工作通过 `ManagedTaskGroup` 创建，并声明 `ReplaySafety`。
- `Never + RestartTask` 会被拒绝。
- `Idempotent` 任务可在配置的重启窗口与退避策略内重建。
- `Checkpointed` 任务需要注册的 `RecoveryHandler` 先恢复安全状态。
- 修改操作使用 `begin_operation` 和持久 checkpoint，人工处理结果后保留 journal 并阻止相关写入。
- 关闭任务组会取消并等待受管理工作；仅丢弃句柄不会停止线程。
- 长时间等待需要响应取消，取消本身不作为 panic 报告。

内部命令使用 `platform::ManagedCommand`，所有者在错误与 Drop 时停止并回收子进程。`ManagedResource` 也覆盖 PTY 子进程，清理成功后注销，失败时保留登记以便重试。

桌面外部程序使用独立启动路径，Tundra 退出后程序可继续运行。更新替换保留明确的启动所有者，系统管理助手将任务交给自己的 watchdog，界面断线不会直接中断软件包写入。

## 报告与运行标记

报告以匹配的 JSON 和文本文件保存，操作 journal 位于：

```text
<data>/watchdog/operations/<app-id>/
```

活动运行标记位于 `<data>/watchdog/runs/`，下次启动可据此记录未观察到的异常退出。

事故上下文使用最小结构化元数据，不写入密码、令牌、剪贴板、粘贴内容或原始用户输入。持久化前还会集中脱敏并限制大小。

运行需要 `panic = "unwind"`，在声明的边界捕获 Rust panic。无法立即捕获的进程退出由运行标记在下次启动时补充记录；需要自动重启的场景由外部监督程序负责。

详细规则见[运行、恢复与报告](docs/runtime.md)。

## 验证

```sh
cargo test --locked -p watchdog
```
