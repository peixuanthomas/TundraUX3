# Logs and diagnostics

Logs is a standalone application that brings together UX runtime events, incident reports, Linux logs, and selected log files. System Status retains system metrics and health checks.

## Sources and permissions

| Source | Content and access |
| --- | --- |
| UX log | Structured runtime events and incident reports; incidents appear at the highest level, Critical. Ordinary users see only their own records; full incident reports require an administrator |
| Linux log | Read the journal on demand, falling back to dmesg when unavailable. The TUI requires an administrator, and operating-system permissions still apply |
| Selected file | Read the absolute path chosen by the user, using current operating-system permissions and bounded tail reads |

UX supports Linux, Windows, and macOS. Guest has no Logs entry. The Linux source displays Unsupported on Windows/macOS.

Sources and levels are separate: Explorer errors belong to `ux.explorer`, kernel messages to `linux.kernel`, and service messages to `linux.service`. Journal priorities map to log levels; original source and timestamp notes are retained.

Permission failures, partial output, cancellation, and unavailable sources are shown separately rather than treated as empty results. Linux records are not assigned invented UX run/task IDs.

## CLI queries and export

```sh
tundra-cli logs query
tundra-cli logs query --source ux --module ux.explorer --level error --format jsonl
tundra-cli logs query --since 2026-09-01T00:00:00Z --until 2026-09-02T00:00:00Z
tundra-cli logs query --run-id RUN_ID --task-id TASK_ID --limit 500
tundra-cli logs query --source linux --level warning
tundra-cli logs incidents --incident-id INCIDENT_ID
tundra-cli logs export --run-id RUN_ID --output ./diagnostics-new
```

Queries return the latest 200 records by default, up to 10,000. Filters include source, UTC time, minimum level, module, run, operation, task, and incident. Text and JSONL output are supported.

The standalone CLI uses the current operating-system identity and file permissions. It does not accept caller-declared Tundra roles or owners.

Exit codes: 0 for success (including empty results), 1 for failure or truncated results, 2 for invalid arguments, 3 for permission denied, 4 for unsupported operations, and 130 for cancellation.

The export directory must not already exist. The program creates a private directory and writes a manifest recording source status, truncation, damaged records, and writer health. Partial exports retain available records and report omissions. Exported files do not count toward managed-log quotas.

## TUI controls

Open Logs from Home. All levels are shown by default.

| Input | Behavior |
| --- | --- |
| Left / Right | Switch between UX / Linux sources |
| L | Cycle the minimum level; Warning includes Error and Critical |
| M / T / C | Filter by module / filter by time / clear filters |
| R or F5 | Refresh |
| Enter or O | Open a redacted query document in read-only Editor |
| I / E | Open the full incident report / view related events |
| G / Open folder | Open the UX log root directory in the built-in Explorer |
| Esc | Return while retaining filters and selection |

Use arrow keys, PageUp/PageDown, Home/End, the mouse, and scrollbars to browse. Tab does not switch log sources.

Opening a file generates a new snapshot filtered for the current user; it does not bypass owner checks. Full incident reports use an allowlist and redacted copies, without directly exposing original crash-file paths.

## Following and filtering logs

The Logs application continuously collects new records. Manually scrolling up, selecting an older record, or dragging the scrollbar pauses automatic following; collection continues in the background and shows the number of new records. Back to latest resumes following. The current buffer retains up to 1,000 records.

Service, boot, individual run, and file selection are in the filter form. Service, process, and configuration tasks can open logs with a service name, system/user scope, boot ID, run ID, and time, then return to the original page.

```sh
tundra-cli logs follow --unit ssh.service --scope system --boot 0 --json
tundra-cli logs query --unit worker.service --boot -1
tundra-cli logs follow --file /var/log/syslog --format jsonl
```

Use `/logs follow …` in the built-in Command Line. JSONL emits one event per line; status and truncation notices go to standard error. Ctrl+C stops following and returns 130.

Selected files use bounded tail reads and report replacement, truncation, deletion, and permission changes. Following continues with the new file after rotation and keeps waiting when the file is temporarily absent. Unknown timestamps and levels in original files remain unknown. Neither queries nor following modify the log source.

## Clearing logs

Use `/debug clear-logs` or external `tundra-cli debug clear-logs`. Without `--yes`, the command only previews files and sizes; `--yes` confirms cleanup.

```sh
tundra-cli debug clear-logs all
tundra-cli debug clear-logs incidents --yes
tundra-cli debug clear-logs runtime --yes
tundra-cli debug clear-logs snapshots --yes
tundra-cli debug clear-logs legacy --yes
tundra-cli debug clear-logs --file crashes/crash-EXAMPLE.json --yes
```

| Target | File scope |
| --- | --- |
| `all` / `--all` | All categories below |
| `runtime` | `runtime/runtime-*.jsonl` |
| `incidents` | JSON, TXT, and LOG reports under `crashes/` |
| `snapshots` | `runtime/snapshots/snapshot-*.json` and `*.jsonl` |
| `legacy` | `*.log` in the log root directory |
| `--file PATH` | One supported file within the log root directory |

Select only one target at a time; `--type TYPE` is an alias for category selection. Clearing a single incident JSON file does not delete its TXT copy, while category cleanup handles both formats. Corresponding UX events require selecting `runtime` or `all` separately.

Cleanup uses only current operating-system permissions and rejects path traversal and symbolic links. The active runtime segment is emptied under the same lock as the writer, which resumes writing at the actual end of the file. Running processes may continue to produce new events after cleanup.

Export directories, Linux journal/dmesg, configuration, accounts, watchdog state, and run markers are outside the cleanup scope.

Cleanup exit codes: 0 for complete success or no matches, 1 for no files eligible for cleanup or output failure, 2 for invalid arguments, and 3 for partial failure.

## Storage and quotas

Runtime files reside in `runtime/` under the platform log directory. Events carry a schema, UTC time, event ID, process/run/app/module/operation/task IDs, owner, phase, level, and optional incident ID.

Default configuration:

```toml
[runtime_logs]
max_age_days = 30
max_total_mib = 200
segment_mib = 10
```

Segments rotate by UTC date or configured size. Retention cleanup uses a cross-process lock and does not delete segments locked by other active writers. Configuration is read at process startup.

Editor snapshots share the total capacity limit and also have a 20 MiB limit, a 30-minute TTL, and a 5 MiB limit per document. watchdog crash reports use a separate retention policy.

The writer uses a bounded queue of 4,096 events, non-blocking submission, one-second flushes, and at most a two-second shutdown wait. Queue saturation and write failures are reported through health counters, without recursively writing logs. Repeated warnings are merged by identity, with count summaries generated every 60 seconds.

## Validation

```sh
cargo test --locked -p runtime-log
cargo test --locked -p cli --test logs
cargo test --locked -p shell --lib logs
```

Tests cover core storage, Linux parsing and fallback, permission isolation, CLI/export, TUI, file rotation, and cleanup. Linux sources are validated on Ubuntu/Fedora; UX and Unsupported sources are validated on Windows/macOS.
