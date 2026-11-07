# File permissions and ownership

Select one or more items in File Explorer and open Properties → Permissions and Ownership. Owner, group, and other users' read, write, and execute permissions can be set separately for files and directories. Operations are non-recursive by default.

Mixed values appear as `[-]`; unchanged items retain their individual original values. Owners and groups can be looked up by name or ID, or left unchanged.

## Recursive scope and writes

Changes are performed through AA and the system authorization helper:

1. The helper scans the scope and lists full paths, differences, and reasons for skipping in pages.
2. After user confirmation, it rescans and checks file identity.
3. Symbolic links, nested mount points, and special files are skipped; overlapping selections and hard links are deduplicated.
4. Directory permissions are changed last to avoid losing access to subdirectories prematurely.

Targets are passed using their raw Linux filenames; the listing renders special characters such as newlines visibly. Permissions or ownership already matching the target are not written again.

Ordinary permission changes preserve special permission bits. Additional ACL entries are not edited on this page, and default ACLs are preserved. Changing group permissions affects the permission ceiling for additional rules; Linux may clear special permissions or file capabilities when ownership changes. The interface explains this before confirmation.

Cancellation or failure stops further changes and lists completed, failed, and unprocessed items; completed changes remain in place.

## Validation

```sh
cargo test --locked -p platform file_permissions --lib
cargo test --locked -p ui --test file_permissions
```

Shell tests cover the keyboard, mouse, and touchscreen model without hover. On Windows/macOS, the page indicates that the operation is unsupported on the current platform.
