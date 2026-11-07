# User management

Open User Management from Launcher. Linux enumerates root, service accounts, and regular accounts through NSS, and can look up directory accounts by exact name when enumeration is unsupported. Accounts and groups from remote directories remain read-only; Windows/macOS use the local application account flow.

## Queries

Name lookups support `DOMAIN\user` and `user@domain`. After selecting an account, complete membership information is read through `id --groups` and NSS group queries, without requiring the directory service to support group enumeration.

Failed membership queries are shown as unknown. Until the current account's administrator groups are known, changes that could remove administrative access are not performed.

## Changes and protections

Linux supports changing display names, passwords, primary groups, supplementary groups, login Shells, expiration, and SSH public keys, as well as creating, renaming, and deleting groups and changing membership.

- Select Shells from `/etc/shells`; expiration uses `YYYY-MM-DD` or `never`.
- SSH public-key locations are determined through `sshd -T -C`, with support for user-level `AuthorizedKeysFile`. Add or remove keys by SHA256 fingerprint, preserving other keys and options.
- Every write goes through AA and system authorization, preferring operations supported by AccountsService and using fixed system tools when those operations are unavailable or unsupported.
- If a write is denied or its result is unconfirmed, refresh the state first rather than repeating the operation through another backend.
- root cannot be deleted; the current account cannot be deleted, locked, expired immediately, or removed from identified administrator groups.
- Deleting an account preserves its home directory; groups still used as primary groups cannot be deleted.
- If setting a new account's password fails, the account is retained and the page provides an action to continue setting the password.

Each change writes a protected operation record, which can be viewed and restored in [Configuration backups and recovery](config-backups.md). Passwords are stored only as hashes; recovery preserves home directories, unrelated attributes, and unrelated members.

## CLI

The page and CLI share operations and input checks from `platform::management::users`:

```sh
tundra-cli users --help
```

Queries support JSON; passwords are passed through hidden input or a specified input descriptor.
