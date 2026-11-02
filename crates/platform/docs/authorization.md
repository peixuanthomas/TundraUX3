# Linux management authorization and helper connections

`platform::management::client` and `authorization` provide Shell and CLI with fixed operations, event reception, retained authorization, and task reconnection. AA decides whether to approve an operation; the operating system determines the process's actual permissions.

## Initial authorization

1. When built-in privilege elevation is needed, start `tundra-cli __privilege-session` through sudo.
2. Pass the password to sudo only through standard input, then clear it from memory after verification.
3. sudo uses `-k` with a command and does not read or refresh other programs' password caches.
4. The authorized program inherits a private bidirectional connection, reused for subsequent management, account, power, and task-reconnection requests.
5. Each operation still follows AA's approval policy.

This connection accepts only fixed requests. Normal users' own profile and password changes, and power operations already permitted by the system, follow their respective system authorization flows.

## Connection protection and revocation

| Object | Rule |
| --- | --- |
| Authorized program | Holds only the connection inherited at startup; does not expose a session socket that other processes can connect to |
| Shell | Prevents processes owned by the same user from copying authorization through ptrace or `/proc` |
| Connection descriptors | Set close-on-exec; do not pass them to external commands |
| Exit, logout, or communication failure | Revoke the connection and terminate the authorized program |
| Normal service errors or expired tasks | Return an error and retain an authorization session that is still valid |

A task's `control.sock` is owned by root and accepts only root. The authorized program checks the ownership, permissions, and task identity of `/run/tundraux3-management/<actor>/<operation>/control.sock`, then passes the connected descriptor to Shell. A new Shell must authorize again before reconnecting.

SSH confirmation uses the actual calling process, start time, sshd parent process, and kernel TCP state supplied through the authorization connection. It does not use caller-supplied process IDs or `SSH_CONNECTION`.

The helper receives a copy of the current language resources in the request and uses it in memory, without reading user language files as root. Command arguments, action names, and object identities retain their original values; transaction IDs are returned through separate events.

## Fixed operations and results

`ManagementAction` describes operations using stable action names, `primary`, `group`, and fixed `values`; button positions do not affect what executes. `OperationProblem` returns stable error codes, next steps, and technical details.

- Process signals use pidfd and check the start identifier.
- Network configuration saves the original configuration before starting an independent recovery helper.
- Package writes preserve native command output and confirmation flows.
- System configuration reaches the editor through private events; candidate content does not enter ordinary drafts or runtime logs.
- When the result is unknown, query the actual state first; do not automatically retry writes.

Related documents: [System integration](integration.md), [Linux system management](linux-management.md), [System configuration](system-config.md), [Networks and disks](network-disks.md).

## Validation

```sh
cargo test --locked -p platform management:: --lib
```

Automated tests cover fixed commands, identity, and input validation; real sudo/polkit authorization is verified on the corresponding Linux test host.
