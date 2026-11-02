# Firewall and listening ports

`management::firewall` queries and modifies an existing UFW installation or a running firewalld instance, supporting the addition and removal of allow rules. When both backends are running, only read-only queries remain available to avoid ambiguity about which backend to modify.

## Listening ports

`query.scope = "listeners"` reads `/proc/net/tcp`, `tcp6`, `udp`, and `udp6` in the current network namespace. TCP shows listening sockets; UDP shows bound, unconnected addresses.

`listening_ports` matches socket inodes against `/proc/PID/fd` and checks PID and start time before and after reading. Insufficient permissions, exited processes, and other namespaces are reported separately; AA can be used to read protected process information.

## Native rules

### UFW

- Use `status numbered` while running, and `show added` to read saved rules while disabled.
- Add rules with the native `allow in proto ... from ... to any port ...` command.
- Modify runtime and saved rules together; while disabled, only save without enabling the firewall.
- Before deletion, check all native arguments, the address family, and rule identity; numbers are used only for display and verification.

### firewalld

- Use the default zone, active zones, and `--list-all-zones` to read runtime and permanent configuration separately.
- Add rich rules supporting source IPs/subnets, TCP/UDP, and port ranges.
- `runtime` takes effect immediately without saving, `permanent` only saves, and `both` changes the two configurations separately without a global reload.
- Existing service, port, source-port, and rich allow rules can be removed; other rules and settings are read-only.

All tools run with fixed paths and argument arrays. Reread the complete rule state before writing and after backing up; reject stale requests if external changes are found. File locks serialize writes from this application.

Actions include `show_listening_ports`, `show_firewall_rules`, `inspect_firewall`, `add_rule`, and `remove_rule`. New-rule fields are `protocol`, `port`, and `source`, with `zone` and `duration` added for firewalld.

## Backup and recovery

Before writing, `config_backups::begin("firewall", ...)` saves UFW configuration files or the affected firewalld zone XML, along with the native rule state before and after the change.

Before recovery, check current rules and backup files and first create a reverse backup of the current state, so recovery itself can also be undone.

- After restoring UFW files, run `ufw reload` only if UFW was enabled originally and is still enabled now.
- firewalld precisely reverses the rules from this operation and restores zone XML and the native `.xml.old` file without a global reload.
- Stop recovery if current rules or files have been changed externally.

SSH port rules join the same operation record as SSH through `prepare_ssh_ports` and the corresponding recovery interface. Only missing new TCP ports are added; old ports and existing rules remain unchanged, and rollback removes only rules added by this operation.

During boot recovery, before D-Bus and firewalld are running, the official `firewall-offline-cmd` handles permanent rules. UFW uses official commands to add or remove rules by native rule identity, without overwriting rule files directly.

## Validation

```sh
cargo test --locked -p platform management::firewall --lib
cargo test --locked -p platform management::listening_ports --lib
```

Ordinary tests use offline data and temporary sockets without modifying real firewalls.

Real UFW/firewalld cases run using extracted official packages and private mount/network/PID/D-Bus environments:

```sh
scripts/tests/firewall-isolated.sh TEST_BINARY UNPACKED_ROOT
scripts/tests/firewall-isolated.sh PLATFORM_UNIT_BINARY UNPACKED_ROOT ssh SSH_UNPACKED_ROOT
scripts/tests/firewall-isolated.sh PLATFORM_UNIT_BINARY UNPACKED_ROOT firewalld-boot
```

These cases cover adding and removing rules, runtime/permanent configuration, complete recovery, combined SSH rollback, and boot recovery without daemons, and check that the host's nftables rules remain unchanged.
