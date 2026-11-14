# watchdog

`watchdog` provides the workspace guardian and the in-process runtime for managed tasks, panic boundaries, operation journals, resource cleanup, and incident reports. Worker executables create a `WatchdogRuntime`; libraries use the existing instance through `AppWatchdog`.

## Workspace guardian

`Supervisor` runs in the parent guardian and owns the Shell, application, and shared service processes. Workers report progress from their actual event loops every 250 ms. After two seconds without progress the guardian sends a probe, then allows two seconds for a matching reply carrying a newer progress counter. A scheduling pause on the guardian triggers a fresh probe.

An unresponsive app or service remains available for inspection. Shell failure triggers force termination and workspace shutdown. Responsive workers have up to two seconds to close after Shell stops. An ordinary app close that takes more than two seconds prompts the user for a decision.

The guardian initiates force termination with an owned process identity and publishes exit after confirming cleanup and reaping the worker. Workers register managed Unix process groups, PTY sessions, and additional Windows jobs through the guardian. On macOS, an independently daemonized process that has already lost its parent link and left all registered sessions falls outside the tracked tree.

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

External desktop programs use a separate launch path and may continue running after Tundra exits. Update replacement retains an explicit launch owner. A supervised Linux management helper keeps its existing authorized process and watches the originating worker by pidfd. It closes its own process tree when that worker exits. Disconnecting the listening interface preserves an operation while its owner is alive.

## Reports and run markers

Reports are saved as matching JSON and text files. Operation journals reside in:

```text
<data>/watchdog/operations/<app-id>/
```

Active run markers reside in `<data>/watchdog/runs/`, allowing the next startup to record abnormal exits that were not observed.

Incident context uses minimal structured metadata, without passwords, tokens, clipboard contents, pasted content, or raw user input. Centralized redaction and size limits are also applied before persistence.

The runtime requires `panic = "unwind"` and catches Rust panics at declared boundaries. Process exits outside those boundaries are recorded at the next startup using run markers. The workspace guardian ends the run when Shell stops responding.

See [Runtime, recovery, and reports](docs/runtime.md) for detailed rules.

## Validation

```sh
cargo test --locked -p watchdog
```
