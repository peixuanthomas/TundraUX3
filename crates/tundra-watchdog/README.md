# tundra-watchdog

`tundra-watchdog` is the single process-level supervision runtime for TundraUX3.
Executables own one `WatchdogRuntime`, install its `ProcessWatchdog` once, and
register every application before starting application code. Libraries receive
an `AppWatchdog`; they never install panic hooks or create unmanaged production
threads/tasks.

## Host setup

```rust,no_run
use tundra_watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig,
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

The process host is the only incident consumer. It chooses a TUI critical
modal, stderr, or a native critical dialog and de-duplicates by incident ID.
The watchdog crate emits structured receipts and deliberately has no dependency
on `tundra-platform`.

## Task and recovery rules

- Create production work through `ManagedTaskGroup` only.
- Declare `ReplaySafety` for every task. `Never + RestartTask` is rejected.
- `Idempotent` tasks may be reconstructed by their factory under the configured
  restart window and backoff.
- `Checkpointed` tasks restart only after a registered `RecoveryHandler`
  establishes a safe state.
- Mutating operations use `begin_operation` and durable checkpoints. A manual
  outcome keeps the journal and must leave related writes disabled.
- Dropping/shutting down a task group cancels and reaps tracked work; cancellation
  is not reported as a panic.

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
