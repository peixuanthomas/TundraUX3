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
task, and incident. Text and JSONL output are available. Follow mode keeps
bounded memory; see the following and filtering section below. The standalone CLI relies on current OS identity and file access;
it does not accept a claimed administrator role or a TundraUX owner identity.

Exit codes: 0 success (including empty results), 1 failure or partial/truncated
results, 2 invalid arguments, 3 permission denied, 4 unsupported, 130 cancellation.
Diagnostic export creates a new private directory and refuses to overwrite an
existing target. Its manifest records source status, truncation, damaged records,
and writer health. Partial exports retain usable records and report omissions.
Exports are user-owned artifacts outside the managed runtime retention quota.

## Clearing logs

Use `debug clear-logs` in Command Line, or prefix it with `tundra-cli` in an
external terminal. No arguments shows help. A target without `--yes` previews
matching files and their sizes; `--yes` confirms permanent clearing.

```sh
tundra-cli debug clear-logs all
tundra-cli debug clear-logs all --yes
tundra-cli debug clear-logs incidents --yes
tundra-cli debug clear-logs --type incidents --yes
tundra-cli debug clear-logs runtime --yes
tundra-cli debug clear-logs snapshots --yes
tundra-cli debug clear-logs --file crashes/crash-EXAMPLE.json --yes
tundra-cli debug clear-logs --file runtime/runtime-EXAMPLE.jsonl --yes
```

| Target | Files in the configured Tundra logs directory |
| --- | --- |
| `all` (also `--all`) | All four categories below. |
| `runtime` | `runtime/runtime-*.jsonl` event segments. |
| `incidents` | JSON, TXT and LOG report files directly under `crashes/`. |
| `snapshots` | `runtime/snapshots/snapshot-*.json` and `*.jsonl` viewer snapshots. |
| `legacy` | Top-level `*.log` files. |
| `--file PATH` | One recognized log file; accepts a relative path or an absolute path inside the configured logs directory. |

Exactly one target is required; `--type TYPE` is an alias for a named category.
Selecting one incident JSON file does not delete its TXT sibling; `incidents`
clears both report formats. Clearing incident reports leaves the corresponding
runtime events intact. Clear `runtime` or `all` as well to remove those events.

Clearing uses current OS filesystem permissions, refuses directory traversal
and symlinks, and preserves log retention settings and locks. Inactive files are
removed; active runtime segments are emptied while holding the same reservation
lock used by writers. Writers resume at the actual end of the file so clearing
cannot leave zero-filled gaps in subsequent events. A locked file that cannot
be emptied is reported as a failure. Running processes can write queued events,
repeat summaries, and new events after clearing; this command does not stop
logging. The displayed file list describes this invocation, not future files.

Scope is the configured Tundra logs directory only. Temporary watchdog fallback
reports, user-owned exported bundles, Linux journal/dmesg, configuration,
accounts, and watchdog state/run markers are excluded. Clearing does not dismiss
in-memory notifications or regenerate an already open editor document; refresh
Logs to read the current files.

Output lists each removed or emptied file and reports failures individually.
Exit codes are 0 for complete (including no matching files), 1 when no selected
file can be cleared or output fails, 2 for invalid command arguments, and 3 for
partial failure. A missing explicitly selected file is an error.

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

## 日志跟随与集中排查 / Following and filtering logs

日志应用默认持续收集新日志并滚动到最新内容。手动上滚、选择旧日志或拖动滚动条会暂停自动滚动，后台仍继续收集。暂停时显示新增数量，点击“回到最新”恢复跟随。当前缓冲最多保留 1000 条，达到上限时显示截断提示。

常驻操作是刷新、打开、暂停滚动或回到最新，以及“更多操作”。服务、启动批次、单次运行和文件选择放在二级菜单的筛选表单中。服务、进程和配置任务可携带服务名称、system/user 范围、启动 ID、运行 ID 与开始时间打开日志，并能返回原页面。

“更多操作”或 F10 打开内容区中央的菜单，文字居中并与边框留出空隙。方向键、Tab 和 Home/End 选择可用操作，Enter 或空格执行；Esc、“关闭”或点击菜单外侧返回日志。菜单内的空白处不关闭菜单，小窗口下可用滚轮或滚动条查看其余选项。

指定文件只读取操作系统允许访问的绝对路径，逐次读取有界尾部。文件更换、截断、删除和权限变化都有状态提示。原始文件按行展示；未知的时间和级别不会伪装成解析出的真实记录，不能对原始文件使用事件时间、模块或级别筛选。Linux 拒绝把符号链接当成指定日志文件。

```sh
tundra-cli logs follow --unit ssh.service --scope system --boot 0 --json
tundra-cli logs query --unit worker.service --boot -1
tundra-cli logs follow --file /var/log/syslog --format jsonl
```

内建 Command Line 使用 `/logs follow …`。JSONL 每行一个事件，状态和截断提示写入标准错误。Ctrl+C 停止跟随，返回 130。文件轮转后跟随新文件；文件临时不存在时继续等待，恢复后继续读取。查询不会修改日志源。

自动测试覆盖有界缓冲、暂停时保留位置并统计新增内容、筛选表单、二级菜单点击区域、服务返回位置、文件截断与轮转，以及真实 CLI 子进程的追加、轮转、截断和 SIGINT。真实终端触屏、各主题实际颜色及操作系统日志权限仍需要对应环境验收。

The log application keeps collecting while scrolling is paused and displays the number of new records. “Latest” resumes following. Service filters support system/user scope, boot ID, invocation ID and time range. Selected text files use bounded tail reads and report rotation, truncation, disappearance or access changes. JSONL records go to stdout; progress and source notices go to stderr. Neither viewing nor following modifies the original log.
