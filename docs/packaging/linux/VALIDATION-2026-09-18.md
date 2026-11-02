# Linux editor crash and RPM update restart validation

Validation dates: 2026-09-17 to 2026-09-18 (Asia/Shanghai).
The initial branch was `master`, the worktree was clean, and the baseline commit was
`bda731b8645f5c22e983dfdac2c620e23e445fcf`.
The editor fix commit was `eca6c4ef5ec14c96d39563d0dbd59a29d66c748c`;
the RPM restart and normal-exit fix commit was `0da9c5da772888f25440cc3d6a3e6391c8f57062`.
No development branch was created, and nothing was pushed.

## Environment and evidence directory

- Local macOS, Rust/Cargo 1.97.1.
- `ssh x240s-test`: Fedora 43 KDE, x86_64, Rust/Cargo 1.98.0, ordinary-user UID 1001.
- Isolated test root:
  `/home/x240s-test/agent-tasks/editor-crash-bda731b-20260917`; all paths below are relative to this directory.
- `source/` holds the source under test; `target/` and `release-target/` hold separate build artifacts.
  The original server repository and build cache were not overwritten. Final `source/` was at the RPM fix commit above with a clean worktree;
  `tux3-final-source-manifest.json` verified matching SHA-256 values for all 613 tracked files.
- Before the update migration, the RPM database recorded `tundraux3-1.3.1-1.x86_64`, but Shell/CLI had been manually replaced
  and actually reported commit `8504a7a9962a6b5d291ba436fbdc62aaa8a5c369`; `rpm -V` reported binary mismatches.
- `installation-backup/installed-payload.tar` backed up the original binaries, resources, desktop entry, documentation, and PAM files;
  the same directory holds original RPM metadata, file lists, dependencies, binary hashes, and build identities. Original user data was not rewritten.

## Editor root cause and regressions

The original evidence was in the root user's state directory:
`crash-20260917T154621.855Z-run-5068-1789659960889098190-7.txt`,
with panic `byte_slice(): Invalid byte range 109..0: start must be <= end`.
The call chain passed through `SourceBuffer::viewport_line`, editor view conversion, and mouse-pointer handling.

When horizontally scrolling beyond a short line in a non-ASCII document, no grapheme entered the visible range. The start fell back to the end of the line,
but the end remained at the beginning, ultimately passing an inverted byte range to Rope. The fix returns a valid empty range for invisible short lines
while maintaining the corresponding column coordinates.

- The original code reproduced the exact `109..0` panic both locally and on Fedora.
- Fedora GDB captured `line={start:0,end:109}`, `left_column=108`,
  `visible_start=109`, and `visible_end=0`; evidence is in `baseline-gdb.log`.
- New tests cover short Chinese lines, short ASCII lines when other lines contain Unicode, emoji/combining characters, empty lines,
  LF/CRLF/CR, scrolling to and beyond line ends, `usize::MAX`, and offsets for non-first lines.
- Shell regressions cover horizontal keyboard scrolling, Home restoration, and scrollbar mouse dragging, checking that disk contents remain unchanged.
- A real Linux PTY entered short Chinese lines and long lines, then performed horizontal scrolling and mouse operations; passed.
  Character response after 64 mouse events was 0.011 seconds (threshold 0.25 seconds), the terminal recovered after SIGTERM,
  and there were no watchdog incidents. Evidence: `update-editor-pty-assets.log`.

## Issues found and fixed during the update

An RPM update unlinks the old executable, after which Linux `current_exe()` returns a path ending in
` (deleted)`. The old implementation tried to execute that path after Restart was clicked, returned ENOENT, and exited.
The original executable path is now saved at startup; after updating, the new file is exec'd through the same path, preserving the foreground process group.
The Linux regression actually replaces a running test program by renaming, then verifies successful exec.

Another existing issue was the logind listener thread blocking while waiting for signals and failing to respond to watchdog shutdown.
The existing managed-thread API gained a cooperative cancellation token; logind connection, subscription, idle waits, and reconnection delays are all cancelable.
Unit tests cover cancellation through the handle, task group, and entire process; terminal tests add incident checks after exit.

The isolated Fedora rootless Podman environment used real PackageKit, polkit, pkttyagent, and `/bin/login`;
test RPMs used temporary signing keys inside the container. Host repositories and trusted keys were not changed.

| Test | Result and evidence |
| --- | --- |
| Authentication, installation, restart | `verified-update-success.log`: real authentication completed `1.0.0-1 → 2.0.0-1`; clicking Restart returned to the new Home. The running process's device number/inode matched the installed file; normal exit with no watchdog incidents. |
| RPM transaction scope | `verified-update-rpm.txt`: Tundra 2.0.0-1, required dependency tundra-runtime 1.0.0-1, unrelated tundra-unrelated retained at 1.0.0-1. |
| Authentication cancellation | `verified-update-cancel.log`: Ctrl-C at the real system password prompt retained Tundra 1.0.0, restored the terminal, and produced no watchdog incidents. |
| User data and credentials | The update script checked that isolated state files belonged to the ordinary test user; only fixture credentials were used, and they were verified absent from Shell output and state logs. |

The success container was `tundra-editor-update-verified-20260918`; the cancellation container was
`tundra-editor-update-cancel-fixed-20260918`. Validation used a native debug binary containing the final Rust fixes,
stripped and installed in a signed test RPM. Its build identity still reflected the pre-commit worktree, so it was recorded separately from the final
release RPM's build identity. `scripts/tests/authorization-pty.py` now contains complete restart checks;
an installation message or leftover Home text in the terminal alone cannot establish successful restart.

## Real-host migration and post-installation validation

The user explicitly chose to retain the RPM installation and validate the PackageKit workflow. A release build through
`scripts/package-linux.sh --rpm` generated
`dist/tundraux3-1.3.1-1.x86_64.rpm`, with SHA-256
`d35f4999f1056558556831765fb6ef91307c7234b1583bd939117dc65acb7c9c`.
This was a locally built, unsigned RPM, not published to a remote repository; the isolated update fixtures above used signed RPMs.

First, DNF `--assumeno reinstall` checked the transaction and confirmed that Tundra was the only package; `dnf -y reinstall` then
performed the migration. The project package version remained 1.3.1-1, hence reinstallation; the actual code version was confirmed by the build commit.

- `host-rpm-install.log`: transaction succeeded, reinstalling only Tundra.
- `host-rpm-verify.log`: `rpm -V tundraux3` returned 0 with no output; both Shell and CLI were managed by that RPM.
- `host-shell-build.txt`, `host-cli-build.txt`: version 1.3.1, protocol 2, commit
  `0da9c5da772888f25440cc3d6a3e6391c8f57062`, reporting `dirty=false` and `state=clean` respectively.
- `host-installed-smoke.log`: the **unmodified** PTY smoke test passed with its default startup wait and 250-millisecond keyboard threshold;
  character response after 64 mouse events was 0.001 seconds, terminal restoration was complete, and normal exit produced no watchdog incidents.
- `host-installed-editor-pty.log`: the installed release binary passed short-Chinese-line, long-line horizontal scrolling, and mouse regressions;
  keyboard response was 0.001 seconds, with no watchdog incidents on exit.
- `host-installed-update-page.log`: entered Settings → Update through a real PTY, identified the installation as `SystemRpm`,
  and showed that the configured repositories offered no newer Tundra package; `tux3-host-update-pty.frame.txt` preserves the final page.
  This check exited normally and restored the terminal without watchdog incidents. No Tundra release repository was configured on the host,
  so this normal no-candidate result cannot be described as a successful update from a real release repository.

## Final Fedora workspace checks

- Native `cargo check --workspace --locked` passed; evidence: `update-check.log`.
- The final commit's complete `cargo test --workspace --locked -- --test-threads=1` run passed:
  **86 test targets, 1459 passed, 0 failed, 2 already ignored**. The ignored cases were existing native Trash round-trip tests;
  their implementation was not changed and these ignored cases were not run in this round.
- The full log is `final-tests-linux.log`, and `final-tests-linux.exit` is `0`;
  it includes new regressions for Unicode viewports, Shell horizontal scrolling, restart after replacement of a running program, and three thread-cancellation entry points.
- The server used `CARGO_BUILD_JOBS=1` to limit build resources. Early concurrent linking caused high disk load;
  old builds belonging only to this validation were stopped. The final full run used non-PTY SSH standard input
  to avoid CLI doctor tests being suspended by SIGTTIN in background interactive jobs. Those interrupted runs were not counted as passing.
- Final source remained at clean `0da9c5d`; this documentation commit does not change the tested code or installed RPM's build identity.

## Validation scope and retained failure records

- Graphical authentication and real power operations were outside this round's scope; container polkit service-isolation overrides were test-only
  and did not change host services. No host reboot, shutdown, or network modification occurred.
- Early default-concurrency macOS tests had four existing system-services loopback HTTP wait timeouts;
  a full serial run after the editor fix passed 1398 tests, with 0 failures and 2 already ignored.
  The first serial run after the RPM fix again encountered a 5-second request wait timeout in `active_metrics_preserve_pending_time_sync_request`,
  recorded in `/tmp/tux3-update-tests-macos.log`; that application code and test were not changed.
  A subsequent run with `--no-fail-fast -- --test-threads=1` completed all 86 targets: 1398 passed, 1 failed,
  and 2 already ignored. The failing case was a background-task wait timeout in another unchanged test, `failed_time_server_validation_shows_error_and_does_not_save`,
  recorded in `/tmp/tux3-update-tests-macos-retry.log`. This full run cannot be reported as having zero failures.
  The `shell --test settings` target was then rerun separately in serial, with all 16 tests passing; see
  `/tmp/tux3-final-settings-macos.log`. Local `cargo fmt --check`,
  `cargo check --workspace --locked`, and diff checks passed.
- Fedora had no rustfmt installed; formatting checks ran locally.
- An early standard PTY smoke test of a debug build exceeded the startup wait limit; a temporary editor test relaxed only startup waiting to
  45 seconds, retaining the 250-millisecond keyboard threshold. Another test used an old artifact whose linking had not completed; validation was subsequently repeated with the completed
  build. A separately stripped binary initially read old language resources from the host and displayed a resource-compatibility prompt; validation passed after copying matching
  resources. These failure logs were retained and were not counted as passes.
- No Tundra RPM repository was configured on the host. The new SystemRpm updater uses newer RPMs from configured repositories
  rather than building and installing GitHub commits. Future automatic updates require publishing a higher RPM EVR and providing a trusted repository.
  Complete update success in an isolated environment does not establish availability of a real release repository.
