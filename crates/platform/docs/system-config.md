# System configuration files

Service overrides, package repositories, fstab, and other system files are edited in the same text editor. Saving includes version checks, diff previews, candidate validation, system authorization, backups, and atomic replacement.

## Reading and validation

Reads record the file's content version, owner, group, and permissions. Before saving, content and attribute differences are shown, and the corresponding validator runs against a private candidate view.

- SSH uses `sshd -t -ddd` to check the main configuration, Include fragments actually read, and host keys.
- systemd uses `systemd-analyze verify`, preserving the original unit and other overrides; user services use the corresponding `SYSTEMD_UNIT_PATH`.
- fstab uses `findmnt --verify`; package repositories use the distribution's parsing checks.
- Time-synchronization configuration can validate multiple candidate files together and back them up as one operation.

Candidate content appears only in private connections and temporary validation directories. Failed validation blocks saving; when no validator is available, saving without validation must be selected explicitly.

## Saving and recovery

Before writing, recheck the original file and its parent directory. Save through atomic replacement, preserving owner, group, permissions, ACLs, and extended attributes. New files inherit the owner of the nearest existing parent directory; symbolic links are rejected at every level of the path.

Recovery records are in `/var/lib/tundraux3/config-backups`, with directory permissions `0700` and file permissions `0600`. Records contain original content, attributes, and the version after modification.

Before recovery, preview the diff again, check the current version, and validate the candidate configuration. Restoring an originally absent file deletes the file created by the operation and retains a recovery record for that deletion.

If a multi-file save fails partway through, restore previously written files against their versions from this save. If external changes are found, stop overwriting and report a conflict.

## Loading after saving

By default, only the configuration is saved.

- Reloading SSH requires the backup ID from this save and `tundra-cli ssh confirm ID` from an actual new connection within 120 seconds; otherwise, the old configuration is restored.
- systemd rereads service definitions by default; `reload_service=true` also reloads the service.
- Configuration saving, service loading, and runtime state are reported separately. If loading fails, inspect logs or restore from a backup.

CLI usage:

```sh
tundra-cli system-config read|diff|check|apply|permissions|history|restore|reload …
```

Scripted saves must provide `--expected-version`; saving without validation must explicitly provide `--allow-unvalidated`. In the built-in Command Line, use `/system-config …`.

## Validation

```sh
TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  cargo test --locked -p platform real_isolated_ssh -- --ignored
cargo test --locked -p platform real_isolated_user_service -- --ignored
```

Ordinary tests cover version conflicts, external changes, attribute preservation, backup permissions, and recovery. Real-validator tests run as root in a private mount namespace, using a temporary `/etc` and validation tools without changing host configuration.

References: [OpenSSH sshd](https://man.openbsd.org/sshd.8), [OpenSSH Include parser](https://github.com/openssh/openssh-portable/blob/master/servconf.c), [systemd-analyze](https://www.freedesktop.org/software/systemd/man/latest/systemd-analyze.html).
