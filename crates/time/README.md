# time

`time` 提供 `NetworkClock`、`ClockDisplay` 和 `ClockSnapshot`。`clock.rs` 推进时间并转换时区，`network.rs` 获取 HTTP 时间；闹钟和计时任务属于 [app](../app/README.md)。

1. 默认每 5 分钟依次请求 Google、Cloudflare、Microsoft 的 HTTP `Date` 响应头，每次连接和总请求超时均为 5 秒。这不是 NTP。
2. 成功后保存 UTC 时间与单调时钟 `Instant`，用经过的时间推进当前值。
3. 通过 `chrono-tz` 转换到配置的 IANA 时区，夏令时不使用固定偏移。
4. 同步失败时保留可信的既有时间；没有可信时间时才使用系统时钟。

`NetworkClock::operating_system()` 每次读取当前系统时间和本地时区，不使用已保存的网络时间；Linux Shell 使用此模式。演示平台设置仍可选择平台时钟、默认 HTTP(S) 服务器或自定义地址。自定义地址验证成功后才保存；本 crate 不修改操作系统时间。

验证：`cargo test --locked --manifest-path crates/time/Cargo.toml`（从仓库根目录运行，避免与同名依赖混淆）。测试只检查离线日期解析、服务器地址校验和本地时钟计算，不发起 HTTP 请求。
