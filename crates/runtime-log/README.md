# runtime-log

`runtime-log` stores and queries structured runtime events, handling redaction, rotation, retention, capacity limits, and safe cleanup. Shell's Logs page and the CLI share these data; `platform` provides Linux journal and selected-file reading.

## Modules

| Module | Responsibility |
| --- | --- |
| `model` | Event, level, query, and status types |
| `writer` | Bounded queue, deduplication, flushing, rotation, and capacity management |
| `reader` | Queries, filtering, export, and bounded reads |
| `privacy` | Removal of sensitive data and control sequences |
| `clear` | Preview and cleanup of managed log files |

The UX failure phase `Incident` is consistently mapped to the highest level, `Critical`, during queries. Original files remain unchanged, and recovery results continue to use their original levels.

Log bodies use a fixed language and stable event codes, rather than being generated from editable translation files. Passwords, tokens, clipboard contents, file bodies, and command contents are not used as log context.

## Documentation and validation

The [log guide](docs/logs.md) describes sources, permissions, TUI, CLI, following, export, cleanup, and disk quotas. Incident identities and task IDs are managed by [watchdog](../watchdog/README.md).

```sh
cargo test --locked -p runtime-log
```

After changing pages or commands, also run the affected Shell/CLI tests. Validate Linux journal/dmesg sources in a Linux environment.
