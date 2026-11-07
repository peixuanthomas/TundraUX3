# Application self-updates

The update workflow is in `app::update`. CLI runs the update helper, while Shell saves settings and schedules exit and restart. Linux updates treat the portable directory as the installation unit, separately from system package management.

## Update modes

On Linux, **Settings → Update → Update mode** is saved as `linux_update_mode` in `config.toml`, with `release` as the default.

- **Release:** reads the latest stable GitHub Release, downloads the Linux x86_64 portable archive, verifies SHA-256, and extracts Shell, CLI, and the complete Vim. Rust and Python are not required.
- **Beta:** reads the latest `master` commit and displays its hash, downloads the corresponding source, and builds Shell and CLI with local Rust. A complete existing Vim is reused when its pin is unchanged; otherwise, `scripts/build-vim.py` builds the target Vim, requiring Python 3.12 and the corresponding C build tools.

Switching modes clears old check results and checks again. The current mode is retained during a download or build. All downloaded and built files go into the user's cache directory; the update process does not install toolchains or invoke sudo.

Windows builds updates from the default branch's source and does not show the Linux mode switch. macOS currently has no application self-update entry.

Source updates compare the target `packaging/vim.json` with `UPSTREAM.json` in the Vim directory beside the running program, then the application data directory. Matching compares the complete parsed configuration, including the upstream commit, checksum, required features, and Windows iconv dependency. A valid match copies the executable, runtime, licenses, and metadata into the prepared update without launching Python or compiling Vim. Missing or incomplete engines are rebuilt. Copying preserves executable permissions and responds to cancellation; installation and rollback use the same complete Vim payload as before.

Release shows release notes and download progress without compilation or live-output panels. Beta and Windows source updates also show compilation progress and live output.

## Build logs

Each source update writes Cargo and Vim stdout and stderr to a separate `update-*.log` in the application log directory, before the UI applies its 200-line tail and line-length limits. Build progress output and long diagnostic lines are retained. The file includes the target commit, build stages, and exit results, and remains after failure or cancellation even when temporary build files are removed.

Build failures include the log path. Open **Logs → Open folder** to find the file and view it in the editor. On Linux the directory is `$XDG_STATE_HOME/TundraUX3/logs`, or `~/.local/state/TundraUX3/logs` when `XDG_STATE_HOME` is unset. These separate text logs can be removed with the existing `tundra-cli debug clear-logs legacy --yes` command; runtime JSONL rotation does not manage them.

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
