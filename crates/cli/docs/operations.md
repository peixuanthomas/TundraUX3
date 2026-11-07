# Operations CLI

Use `tundra-cli <module> help` for the exact arguments. Prefix UX commands in the built-in Command Line with `/`, for example `/services list`; enter system commands directly as before.

## Command groups

| Module | Queries | Changes |
| --- | --- | --- |
| `services` | `list`, `show`, `dependencies`, `config` | `start`, `stop`, `restart`, `reload`, `daemon-reload`, `enable`, `disable`, `create`, `create-instance` |
| `processes` | `list`, `show`, `files`, `ports`, `io`, `service` | `term`, `kill`, `stop`, `cont`, `nice` |
| `packages` | `list`, `show`, `search`, `updates`, `status`, `conflicts`, `check`, `sources` | `install`, `remove`, `upgrade`, `upgrade-all`, `refresh`, repair and package-source drafts |
| `network` | `list`, `show`, `wifi-list`, `check`, `transaction-status` | Wi-Fi, `configure`, `disconnect`, `confirm` |
| `disks` | `list`, `show`, `health`, `inodes`, `scan` | `mount`, `unmount`, `automatic-mount` drafts |
| `users` | `list`, `show`, `lookup`, `groups`, `shells`, `ssh-keys` | Accounts, groups, Shell, expiration, and public keys |
| `system-config` | `read`, `diff`, `check`, `history`, `preview-restore` | `apply`, `permissions`, `restore`, `reload` |
| `operations` | `list`, `status` | `attach` reconnects to an existing task |
| `firewall` | `list`, `show`, `listeners` | `allow`, `remove` |
| `ssh` | `show`, `keys`, `status` | `configure`, `add-key`, `remove-key`, `confirm`, `cancel` |
| `backups` | `list`, `show`, `preview-restore` | `restore` |
| `scheduled-tasks` | `list`, `inspect`, `preview` | `create`, `pause`, `resume`, `run-now` |

Queries support `--json`. Standard output contains only results; progress and native-tool output go to standard error. The first column in text lists is the exact ID to use for subsequent operations. Log following in JSON mode emits JSONL.

## Confirmation and authorization

- `--yes` confirms the current change; `--non-interactive` disables interactive prompts. System authorization is still required.
- `--password-fd FD` supplies Wi-Fi or account passwords; `--authorization-fd FD` supplies the sudo password. Only one line is read each time; passwords are not placed in command-line arguments.
- Using `--yes --non-interactive` together for the supported operations commands also satisfies the root startup confirmation.
- Protected read-only queries can use `--authorize`; queries that require authorization to read use the same authorization workflow.

## Firewall, SSH, backups, and scheduled tasks

```sh
tundra-cli firewall listeners --json
tundra-cli firewall list --json
tundra-cli firewall allow --port 8000-8010 --protocol tcp --source 192.0.2.0/24 --yes
tundra-cli firewall allow --port 8443 --zone public --duration both --yes
tundra-cli firewall remove RULE_ID --yes

tundra-cli ssh show alice --json
tundra-cli ssh configure alice --port 2222 --open-firewall true --yes
tundra-cli ssh confirm TRANSACTION_ID --yes --non-interactive

tundra-cli backups list --json
tundra-cli backups preview-restore BACKUP_ID --json
tundra-cli backups restore BACKUP_ID --allow-unvalidated --yes

tundra-cli scheduled-tasks list --json
tundra-cli scheduled-tasks preview --name daily-job --command '/usr/bin/true' --schedule daily --json
tundra-cli scheduled-tasks create --name daily-job --command '/usr/bin/true' --schedule daily --yes --non-interactive
tundra-cli scheduled-tasks pause TASK_ID --yes
tundra-cli scheduled-tasks resume TASK_ID --yes
tundra-cli scheduled-tasks run-now TASK_ID --yes
```

Firewall commands operate on allow rules in the existing UFW/firewalld setup. firewalld's `--duration` supports `runtime`, `permanent`, and `both`. Operations remain read-only when both backends are running.

SSH `show`, `keys`, and `configure` can omit the account, defaulting to the Linux account that launched the program. `configure` supports the port, password authentication, public-key authentication, root login, and `--open-firewall`. Unspecified fields retain their current values.

SSH changes wait up to 120 seconds. Run `ssh confirm` from a genuinely new SSH connection with connection sharing disabled; query with `ssh status` or cancel with `ssh cancel`. Timeout or cancellation restores the old settings. The transaction ID is provided on standard error and in the JSON `transaction_id` field.

Backups are grouped by operation; current files and runtime state are checked before restoration. `--allow-unvalidated` only accepts the absence of an available validator; failed validation still blocks restoration.

Scheduled tasks list systemd timers and cron together. Use `preview` to inspect a definition before saving it with `create`. Pausing only prevents future triggers and does not stop commands already running.

## Services and processes

Service details include recent results, exit reasons, dependencies, and logs. `services reload` reloads a running service; `daemon-reload` makes systemd reread definitions. After creating a service or changing configuration, explicitly choose to load, start, or enable it.

Process details can include open files, TCP/UDP ports, cumulative disk reads/writes and rates, and the owning service. The PID and start time are checked before and after collection; if the process exits or the PID is reused, the corresponding status is returned.

## Package recovery and sources

Status checks distinguish lock contention, downloads, signatures, dependencies, space, and incomplete transactions. APT provides `repair-configure` and `repair-dependencies`; other backends show guidance. Users handle lock files and the processes holding them based on native output.

`packages conflicts` lists configuration candidates for comparison and merging in the editor. Package sources support APT's traditional format and deb822, DNF, and pacman; adding, enabling, disabling, or removing a source first generates a draft.

Scripted package operations use `--yes --non-interactive` and native noninteractive options, preserving the current configuration by default. APT can explicitly replace it with `--config-policy replace`. Arch installation and upgrades always include a full system upgrade.

## Configuration drafts and saving

Creating services, changing package sources, and configuring mounts at boot first return a draft containing `path`, `content`, and `validator`. Saving starts only when `--apply --expected-version VERSION` is added.

```sh
tundra-cli services create demo --program /usr/bin/sleep --arguments 30 --json
tundra-cli system-config read /etc/ssh/sshd_config --json
tundra-cli system-config diff /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config check /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config apply /etc/ssh/sshd_config --input sshd.candidate --expected-version VERSION --yes
tundra-cli system-config reload /etc/ssh/sshd_config --service ssh.service --backup-id SAVED_BACKUP_ID --yes
tundra-cli system-config history /etc/ssh/sshd_config --json
tundra-cli system-config preview-restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --json
tundra-cli system-config restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --yes
```

Failed validation blocks saving. If no validator is available, explicitly use `--allow-unvalidated`. For systemd, `reload` rereads definitions by default; `--reload-service` also reloads the service. SSH requires the `document.backup_id` returned when saving.

## Background tasks and network confirmation

The default maximum wait is 900 seconds, adjustable with `--wait SECONDS`. Timeout or disconnection returns an incomplete status and task ID while the helper continues running. `operations attach ID` reconnects, and `operations status ID` waits one second by default.

When network configuration returns `awaiting_confirmation` and `transaction_id`, check the connection and run:

```sh
tundra-cli network confirm TRANSACTION_ID --yes --non-interactive
```

Without confirmation within 120 seconds, the old settings are restored.

## Exit codes

| Exit code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Failure or partial completion |
| 2 | Argument error |
| 3 | Permission denied |
| 4 | Unsupported or a required tool is unavailable |
| 5 | Busy or version conflict |
| 6 | Input required |
| 7 | Incomplete or result unknown |
| 130 | Cancelled |

Native-tool exit codes are preserved in structured error results when available.
