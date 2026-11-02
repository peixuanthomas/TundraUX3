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

`StorageManager` and `StorageLayout` manage document locations and loading, `StorageDescriptor` describes formats, and `StorageLoadReport` returns migration and recovery results.

## Safe saving and recovery

Documents are read up to a default limit of 1 GiB. Length, modification time, and path identity are checked before and after reading. Each path component rejects symbolic links, junctions, and reparse points; conditional writes use document fingerprints to check for external modifications.

Writing proceeds through a temporary file in the same directory, file synchronization, atomic replacement, and parent-directory synchronization. Linux uses `openat` and `O_NOFOLLOW` to prevent ancestor-directory redirection. Application directories use `0700`, and private documents use `0600`.

Future schemas are rejected outright. If a current or older format cannot be parsed, the original is renamed to `<filename>.corrupt.<timestamp>`, then a default document is created and Shell shows a notice. Old `users.v1.json` files migrate to `users.v2.json`.

## Configuration notes

- `[automation].enabled` stores external CLI control, defaulting to `false`.
- `screen_timeout_seconds` stores the idle-weather timeout; `0` means never, and the default is 300 seconds.
- `[editor].vim_mode_enabled` stores the Vim toggle, disabled by default.
- `[editor].explorer_open_extensions` controls the suffixes handed to the built-in editor; custom lists and empty lists take precedence.
- `animation_timings` stores separate acceleration, constant-speed, and deceleration durations for page/dialog/menu animations, each from 1 to 2000ms.
- Launcher's `order` stores a unified ID order, `custom` stores names, icons, commands, and exit options, and `terminal` stores argument arrays and working directories.

`reset_saved_content` clears saved data. The caller is responsible for backup, confirmation, and restart workflows.

## Validation

```sh
cargo test --locked -p storage
```

Tests cover schemas, migration, atomic writes, path safety, corruption recovery, and configuration compatibility.
