# watchdog

`watchdog` is a process-level supervision runtime responsible for managed tasks and threads, panic boundaries, operation journals, resource cleanup, and incident reports. Executables create one `WatchdogRuntime`; libraries use the existing instance through `AppWatchdog`.

## Host initialization

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

// Pass app, or an AppRuntimeContext containing it, to application code.
runtime.shutdown()?;
# Ok(())
# }
```

The process host handles incident display and deduplication. Shell uses a dedicated panic page; other hosts may use stderr or native error messages. `watchdog` does not depend on `platform`.

## Tasks and recovery

- Production background work is created through `ManagedTaskGroup` and declares `ReplaySafety`.
- `Never + RestartTask` is rejected.
- `Idempotent` tasks can be recreated within the configured restart window and backoff policy.
- `Checkpointed` tasks require a registered `RecoveryHandler` to restore a safe state first.
- Modification operations use `begin_operation` and persistent checkpoints. After results are handled manually, the journal is retained and related writes are blocked.
- Closing a task group cancels and waits for managed work; merely dropping a handle does not stop a thread.
- Long waits must respond to cancellation; cancellation itself is not reported as a panic.

Internal commands use `platform::ManagedCommand`; their owner stops and reaps child processes on errors and Drop. `ManagedResource` also covers PTY child processes. Resources are unregistered after successful cleanup; failed cleanup retains the registration for retry.

External desktop programs use a separate launch path and may continue running after Tundra exits. Update replacement retains an explicit launch owner. System-management helpers hand tasks to their own watchdog, so an interface disconnection does not directly interrupt package writes.

## Reports and run markers

Reports are saved as matching JSON and text files. Operation journals reside in:

```text
<data>/watchdog/operations/<app-id>/
```

Active run markers reside in `<data>/watchdog/runs/`, allowing the next startup to record abnormal exits that were not observed.

Incident context uses minimal structured metadata, without passwords, tokens, clipboard contents, pasted content, or raw user input. Centralized redaction and size limits are also applied before persistence.

The runtime requires `panic = "unwind"` and catches Rust panics at declared boundaries. Process exits that cannot be caught immediately are recorded at the next startup using run markers. An external supervisor is responsible for scenarios requiring automatic restarts.

See [Runtime, recovery, and reports](docs/runtime.md) for detailed rules.

## Validation

```sh
cargo test --locked -p watchdog
```
