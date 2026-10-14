# watchdog

[中文运行与恢复约束](docs/runtime.md) · [Workspace guide](../../docs/README-TECHNICAL.md)

`watchdog` is the single process-level supervision runtime for TundraUX3.
Executables own one `WatchdogRuntime`, install its `ProcessWatchdog` once, and
register every application before starting application code. Libraries receive
an `AppWatchdog`; they never install panic hooks or create unmanaged production
threads/tasks.

## Host setup

```rust,no_run
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig,
    WatchdogRuntime};

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

// Give `app` (or an AppRuntimeContext containing it) to application code.
runtime.shutdown()?;
# Ok(())
# }
```

The process host is the only incident consumer. It chooses the host presentation: Shell uses its independent panic page;
other hosts may use stderr or a native critical dialog and de-duplicates by incident ID.
The watchdog crate emits structured receipts and deliberately has no dependency
on `platform`.

## Task and recovery rules

- Create production work through `ManagedTaskGroup` only.
- Declare `ReplaySafety` for every task. `Never + RestartTask` is rejected.
- `Idempotent` tasks may be reconstructed by their factory under the configured
  restart window and backoff.
- `Checkpointed` tasks restart only after a registered `RecoveryHandler`
  establishes a safe state.
- Mutating operations use `begin_operation` and durable checkpoints. A manual
  outcome keeps the journal and must leave related writes disabled.
- Shutting down a task group cancels and waits for tracked work; dropping a
  handle alone does not stop its worker. Threads must observe cancellation and
  bound blocking waits. Cancellation is not reported as a panic.

Internal commands use `platform::ManagedCommand`. Their owners stop and reap
children on errors and on drop, and register cleanup with the current watchdog.
Process shutdown stops registered resources before waiting for worker threads;
cleanup failures and workers that exceed the shutdown deadline return an error.
`ManagedResource` also covers PTY children. Cleanup must have a time limit and
must not wait for another worker that needs shutdown to finish. Successful
cleanup unregisters the resource; failures remain registered for a later retry.

Desktop applications use the separate external-launch path. Its reaper stops
waiting when UX exits, while the application continues independently. Update
replacements keep an explicit owning launcher until the replacement exits.
System-operation helpers transfer work to their own watchdog so disconnecting
the UI does not interrupt a package database write. The privileged authorization
broker monitors its private connection and cancels its own work on disconnect.

Reports live under the configured report directory as matching JSON and text
files. Operation journals live under
`<data>/watchdog/operations/<app-id>/`. Active-run markers live under
`<data>/watchdog/runs/` and allow the next launch to report exits that could not
be observed in-process.

## Security and hard limits

Never put passwords, tokens, clipboard/paste contents, or raw user input into
panic messages, breadcrumbs, snapshots, or operation payloads. The runtime also
applies central redaction and size limits before persistence, but callers must
still provide minimal structured context.

With `panic = "unwind"`, Rust panics can be caught at declared boundaries. An
in-process watchdog cannot recover `abort`, OOM abort, stack overflow, native
faults, forced termination, power loss, or deadlock. Run markers provide a
next-launch “reason unknown” report for many of those cases; immediate process
restart requires a separate external supervisor.
