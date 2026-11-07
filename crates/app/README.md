# app

`app` stores application state and handles domain commands without depending on UI, Ratatui, or crossterm. Shell/UI handles window dimensions, keys, focus, and click positions; platform reads and writes go through `platform` and `storage`.

## Modules

| Module | Responsibilities |
| --- | --- |
| `application/` | Global state, application catalog, notifications, commands, and read-only snapshots |
| `editor/` | Text buffers, multiple documents, undo, saving, recovery, and Vim mirrors |
| `script/` | Script interpreters, arguments, breakpoints, and per-document execution state |
| `explorer/` | File lists, selection, operation tasks, and recovery |
| `launcher/` | Built-in entries, custom commands, external programs, and launch workflows |
| `diagnostics/`, `runtime_logs/` | Diagnostics, log queries, and merging failure reports |
| `system_settings` | System settings backend interfaces and availability state |
| `clock.rs` | Alarm and timer scheduling |
| `update/` | Update checks, downloads, builds, Vim installation, replacement, and rollback |

Callers modify state through `AppCommand` and read a consistent view through `AppSnapshot`. `AppAction` only expresses requests to redraw, exit, reboot, or power off; Shell performs terminal restoration and process operations.

## Main state and workflows

### Explorer and Launcher

Explorer manages directories, filtering, sorting, multiple selection, the clipboard, and operation progress. `ExplorerTaskEngine` provides cancellation, checkpoints, and recovery for lengthy file tasks; the platform layer performs the actual copy, move, trash, and directory operations.

`ExplorerFileService` shares copied and cut files across pages in the current session and clears them when switching accounts. It records completed moves and renames within one filesystem for cross-page undo. Undo restores the location while preserving later content edits, checks the object and both parent-directory identities, and rejects replaced objects or an occupied original path without recursively scanning directories. Cross-volume moves, replacements, and directory merges cannot be undone and clear older undo records. The system file clipboard is not connected; the text clipboard does not carry file transfers.

The shared application catalog supplies Home and Launcher. System applications shown on Home are excluded from Launcher, which retains the editors, Command Line, custom Shell commands, and approved external programs. Custom entries support a name, a 5×2 terminal-cell text icon, arguments, a working directory, and retained output after exit; built-in entries are not stored in removable configuration. Custom commands use `/bin/sh -c` on Unix and `cmd.exe /C` on Windows.

### Editor and scripts

The editor handles user-visible characters as graphemes, using save snapshots tied to an exact revision and separate recovery records. Bytes written externally by Vim enter the same atomic save workflow through `SaveSnapshot::external_bytes`, without re-encoding or changing line endings.

Script state stores the interpreter, arguments, breakpoints, run ID, and pause ID for each document. Editing, undo, and redo move breakpoints; results from an old run or pause location do not overwrite current data.

### Updates and logs

Updates prepare Shell, CLI, and the complete `vim/` together, validate assets, licenses, and upstream metadata, and roll them all back on failure or startup timeout. Vim installation packages are checked for SHA-256, directory contents, and the pinned version; Windows also checks the iconv DLL, license, and source code.

Source updates compile Shell and CLI, reusing a complete installed Vim when its upstream metadata matches the target source's pin. Vim is rebuilt only when that pin changes or no complete matching engine exists. Cargo and Vim build output is saved to a private `update-*.log` in the application log directory. The file remains after build failure or cancellation, and build errors include its path.

Runtime logs merge failure reports that the user is authorized to access at the highest severity, `Critical`, sort them by time, and respect report access restrictions.

## Documentation

- [Application state and built-in applications](docs/applications.md)
- [Application self-updates](docs/update.md)
- [Update tests](docs/update-tests.md)
- [Editor and script workspace](../../docs/source-editor.md)
- [Saving system configuration](../platform/docs/system-config.md)

## Validation

```sh
cargo test --locked -p app
```

Run workspace tests after changing cross-crate commands or shared types. Update and Vim installation tests use offline data and cover validation, cancellation, file safety, and rollback.
