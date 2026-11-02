# time

`time` 提供 `NetworkClock`、`ClockDisplay` 和 `ClockSnapshot`，负责时间推进、HTTP 时间同步和时区转换。闹钟与计时任务属于 [app](../app/README.md)。

## 工作方式

1. 默认每 5 分钟依次请求 Google、Cloudflare、Microsoft 的 HTTP `Date` 响应头，每次连接和总请求超时均为 5 秒。
2. 成功后保存 UTC 时间和单调时钟 `Instant`，用经过的时间推进当前值。
3. 使用 `chrono-tz` 转换到配置的 IANA 时区，夏令时由时区规则处理。
4. 同步失败时保留最近一次可信时间，没有可信时间时使用系统时钟。

HTTP 时间用于应用显示，不修改操作系统时间。

`NetworkClock::operating_system()` 每次读取系统时间和本地时区，Linux Shell 使用此模式。演示平台设置仍可选择平台时钟、默认 HTTP(S) 服务器或自定义地址，自定义地址验证成功后保存。

## 验证

```sh
cargo test --locked --manifest-path crates/time/Cargo.toml
```

从仓库根目录运行可避免与工作区同名依赖混淆。测试覆盖离线日期解析、服务器地址校验和本地时钟计算，不发起 HTTP 请求。
