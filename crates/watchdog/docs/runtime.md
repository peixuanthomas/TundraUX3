# Watchdog runtime, recovery, and reports

See the [crate README](../README.md) for host initialization and task APIs.

## Runtime constraints

Each process creates one `WatchdogRuntime`, providing a process-level panic boundary, managed tasks, recovery policies, operation journals, and incident reports. `ManagedTaskGroup` manages threads and Tokio tasks through a common interface.

Long-lived threads receive stop requests through `spawn_thread_with_cancellation` and check `ThreadCancellation` while waiting. System-signal listeners, reconnection retries, and blocking reads must all respond to cancellation to avoid exceeding the shutdown deadline.

Restart policies are determined by `ReplaySafety`: only `Idempotent` tasks or `Checkpointed` tasks with recovery handlers can restart. `Never + RestartTask` is rejected.

## Workspace process supervision

`Supervisor` associates each child with a run, role, session, and generation. Its monotonic deadlines use progress from the worker loop. The guardian may continue accepting close and force-stop requests while a child is stuck, and it delegates native process-tree cleanup to independent workers.

Unix starts each guardian child in its own process group and keeps the group's leader unreaped until cleanup completes. Independent PTY sessions and command groups register before running their commands. Cleanup includes session members and descendants still linked to a registered tree. Windows starts children suspended, assigns them to a Job Object, and resumes them once containment is in place.

On macOS, the audit-token signal API safeguards detached process signals against PID reuse. macOS releases without this API can still signal directly owned worker groups. A detached process that cannot be signalled or verified remains in the stopping state with a supervision error.

## Operation journals

`OperationGuard` atomically maintains journals in:

```text
<data>/watchdog/operations/<app-id>/
```

Committing an operation deletes its journal; Drop without a commit marks it `interrupted`. When safe recovery is not possible, the journal is retained and changes of the same kind are blocked pending manual handling.

## Incident reports and retention

Active run markers reside in `<data>/watchdog/runs/`. Each incident generates JSON and text reports. If the primary report directory is unavailable, fallback is tried, with stderr as the final destination.

Centralized redaction and size limits are applied before persistence: up to 4,096 bytes of text and 256 array items. Default retention keeps the latest 30 incidents for up to 30 days, with a total limit of 50 MiB. Callers provide only necessary structured context, excluding passwords, tokens, clipboard contents, and raw input.

Both normal exits and catchable panics should restore raw mode, mouse state, alternate screen, colors, and cursor. After forced process termination, reset the current terminal first, then inspect crash reports. At the next startup, unclosed run markers add a record with an unknown cause.

## Validation

```sh
cargo test --locked -p watchdog
```

Tests cover task cancellation, shutdown waits, panic boundaries, journals, resource cleanup, report redaction, and retention policies. Native supervision tests exercise dropped progress, a stopped or busy Shell, an app deadlock, and child processes in separate groups and sessions. Real-terminal restoration is validated by PTY/ConPTY tests on the corresponding platform.
