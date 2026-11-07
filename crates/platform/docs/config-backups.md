# Configuration backups and recovery

Configuration Backups and Recovery lists time, source, related files, and results by operation. Each change first saves the originals and their attributes, then records the actual result; multiple files involved in one operation are stored in a single record.

## Records and recovery

Shared records are in `/var/lib/tundraux3/config-backups/operations`, with directory permissions `0700` and file permissions `0600`. They may contain pre-change text, UID/GID, permissions, ACLs, extended attributes, and post-change versions.

Before recovery, recheck the current version and candidate configuration, and preview the diff. Stop if later external changes are found; when no validator is available, recovery without validation must be selected explicitly. Recovery itself also saves the current originals, then rereads and checks the result after completion.

Configuration saving, service loading, and connection verification report their results separately. SSH and network recovery continue to use their respective new-connection confirmation flows.

The editor, networks, public keys, firewall, repositories, fstab, service definitions, and scheduled tasks all write to the shared records. Package transactions, external-program changes, and ordinary service starts and stops are outside file-configuration backups.

## Browsing and cleanup

- The search box filters by text; the filter panel sets start/end dates and sources. Dates use the system timezone.
- Each batch displays 50 records, with sorting across all matching records.
- Lists read summaries; full content and diffs are read and calculated only when opening details. The summary index can be rebuilt from old records.
- Retain/Unretain prevents records from being cleaned up; rebuilding the index does not affect retention flags.

Cleanup first previews the recovery points and related files to be deleted, and the expected space savings, using the current filters. After confirmation, it rechecks the same records. Records that are in use, still needed for network/SSH confirmation, referenced by other retained records, or already changed are not deleted.

## Accounts and groups

Account recovery handles only accounts, groups, and attributes affected by the selected operation. It supports creation, deletion, passwords, locking, display names, Shells, expiration, primary groups, supplementary groups, and related operations.

Before recovery, check current values and whether names or UID/GID values are already in use; reread system account data afterward. Restoring a Shell does not overwrite a display name changed later; restoring one user's group membership preserves other members.

Deleting accounts and undoing their creation both preserve home directories. Existing protections for the current account, root, and remote directory accounts remain in effect.

Passwords are stored only as protected hashes; diffs show only "Password changed." Records with unconfirmed results require checking the actual state before deciding whether recovery is possible.

## Validation

```sh
cargo test --locked -p platform management::config_backups --lib
cargo test --locked -p platform management::users::recovery --lib
```

Real account-recovery tests run as root in an isolated mount namespace:

```sh
unshare --mount --propagation private --fork TEST_BINARY \
  --exact management::users::recovery::tests::real_isolated_account_recovery_preserves_unrelated_changes_and_homes \
  --ignored --nocapture --test-threads=1
```

Tests use temporary `/etc`, `/run`, `/var/lib`, `/var/log`, `/var/spool`, and `/home` directories, covering attribute recovery, home-directory preservation, group membership, UID/GID conflicts, and subuid/subgid.
