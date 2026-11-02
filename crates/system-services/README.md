# system-services

`system-services` 定义天气、时间、存储、网络和系统指标快照。启用 `runtime` 功能后，还提供平台采集、网络请求、缓存和后台刷新。

| 构建方式 | 内容 | 使用者 |
| --- | --- | --- |
| 默认，无功能 | `model/` 的共享数据 | app、weathr |
| `features = ["runtime"]` | 共享数据及 `runtime/` 服务 | Shell |

数据消费者无需引入 `reqwest`、`platform`、`watchdog` 或 `time`。Cargo 在 workspace 构建中合并功能，因此 Shell 使用完整运行时。

```mermaid
flowchart LR
    Platform[平台采集] --> Snapshot[不可变 SystemSnapshot]
    Snapshot --> App[app 快照]
    App --> Shell[Shell 按权限整理]
    Shell --> UI[UI 显示与通知]
```

## 系统指标

前台每 5 秒、后台每 30 秒采样，也可显式立即刷新。各项数据独立标记过期或不可用，单项失败不影响其他指标。网络 link 只表示本机接口链路状态，采集不读取 MAC/SSID，也不执行外部连通性探测。

Linux 按当前进程权限显示数据；Windows/macOS 本地管理员和普通用户可查看明细，Guest 没有 System Status 入口。写操作另行检查权限。

## 天气与时间

应用与启动预取使用 Open-Meteo。天气内存和磁盘缓存为 300 秒，位置、地址及地理编码缓存为 24 小时，显式刷新绕过缓存。

系统时间模式每次生成快照都读取系统时间，Linux 同时使用系统本地时区。网络时间模式按最近成功同步的时间和单调时钟推进，详见 [time](../time/README.md)。

## 验证

```sh
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
bash scripts/check-weathr-dependency-boundary.sh
```
