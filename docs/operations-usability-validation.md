# Operations usability acceptance, 2026-10-26 to 27

This round targeted users unfamiliar with the command line, filling gaps in visible multi-selection, built-in text input, the task page, the script interface, scheduled maintenance, and configuration backup management. Network extensions, business-data backups, remote-host lists, and container management were outside this round's scope.

The environment was local Windows and WSL Ubuntu. All Linux checks ran in WSL; system modifications were limited to private systemd test environments, temporary files, or stub tools. Development-machine packages were not upgraded.

## Feature checks

| Scope | Automated checks | Result |
| --- | --- | --- |
| Multi-selection and screen input | Empty selections, disappearing targets, sorting/refresh, process-identity mismatch; keyboard, mouse, and press/release without hover; login, initial setup, accounts, editor, files, and dialog switching | Windows Shell: 606 passed; WSL Shell: 633 passed and 1 skipped under its existing conditions; WSL file management: 16 passed |
| Scripts | At most five actions per state, source/output defaults, reversed detail transitions, narrow windows, breakpoint stepping, and stopping | Windows/WSL UI: 11 passed on each platform; WSL controller: 19 passed; animated-tap regression passed again |
| Tasks | Reopening summaries and chunked output, duplicate IDs, cancellation cleanup, pending verification, write failures, and cleanup version checks | WSL history: 8 passed, AA: 8 passed, terminal: 35 passed; task page included in the full Shell suite |
| Scheduled tasks | Structured actions, time previews, preservation of complex rules, single-line cron edits, disabling and resuming | Included in the final WSL results of 283 platform-management tests and 633 Shell tests passing |
| Isolated systemd | Creation, execution records, time/catch-up edits, pause, resume, deletion and restoration; preservation of execution history and business files | Real-tool tests passed; units also saved execution results while Tundra was not running |
| Configuration backups | Pagination and global sorting, summary-index rebuild, legacy records, retention, restore contention, concurrent cleanup, and version changes | 17 targeted tests passed, including explicit scale measurements; final platform-management suite: 283 passed and 20 skipped by default, with two isolated systemd cases also explicitly run and passed |

Package-upgrade checks used stub tools to verify APT, DNF, and pacman arguments, unattended settings, and failure handling. Native cron limitations remain explicitly displayed, and a successful start request is not treated as successful execution.

Formatting checks with `cargo fmt --check`, Windows `cargo check --workspace --all-targets --locked`, and a total of 690 Windows UI/App/i18n tests passed; Shell/CLI builds completed on both Windows and WSL. The full WSL CLI suite had 133 passes and 4 skips under existing conditions, including the update-installation fixture of the same name. An additional full Windows CLI run had 103 passes and failed the update-installation test `update_staging_requires_complete_vim_and_uses_the_validated_new_helper`; a separate rerun still returned “The system cannot find the file specified” (`update_staging.rs:54`). It was not recorded as passing, and the update module was not changed to evade that result.

The real Linux PTY script smoke test passed execution, input, output scrollback and scrollbar dragging, debugging without hover, breakpoints and stepping, narrow-window switching, stopping, and Ctrl+C. The new default output area is taller, so the fixture was changed to produce enough output before checking scrollback; the call stack was checked after expanding it through its visible label. The explicit `--skip-vim` option was used because the local target directory lacked the matching Vim engine; real Vim handoff was outside this passing result's scope. All 11 workflows in `shell-control.py` passed, including Shift multi-selection, suspending/resuming two temporary processes, modal interception, Unicode editing, resizing, and embedded-terminal input.

## Backup scale measurements

Temporary legacy records were created under a WSL debug build, with 32 KiB of body content per record and 50 records per page. The first read includes building the summary index; subsequent listings neither read bodies nor compute every diff.

| Records | Initial index rebuild | First batch read using the index |
| --- | --- | --- |
| 100 | 116.7 ms | 1.94 ms |
| 1000 | 951.5 ms | 12.51 ms |
| 10000 | 9.43 s | 123.55 ms |

New records maintain the index from their first write; building the index for legacy records still requires reading historical files once. The cleanup request for 10,000 records is under 4 KiB and sends only a fixed summary of the confirmed scope. Cleanup rechecks scope and file versions; any changed candidate rejects the old confirmation. Temporary records have been deleted.

Rerun with: `cargo test --locked -p platform scale_measurement_100_1000_10000 --lib -- --ignored --nocapture`.

## Input methods and unverified items

| Input or environment | Coverage in this round |
| --- | --- |
| Keyboard only | Automated tests cover focus, input, correction, submission, and return; Linux PTY verified execution, debugging, editing, and terminal input. No person performed page-by-page acceptance using a physical keyboard |
| Mouse only | Automated tests send movement, press, and release; real PTY verified wheel scrolling and dragging. No person has performed page-by-page acceptance using a real mouse |
| Taps without hover | Automated tests directly send press and release; real PTY verified debugging and stop controls. No real touchscreen acceptance yet |
| Native Windows/macOS terminals | Windows builds and fixed tests; Windows ConPTY, macOS runtime, and all terminal combinations were not verified |
| Real reboot and authorization dialogs | Isolated systemd verified catch-up execution and recording while Tundra was closed; no manual acceptance of physical-device reboot or real sudo dialogs |

Task history is cleaned manually by source. Records belonging to other execution accounts remain read-only; system archives and personal copies are confirmed and deleted separately, without automatic linked cleanup. Automatic approval review rejected extending deletion across accounts and automatically deleting linked copies, so this round retained those limits.

Storage reopening, old process identities, and boot-marker mismatches were verified with fixed regressions; there was no actual whole-machine reboot or deliberately induced real PID reuse. Zero-context patches used for staged batches had caused some insertions to shift; separate corrective commits fixed them. Final production code matches the tested worktree. Automatic review rejected the history-rewriting `git reset --mixed master` operation, and it was not executed.

The user subsequently authorized merging `work` back into `master` and pushing to the remote after all implementation and checks were complete, then deleting local `work` after a successful push.
