# identity

`identity` handles application identity and UX permissions: Linux attaches to the current process's system user, while Windows/macOS use Tundra local accounts.

## Modules

| Module | Responsibility |
| --- | --- |
| `identity` | `AuthSession`, identity sources, and user accounts |
| `backend` | Identity backend interface |
| `credentials` | Password validation, Argon2 hashing, and verification |
| `session_service` | Login sessions, failure counts, and lockouts |
| `user_service` | Local account management |
| `authorization` | Guest/User/Admin roles and permission checks |
| `linux` | Current Linux process user |

## Local account rules

The following rules apply to Windows/macOS and test backends:

- Passwords use Argon2 hashes with random salts. They must contain 10–256 characters and cannot consist entirely of whitespace or equal the normalized username.
- Username matching is case-insensitive. Unknown users and incorrect passwords return the same invalid-credentials result.
- Five consecutive authentication failures cause a five-minute lockout. Failure and lockout information is persisted.
- Authentication sessions are kept only in memory.

UX roles are Guest, User, and Admin. File and profile operations are available to User/Admin; user management, Command Line, Launcher management, diagnostic repairs, and settings management require Admin.

Linux system accounts, passwords, and lockout policies are managed by the operating system. Saved UX roles do not grant Linux permissions. System accounts are managed through `tundra-cli users` and the system authorization workflow; CLI `config` does not read or write identity fields.

## Validation

```sh
cargo test --locked -p identity
```
