# Application state and built-in applications

## AppState

`app::AppState` stores the clock, exit state, notifications, authentication session, user, configuration, appearance, and optional Explorer, Launcher, Diagnostics, and Editor state.

```rust
AppState::dispatch_at(AppCommand, Instant) -> AppAction
AppState::snapshot() -> AppSnapshot<'_>
```

Passing the monotonic clock `Instant` explicitly makes notification timeouts and login lockouts deterministic to test. `AppSnapshot` provides a borrowed read-only view; state changes go through new `AppCommand` values. `AppAction` only describes `Redraw`, `Exit`, `Reboot`, or `PowerOff`; Shell performs the actual terminal and system operations.

Notifications include 4-second toasts, keyed alerts, FIFO modals, and critical modals that can preempt others. The response queue has bounded capacity.

## Explorer

Explorer maintains filtering, sorting, multiple selection, history, the clipboard, drag and drop, conflict handling, and progress. The platform layer performs directory enumeration, copying, moving, renaming, opening, and system trash operations.

`ExplorerTaskEngine` provides a single worker, cancellation, staging, journals/checkpoints, and crash recovery for copy, move, and trash operations. Some lightweight file and storage operations still run in the command apply path.

Common selection controls:

- Ctrl/Cmd+click or Space selects individual items; Shift+click or arrow keys selects a contiguous range.
- Ctrl/Cmd+A selects all, Ctrl/Cmd+I inverts the selection, and Ctrl/Cmd+Shift+A clears it.
- C/X/V copies, cuts, and pastes; D or Delete moves items to the trash.
- N/T creates a folder or text file, F2 renames, and Alt+Up goes to the parent directory.
- Right-click or Shift+F10 opens the menu; clicking outside it or pressing Esc closes it.

Letter shortcuts only apply while browsing the list. Path, search, and file-name fields accept normal text input. Batch operations only affect selected items in the currently visible list.

Ctrl/Cmd+L edits the path bar, which supports selection, pasting, and automatic path-separator conversion. Right-clicking empty space in a regular directory can open Command Line in that directory; the working directory is passed directly to the child process.

## Launcher

Launcher stores executable targets and a fixed order, with icon and list views. Records include an absolute, non-symlink target, type, approver, and approval time. Paths and execution approval are checked again before launch; scripts and installation packages retain a second confirmation.

Built-in applications are declared per platform in `BUILT_IN_LAUNCHER_APPLICATIONS` and are not written to the removable Launcher configuration. The package entry uses distribution tools on Linux, WinGet on Windows, and Homebrew on macOS.

For external programs, **Run settings (F4)** saves the choice of system launch or an embedded terminal, arguments, and working directory. Arguments are parsed into an array without variable expansion. Custom commands can return immediately after exit or retain output until Enter is pressed.

For page shortcuts, see [Shell sessions](../../shell/docs/session.md#common-shortcuts). For Command Line state, see [CLI commands](../../cli/docs/commands.md) and [terminal-runtime](../../terminal-runtime/README.md).

## Plain-text editor

The editor uses `Rope` and a grapheme cursor to read and write plain text directly. Markdown files are edited as source text, without generating rich text or previews.

- Opening and saving use background tasks. Save snapshots are tied to an exact revision, so older tasks cannot clear newer changes.
- Each document has its own recovery ID, process file lock, and throttled writes; drafts are claimed individually after an abnormal exit.
- The extensions Explorer opens in the editor by default are defined by `storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS`; custom lists and empty lists take precedence.
- LSH provides syntax highlighting, with selection styling taking precedence. Display choices do not affect file contents.
- Line-change markers compare the current text with the text from the most recent successful save, independently of Git.

For complete controls, see [Editor and script workspace](../../../docs/source-editor.md).

## Clock, system status, and diagnostics

Clock scheduling is in `app::clock` and is saved in `clock.v1.json`. Linux follows system time and timezone; demonstration time settings on Windows/macOS continue to use their respective configurations. See [time](../../time/README.md) for synchronization details.

System Status displays system metrics, processes, network, and storage. The dashboard supports adding, editing, and arranging widgets; the system overview shows detailed metrics separately.

`app::system_settings` defines `SystemSettingsBackend`, snapshots, requests, and operation state. The backend performs I/O through supervised tasks; Shell turns availability and failure reasons into interface state.

`DiagnosticsTaskRuntime` is managed by watchdog, and each page can scan independently. Repairs first show a preview and require user confirmation. After storage repair, the "restart required" state is latched, and the repaired storage is used after restart. Logs and incident reports are both viewed in the Logs application; see the [log guide](../../runtime-log/docs/logs.md).
