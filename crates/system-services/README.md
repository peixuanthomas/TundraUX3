# system-services

`system-services` defines snapshots for weather, time, storage, networking, and system metrics. Enabling the `runtime` feature also provides platform collection, network requests, caching, and background refresh.

| Build configuration | Content | Consumers |
| --- | --- | --- |
| Default, no features | Shared data in `model/` | app, weathr |
| `features = ["runtime"]` | Shared data and `runtime/` services | Shell |

Data consumers do not need to pull in `reqwest`, `platform`, `watchdog`, or `time`. Cargo unifies features in workspace builds, so Shell uses the full runtime. Storage, network, and metric snapshots support serialization so isolated app hosts consume the same sampled state; requests for active sampling and immediate refresh are forwarded to the Shell through the shared services process.

```mermaid
flowchart LR
    Platform[Platform collection] --> Snapshot[Immutable SystemSnapshot]
    Snapshot --> App[app snapshot]
    App --> Shell[Shell organizes data by permissions]
    Shell --> UI[UI display and notifications]
```

## System metrics

Sampling runs every 5 seconds in the foreground and every 30 seconds in the background, with explicit immediate refresh also available. Each metric is independently marked stale or unavailable; one failure does not affect other metrics. Network link status describes only the local interface link. Collection does not read MAC/SSID or probe external connectivity.

Linux displays data according to the current process's permissions. Windows/macOS local administrators and ordinary users can view details; Guest has no System Status entry. Write operations check permissions separately.

## Weather and time

The application and startup prefetch use Open-Meteo. Weather memory and disk caches last 300 seconds; location, address, and geocoding caches last 24 hours. Explicit refresh bypasses the cache.

System-time mode reads system time for every snapshot and also uses the system's local timezone on Linux. Network-time mode advances from the most recent successful synchronization using a monotonic clock. See [time](../time/README.md) for details.

## Validation

```sh
cargo test --locked -p system-services --no-default-features
cargo test --locked -p system-services --features runtime
bash scripts/check-weathr-dependency-boundary.sh
```
