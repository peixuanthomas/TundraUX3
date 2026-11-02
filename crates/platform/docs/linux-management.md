# Linux system management

Open Services, Processes, Packages, Network, Disks, User Management, Firewall, SSH, and Scheduled Tasks from Launcher. The application normally runs as a regular user; operations requiring system permissions go through AutoAdmin (AA) and fixed authorization helpers.

## Page controls

The top of management pages contains search, filtering, and refresh controls. Primary actions are at the bottom right, and less frequent actions are under More Actions. Every action has a visible button labeled with its actual shortcut.

| Key | Behavior |
| --- | --- |
| S or `/` | Enter search text; Enter applies it and Esc finishes input |
| R or F5 | Refresh the current list |
| F4 | Expand or collapse details |
| Tab / Shift+Tab | Move between navigation, list, details, and actions |
| Ctrl+Enter / Ctrl+U | Submit a search or form / clear the current text field |
| Ctrl+T | Open the operation's AA output |
| F12 | Open the task-close confirmation for the current page |
| Esc | Close a form or details, or go back |

In forms, Enter moves to the next field, selection fields open an option list, and Ctrl+Enter submits directly. Long lists, details, and forms support the keyboard, mouse wheel, and scrollbars.

See [List selection](../../../docs/list-selection.md) for multiple-selection behavior, and [AutoAdmin workflow](../../auto-admin/docs/workflow.md) for AA input and stopping rules.

## Supported operations

| Entry | Main operations | Backend and requirements |
| --- | --- | --- |
| Services | Start, stop, restart, reload, enable, disable, dependencies, logs, and unit configuration | systemd |
| Processes | Process tree, signals, priority, files, ports, and read/write metrics | Check PID and start time |
| Packages | Query, install, remove, upgrade, repositories, and configuration conflicts | APT/dpkg, DNF, pacman |
| Network | Addresses, routes, DNS, Wi-Fi, and diagnostics | Wi-Fi uses NetworkManager |
| Disks | Mounts, fstab, health, inodes, and directory scans | UDisks/smartctl and system file interfaces |
| Users | Accounts, groups, Shell, expiration, and SSH public keys | NSS, AccountsService, and fixed system tools |
| Firewall and ports | Listening ports, adding and removing allow rules | UFW or firewalld |
| SSH | Effective settings, ports, public keys, and login policies | OpenSSH and systemd |
| Configuration backups | Diffs, originals, results, recovery, and cleanup | Shared recovery records |
| Scheduled tasks | systemd timers, crontab, pause, resume, and run now | systemd and local cron |

See [Networks and disks](network-disks.md), [User management](user-management.md), [Firewall](firewall.md), [SSH](ssh-management.md), and [Scheduled tasks](scheduled-tasks.md) for detailed requirements.

## Packages page

The left side offers Installed, Updates, and Search, opening Installed by default. Details query the local database or repositories. Primary actions are at the bottom of the details area, while advanced sections handle repositories, state repair, and configuration conflicts separately.

- Queries use existing repository information; explicitly refresh when current data is needed.
- Search displays at most 2,000 rows; narrow the query if needed.
- Before writing, recheck installed versions, candidate versions, and the complete target list.
- Native dependency confirmations, configuration conflicts, and installation-script questions appear in the AA terminal, which accepts input.
- Normal removal does not automatically purge or autoremove; APT/DNF upgrades are not distribution upgrades.

### Arch / pacman

Installed-package, search, detail, and update queries use `/usr/bin/pacman`. Repository queries also require `/usr/bin/pacman-conf` and an available sync database; repositories must support the corresponding Search, Install, and Upgrade uses.

Installation, selected-package upgrades, and upgrading all packages execute `pacman -Syu --needed`, appending package names when targets are specified. These operations upgrade the entire system; pacman confirms final versions, dependencies, and the transaction in the AA terminal.

Normal removal uses `pacman -R -- PACKAGE_NAME`, preserving `.pacsave` behavior. Check `.pacnew` files after upgrades according to native output. Packages outside repositories can be inspected and removed; AUR builds and helpers are outside the backend's scope.

## Background tasks and authorization

When closing a page with an active operation, you can keep it running in the background, cancel and close, or stay on the page. AA reappears when input is needed. Exiting Shell requests that current-session tasks stop and waits for cleanup.

The first built-in elevation establishes a private authorization connection for the current Shell through sudo; the password is cleared after verification. Exit, logout, or connection failure revokes authorization; restarting Shell requires authorization again.

Tasks still running in a helper can restore output and interaction through Reconnect System Task. When the result is unknown, query the actual state first instead of resubmitting the same write operation.

Network configuration and loading SSH settings use independent recovery tasks and require confirmation within 120 seconds. Confirm network changes with `network confirm`; SSH uses `tundra-cli ssh confirm ID` and verifies an actual new connection.

## Validation

```sh
cargo test --locked -p platform management::packages --lib
cargo test --locked -p platform management::packages::query::tests::live_apt_installed_search_updates_and_details --lib -- --ignored
cargo test --locked -p platform management::packages::pacman::tests::live_pacman_installed_search_updates_and_details --lib -- --ignored
```

The ignored APT/pacman cases query the local database without writing. Real package writes, Wi-Fi, mounts, and authorization flows are verified in recoverable isolated environments or on test hosts.
