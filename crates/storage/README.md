# storage

`storage` reads and saves application documents, validates format versions, and provides atomic writes, migration, corruption recovery, and data reset. `platform` provides platform paths and low-level file operations.

## Main documents

| File | Format | Content |
| --- | --- | --- |
| `config.toml` | TOML, schema 1 | Language, timezone, weather, appearance, AA, automation, and application settings |
| `users.v2.json` | JSON, schema 2 | Windows/macOS local accounts and Linux personal preferences |
| `state.v1.json` | JSON, schema 1 | General application state |
| `recent-files.v1.json` | JSON, schema 1 | Recent files |
| `sessions.v1.json` | JSON, schema 1 | Recoverable session data |
| `clock.v1.json` | JSON, schema 1 | Clock, alarms, and timer items |
| `trash/trash.v1.json` | JSON, schema 1 | Application trash manifest |

`StorageManager` and `StorageLayout` manage document locations and loading, `StorageDescriptor` describes formats, and `StorageLoadReport` returns migration and recovery results. Shell and app workers install a `SessionConfigStore` for their run's configuration path; its shared services process serializes configuration writes and rejects stale revisions. Other paths and standalone tools retain ordinary local storage.

## Safe saving and recovery

Documents are read up to a default limit of 1 GiB. Length, modification time, and path identity are checked before and after reading. Each path component rejects symbolic links, junctions, and reparse points; conditional writes use document fingerprints to check for external modifications.

Writing proceeds through a temporary file in the same directory, file synchronization, atomic replacement, and parent-directory synchronization. Linux uses `openat` and `O_NOFOLLOW` to prevent ancestor-directory redirection. Application directories use `0700`, and private documents use `0600`.

Future schemas are rejected outright. If a current or older format cannot be parsed, the original is renamed to `<filename>.corrupt.<timestamp>`, then a default document is created and Shell shows a notice. Old `users.v1.json` files migrate to `users.v2.json`.

## Configuration notes

- `[automation].enabled` stores external CLI control, defaulting to `false`.
- `[appearance].icon_display_mode` defaults to `ascii` on Linux and `image` on other platforms. User profiles use the same default when the field is missing.
- `screen_timeout_seconds` stores the idle-weather timeout; `0` means never, and the default is 300 seconds.
- `[editor].vim_mode_enabled` stores the default for newly opened editor sessions, disabled by default. F8 changes only the current editor session.
- `[editor].explorer_open_extensions` controls the suffixes handed to the built-in editor; custom lists and empty lists take precedence.
- `animation_timings` stores separate acceleration, constant-speed, and deceleration durations for page/dialog/menu animations, each from 1 to 2000ms.
- Default page/dialog/menu phases are `[40, 80, 40]`, `[45, 90, 45]`, and `[30, 60, 30]` milliseconds. Existing explicit values and animation speed remain effective; reduced motion bypasses transitions.
- The default border is the v1.5.1 Glacier Night `#29434E`, and the accent is `#63D3E5`. Missing preferences use current defaults; explicit colors and the existing legacy-color migration remain supported.
- Launcher's `order` stores a unified ID order, `custom` stores names, icons, commands, and exit options, and `terminal` stores argument arrays and working directories.

`reset_saved_content` clears saved data. The caller is responsible for backup, confirmation, and restart workflows.

## Validation

```sh
cargo test --locked -p storage
```

Tests cover schemas, migration, atomic writes, path safety, corruption recovery, and configuration compatibility.
