# Linux update tests

The current update system targets portable directories. Run Linux tests as a normal user; use WSL on a Windows development machine:

```sh
cargo test --workspace --locked
```

Main test locations:

| File | Coverage |
| --- | --- |
| `crates/app/tests/unit/update_release.rs` | Stable release selection, checksums, corrupt contents, and portable archive extraction |
| `crates/app/tests/unit/update.rs` | Preparation, replacement, interrupted-update recovery, and rollback state |
| `crates/app/tests/unit/update_cancellation.rs` | Cancellation, cleanup, and races |
| `crates/app/tests/unit/vim_install.rs` | Vim package validation, cancellation, and file safety |
| `crates/cli/tests/update_linux.rs` | Real update helper, replacement, rollback after startup failure, and foreground waiting |
| `crates/shell/tests/settings.rs` | Keyboard and mouse operation of update modes and persistence |

The fixed test suite uses in-memory data and temporary portable directories, without making HTTP requests or starting local HTTP servers. Full Release downloads, Beta builds, and real restarts are checked during release acceptance on the corresponding platforms.

Historical system-package validation records are in `docs/packaging/linux/VALIDATION-*.md`. For current update behavior, see [Application self-updates](update.md).
