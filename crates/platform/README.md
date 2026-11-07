# platform

`platform` provides operating-system query and read/write interfaces, including application paths, files, processes, terminal capabilities, system metrics, accounts, services, packages, networks, and disks. Shell and CLI perform operations through these interfaces rather than assembling privilege-elevation commands in page code.

## Module structure

| Module | Responsibility |
| --- | --- |
| `paths` | Platform-specific configuration, state, cache, log, and temporary directories |
| `document` | Regular-file checks, chunked reads, document fingerprints, and atomic writes |
| `file_operations` | File identity and atomic renames that never overwrite the destination |
| `platform` | Directories, volumes, trash, opening files, and platform capabilities |
| `process`, `managed_process` | Process arguments, output, cancellation, and child-process management |
| `terminal` | Host terminal capabilities, colors, and fullscreen sequences |
| `system_monitor` | CPU, memory, battery, network, and process metrics |
| `management` | Typed system-management queries, actions, and fixed backends |
| `command_session` | System command sessions and executable lookup |
| `shell_control` | Local Shell control connections and session discovery |
| `task_history` | History and output for pages, helpers, and scheduled tasks |
| `diagnostics` | `doctor` environment and path checks |

Platform-specific implementations are in `src/linux/`, `src/windows/`, and `src/macos/`. Document reads reject FIFOs, devices, symbolic links, and other non-regular files, and recheck file identity after opening.

Explorer moves, renames, and undo share `file_operations`: Linux uses `renameat2(RENAME_NOREPLACE)`, macOS uses `renamex_np(RENAME_EXCL)`, and Windows uses `MoveFileExW` without replacement. An existing destination preserves both files.

## Management interfaces

`ManagementQuery`, `ManagementSnapshot`, `ManagementAction`, and `ManagementCommand` describe queries, page data, available operations, and execution requests. Action IDs, field IDs, and target identities remain stable; display text uses the caller's language.

Current management types cover services, processes, packages, networks, disks, users, system configuration, configuration backups, firewalls, SSH, scheduled tasks, task history, file permissions, and operating-system settings.

- Queries use the current process's permissions; operations requiring additional permissions first go through AA and system authorization.
- Batch operations check all targets first. Services use one systemctl invocation, processes are bound to their original identity, and changes from the same cron source are saved together.
- APT, DNF, and pacman support batch package operations; pacman installation and upgrades include a full system upgrade.
- Homebrew is in `management/homebrew/` and distinguishes Formula and Cask targets; WinGet uses exact package IDs and versions.
- If a backend rejects an operation or cannot confirm the result, read the actual state first to avoid repeating writes through a different backend.
- Actions declare destructive risk through `dangerous`, independently of confirmation or authorization; services mark start or stop as the primary action according to their current state. firewalld exit code 253 is treated as insufficient permission and retains the existing authorized-query path and native error details.
- Network connection states and configuration restrictions use English and Chinese resources; device identities and native configuration syntax remain unchanged for later state checks.

## Shell control connections

`shell_control` provides local connections for the external CLI. Unix uses a socket and registration file private to the current user and checks the peer's UID/PID; Windows named pipes and directories allow only the current user's SID.

Each Shell poll processes at most 16 connections, with 64 KiB of reads and writes per connection; requests are limited to 1 MiB and responses to 32 MiB. Connections are cleaned up after 30 seconds, and registration files and sockets are removed on exit. Protocol types are defined by [shell-control](../shell-control/README.md).

## Documentation

| Topic | Document |
| --- | --- |
| Paths, startup identity, desktop and file integration | [System integration](docs/integration.md) |
| Services, processes, packages, and tasks | [Linux system management](docs/linux-management.md) |
| Authorization connections, revocation, and fixed operations | [Authorization and helper connections](docs/authorization.md) |
| Networks, Wi-Fi, disks, and directory scans | [Networks and disks](docs/network-disks.md) |
| Users, groups, and SSH public keys | [User management](docs/user-management.md) |
| File permissions, ownership, and recursive scope | [File permissions](docs/file-permissions.md) |
| Hostname, timezone, and automatic time synchronization | [Operating-system settings](docs/system-settings.md) |
| Configuration validation, saving, and recovery | [System configuration files](docs/system-config.md) |
| Configuration history, retention, and cleanup | [Configuration backups and recovery](docs/config-backups.md) |
| Listening ports, UFW, and firewalld | [Firewall and ports](docs/firewall.md) |
| SSH settings and confirmation from a new connection | [SSH service management](docs/ssh-management.md) |
| systemd timers and cron | [Scheduled tasks](docs/scheduled-tasks.md) |
| Task summaries, output, and cleanup | [Task history](docs/task-history.md) |
| Homebrew Formulae and Casks | [Homebrew packages](docs/homebrew.md) |
| WinGet packages | [Windows packages](docs/windows-packages.md) |
| Process, log, and directory performance | [Performance and retesting](docs/performance.md) |
| Operations commands, task IDs, and exit codes | [Operations CLI](../cli/docs/operations.md) |

## Validation

```sh
cargo test --locked -p platform
cargo test --locked -p platform --test shell_control
```

Run Linux management tests on Linux or WSL. Use the corresponding isolated environment or test host for real authorization, service writes, wireless devices, mounts, and physical-disk operations; see each topic's document for specific entry points.
