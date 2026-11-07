# Application self-updates

The update workflow is in `app::update`. CLI runs the update helper, while Shell saves settings and schedules exit and restart. Linux updates treat the portable directory as the installation unit, separately from system package management.

## Update modes

On Linux, **Settings → Update → Update mode** is saved as `linux_update_mode` in `config.toml`, with `release` as the default.

- **Release:** reads the latest stable GitHub Release, downloads the Linux x86_64 portable archive, verifies SHA-256, and extracts Shell, CLI, and the complete Vim. Rust and Python are not required.
- **Beta:** reads the latest `master` commit and displays its hash, downloads the corresponding source, builds it with local Rust, and runs `scripts/build-vim.py` to build the pinned Vim. Python 3.12 and the corresponding C build tools are required.

Switching modes clears old check results and checks again. The current mode is retained during a download or build. All downloaded and built files go into the user's cache directory; the update process does not install toolchains or invoke sudo.

Windows builds updates from the default branch's source and does not show the Linux mode switch. macOS currently has no application self-update entry.

## Replacement and rollback

The update helper retains the old programs and old `vim/`, then removes the backups after the new Shell reports successful startup. Replacement failure, startup failure, or timeout restores both the programs and Vim. If the old installation had no Vim, rollback removes the directory added by this update.

After an interrupted update, the next startup continues recovery using the transaction record in the installation directory. The Vim directory must contain the executable, complete runtime, license, and `UPSTREAM.json`.

The current protocol is v3, which requires the Vim transaction state field. v2 updaters cannot install v3 packages; upgrading from those versions requires fully extracting the new portable archive once. Existing v2 transactions can only be recovered, rolled back, or cleaned up.

Updates do not replace user data, default themes, custom themes, or local assets. Linux preserves executable permissions; the helper replaces the original process through exec and waits for the new Shell to exit. Windows uses backup copies to handle files still held by running processes and cleans up old backups on subsequent startups.

## Cancellation and cleanup

Checks, downloads, Git queries, builds, extraction, validation, and copying all respond to cancellation. Waiting for the network or a child process can also be interrupted; only process trees and directories created by the current task are cleaned up.

A prepared replacement transaction writes a `Cancelled` record before deleting the staging directory. If writing the marker fails, the directory is retained for retry. At startup, cancellation records trigger cleanup only, not replacement.

If successful preparation and cancellation occur at the same time, Shell rechecks cancellation before accepting the result. Once the update helper takes over, it continues with installation, startup confirmation, and rollback. Only one update runs at a time.

See [Update tests](update-tests.md) for test entry points.
