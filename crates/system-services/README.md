# system-services

`system-services` 定义天气、时间、存储、网络和系统指标快照；开启 `runtime` 功能后，再启动查询、缓存与后台刷新。

| 构建方式 | 包含内容 | 使用者 |
| --- | --- | --- |
| 默认，无功能 | `model/` 的共享数据 | app、weathr |
| `features = ["runtime"]` | `runtime/` 的服务、请求、缓存和刷新 | Shell |

Cargo 会合并同一次构建启用的功能，因此 workspace 构建包含后台服务；单独构建 Weathr 时不会因此引入 `reqwest`、`platform`、`watchdog` 或 `time`。

```mermaid
flowchart LR
    Platform[平台原生采集] --> Snapshot[不可变 SystemSnapshot]
    Snapshot --> App[app 快照]
    App --> Shell[Shell 按权限整理]
    Shell --> UI[UI 显示与通知]
```

系统指标前台每 5 秒、后台每 30 秒采样，也可立即刷新。每项数据独立标记过期或不可用；一项失败不会让其他数据失效。网络 link 只说明本机接口的链路状态，不能代表互联网或服务可达；采集不读取 MAC/SSID，也不执行外部探测。

Linux 按当前进程权限显示数据；Windows/macOS 本地管理员和普通用户可看明细，访客没有 System Status 入口。写操作另行检查权限。

天气提供方支持 Open-Meteo 和 Met Office，但应用和启动预取目前固定使用 Open-Meteo。天气内存和磁盘缓存为 300 秒，位置、地址及地理编码缓存为 24 小时；显式刷新绕过缓存。

```sh
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
bash scripts/check-weathr-dependency-boundary.sh
```
