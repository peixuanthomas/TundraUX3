# Linux administration acceptance record

Date: 2026-10-16. The development host was Windows, with Linux checks run in local Ubuntu WSL. Tests used real system tools; cases requiring system-file writes, service startup, or rule changes ran in isolated mount, network, or systemd environments.

| Check | Result and scope |
| --- | --- |
| Windows `cargo check --workspace --locked` | Passed; platform conditions still prevent Linux operations |
| WSL `cargo test --workspace --locked` | Final rerun passed with `CARGO_BUILD_JOBS=2`; 359 platform unit tests passed and 19 were skipped by default; real-tool cases ran separately |
| Configuration backups | 8 tests passed: grouped originals, file attributes, protected content, version conflicts, originally missing files, undoing restores, and native-task diffs |
| Account recovery | 5 unit tests and 1 real isolated test passed, covering password dates, recreation under the same name, ID conflicts, preservation of home directories, and unrelated attributes |
| Firewall | 20 rule tests and 2 listening-port tests passed; real tests with private UFW, firewalld, and offline startup recovery passed, with host nftables unchanged before and after |
| SSH | 12 ordinary tests and real-connection, SSH/UFW integration, and private systemd service/socket tests passed; rejected confirmation over old or reused connections, checked the target port and independent rollback |
| Scheduled tasks | 9 unit tests and real private-systemd/crontab tests passed; verified execution identity, success and failure results, pause, resume, and undoing restores |
| Network backup integration | 31 ordinary tests passed; checked separate records for changes and rollback, saved versus temporary configuration, and version checks on retries after failure |
| Internationalization | 27 language tests and a real authorization-helper test passing custom Chinese text passed; checked 2989 calls and 3316 Chinese/English resource pairs, preserving native data identifiers |
| Shared forms and screen keyboard | 521 Shell tests passed, including shared keyboard, mouse, touch-event paths without hover, language switching on cached pages, and authorization-list refresh |
| Real PTY | Final rerun passed; isolated XDG directories and 140×40 and 108×20 terminals covered management searches and forms, CLI, mouse, input priority, and terminal restoration. After 64 queued mouse events, input was accepted immediately and the character appeared within 0.008 seconds |

Default parallel compilation of the full workspace previously exited because WSL ran out of memory. Subsequent runs limited build concurrency with `CARGO_BUILD_JOBS=2` without reducing test cases. The full test suite also found that English resources for new entry points, old help assertions, and Launcher entry counts needed updating; these were fixed. PTY checks determine form closure from the final screen and do not require the terminal to re-emit unchanged search labels.

`cargo fmt --all -- --check` reports existing formatting differences in `third_party/lsh`. No third-party code was changed in this round; new and modified Rust files were formatted and checked individually.

Real system tools were extracted into test directories only from official Ubuntu packages; no firewall/SSH services were installed, enabled, or switched on the development machine. Account tests used temporary `/etc` and other directories; firewall scripts used private networking and DBus; scheduled-task and SSH scripts started isolated systemd instances containing only test units and cleaned up their dedicated cgroups afterward. Temporary directories containing the extracted official tools were removed.

Under isolated systemd, SSH tests separately verified ordinary services and `Accept=no` sockets, checking cancellation and timeout recovery for both listening modes. The tests actually ran `systemctl` and production reload/recovery code, checking that ordinary service processes were preserved, socket-associated services restarted, old connections could still execute commands, new connections to the new target port could confirm, and the old port was restored. Additional `systemd-socket-activate` tests covered inherited listeners and real connections under different accounts. A separate root subprocess handled timeout recovery; tests shortened the recovery deadline, and startup recovery was verified by changing the boot ID in test records without actually rebooting the development machine.

Physical touchscreen acceptance, whole-device power-loss/reboot acceptance, desktop sudo authorization-dialog acceptance, and real NetworkManager/Netplan/networkd writes and wireless-hardware acceptance have not been performed. Touch results refer only to tap events in automated tests and do not establish physical-device acceptance.

For repeat validation, see [configuration backups](../crates/platform/docs/config-backups.md), [firewall](../crates/platform/docs/firewall.md), [SSH](../crates/platform/docs/ssh-management.md), [scheduled tasks](../crates/platform/docs/scheduled-tasks.md), and the [testing guide](testing.md).
