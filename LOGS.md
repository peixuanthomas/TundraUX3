# Logs APP

Logs is a standalone home application for signed-in users. System Status retains
health checks and metrics; legacy `logs`, `activity`, and `incidents` dashboard
widgets and their layout placements are removed on load and persisted on the
next normal settings save. Other placements are preserved.

## Sources and access

- **UX log** contains structured runtime events and its Events, Files, and
  Incidents sections. It works on Linux, Windows, and macOS. Guest has no access.
  A regular TundraUX user sees only records owned by that user, captured when a
  task is submitted. Unknown-owner and legacy reports require administrator
  access. Full Incident reports remain administrator-only inside the TUI.
- **Linux log** reads journal on demand, falling back to dmesg if journal is
  unavailable. It never scans `/var/log` and never escalates privileges. The TUI
  requires an administrator; operating-system permissions still apply. Windows
  and macOS report Unsupported. Permission failures, partial output, cancellation,
  and unavailable sources are distinguished from an empty successful result.

Source and severity are separate. An Explorer `EACCES` is an Error in
`ux.explorer`, while kernel/driver messages use `linux.kernel` and external
service messages use `linux.service`. Journal priorities map to runtime severity;
raw priorities and source/time notes remain available. dmesg boot-relative times
are not represented as precise wall-clock times; unsupported time filtering is
reported explicitly. Linux records do not acquire fictitious UX run/task IDs.

## CLI

Use the installed `tundra-cli` command (or `cargo run --locked -p cli --bin tundra-cli --`):

```sh
tundra-cli logs query
tundra-cli logs query --source ux --module ux.explorer --level error --format jsonl
tundra-cli logs query --since 2026-09-01T00:00:00Z --until 2026-09-02T00:00:00Z
tundra-cli logs query --run-id RUN_ID --task-id TASK_ID --limit 500
tundra-cli logs query --source linux --level warning
tundra-cli logs incidents --incident-id INCIDENT_ID
tundra-cli logs export --run-id RUN_ID --output ./diagnostics-new
```

Queries default to the latest 200 events, with a maximum of 10,000. Supported
filters include source, UTC time range, minimum level, module, run, operation,
task, and incident. Text and JSONL output are available. There is no unlimited
follow mode. The standalone CLI relies on current OS identity and file access;
it does not accept a claimed administrator role or a TundraUX owner identity.

Exit codes: 0 success (including empty results), 1 source/permission/output
failure, 2 invalid arguments, 3 partial or truncated results, 130 cancellation.
Diagnostic export creates a new private directory and refuses to overwrite an
existing target. Its manifest records source status, truncation, damaged records,
and writer health. Partial exports retain usable records and report omissions.
Exports are user-owned artifacts outside the managed runtime retention quota.

## TUI navigation

Open **Logs** from Home. UX opens by default. Left/Right switches source; Tab
cycles UX Events/Files/Incidents. Arrow keys, Page Up/Down, Home/End, mouse
selection, wheel scrolling, and scrollbar dragging use existing UI components.
`L` cycles minimum severity, `M` module, `T` time window, and `C` clears filters.
`I` follows a selected event's Incident and `E` returns from an Incident to its
events. `R` refreshes; Enter/`O` opens a sanitized query document in the existing
read-only Editor. Escape returns with filters and selection retained. Refresh
inside that Editor regenerates the authorized query snapshot before reloading.

Files never bypass ownership checks: ordinary users do not see mixed-owner
segments, and opening a segment produces a newly filtered snapshot. Source
notices appear in the document's first JSONL metadata row. Full reports are
whitelisted, sanitized copies rather than raw crash-file paths.

## Persistence and correlation

Runtime files live under the platform logs directory's `runtime/` subdirectory.
The existing watchdog owns run/task/operation/Incident identity. Runtime event
IDs are added to breadcrumbs and report metadata, joining operations before a
fault with the corresponding report. Old watchdog report fields remain readable.
Routine operation errors do not automatically create Incidents.

Events carry schema version, UTC timestamp, event ID, process/run/app/module/
operation/task IDs, owner, phase, severity, optional native error code and cause
chain, and optional source/target paths and Incident ID. Producers log at the
boundary that knows the result. Repeated alerts are merged by identity; changes
of phase and explicit final results are immediate, with count summaries every
60 seconds. Silence and notification dismissal do not imply recovery.

The writer has a bounded 4,096-event queue, nonblocking submission, bounded event
and deduplication memory, one-second flushing, and at most two seconds of exit
flush waiting. Queue saturation and write failure are exposed as health counters
without terminal output or recursive logging. A timed-out exit flush can lose
pending events; it never waits indefinitely for a slow disk.

Default settings under `[runtime_logs]` in the existing TOML configuration:

```toml
[runtime_logs]
max_age_days = 30
max_total_mib = 200
segment_mib = 10
```

Segments rotate each UTC day or at the configured segment size. Retention uses
cross-process locking and does not remove another active writer's locked segment.
Read-only Editor snapshots share the total capacity, additionally capped at
20 MiB with a 30-minute TTL and 5 MiB per document. When safe cleanup cannot free
space, writing reports a failure instead of exceeding the configured capacity.
Watchdog crash-report retention continues to use its existing independent policy.
Settings are loaded when starting the process; changed retention applies after
restart.

Instrumentation supplies metadata only: no passwords, tokens, authentication
headers, clipboard text, file bodies, or command contents. Bounded sanitization
also removes recognizable secrets and control sequences from error/source text.

## Assets and verification

The Logs PNG and ASCII icon are stored with the default theme's home assets and
registered in the embedded catalog. Distribution, validation, startup recovery,
and diagnostic repair use the existing asset pipeline. An older home-icon table
receives the missing Logs entry while retaining healthy custom entries.

Focused tests cover core storage, Linux parsing/cancellation/fallback, permission
isolation, CLI/export, Logs rendering, and asset restoration. CI runs Linux source
tests on Ubuntu/Fedora and UX plus Unsupported-source tests on Windows/macOS.
A successful macOS run does not stand in for a native journal/dmesg integration
run on Linux.
