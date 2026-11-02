# SSH service management

The SSH page and `tundra-cli ssh` read the actual `ssh.service`/`sshd.service` and `ssh.socket`, and query effective settings for the specified user through `sshd -T -C`.

The page shows ports, password and public-key login, root login policy, `AuthenticationMethods`, interactive authentication, PAM, public-key commands, and SHA256 fingerprints. `Match` conditions affect the result; queries can use localhost/127.0.0.1 conditions by default. Password authentication and interactive authentication appear separately.

## Changing configuration

The form supports `Port`, `PasswordAuthentication`, `PubkeyAuthentication`, and `PermitRootLogin`, preserving `Include`, `Match`, and other original content.

Save flow:

1. Find referenced files from the actual read trace of `sshd -t -ddd`, remove old global `Port` settings, and write the new global settings.
2. Run `sshd -t` and `sshd -T -C` for the selected user/root against a private candidate view.
3. Check that `Match` and `ListenAddress` do not override the form's selections.
4. Save a shared configuration backup, start an independent recovery task, and wait until it is ready.
5. Atomically replace configuration and load SSH.

Adding and removing public keys reuse user management's parsing, fingerprinting, and atomic-save flow.

## Confirmation from a new connection

Within 120 seconds after loading, establish an actual new connection and confirm from that connection:

```sh
ssh -o ControlMaster=no -o ControlPath=none -p NEW_PORT user@host
tundra-cli ssh confirm TRANSACTION_ID
```

Confirmation checks the calling process and its start time, the root-owned sshd parent process, the kernel's established TCP connection, and the server port. The sshd session must have been created after loading the new configuration; old sessions and channels reused through ControlMaster cannot confirm. Newly added ports must be confirmed through a connection to a newly added port.

On timeout, cancellation, loading failure, or exit of the original application, the independent task restores files and loads the old settings. Pending-confirmation records are in `/var/lib/tundraux3/ssh`; the boot recovery service processes unconfirmed records before SSH and the firewall start. Rebooting does not restart the timer.

## Services and firewall

An ordinary sshd reloads through SIGHUP to its listener; existing sessions keep running.

`ssh.socket` requires `Accept=no`, a single supported SSH service, and `KillMode=process` before ports can be changed through a socket override and the listener/socket restarted. It remains read-only if these relationships are not satisfied.

Also Allow New Ports is off by default. When enabled, it adds only missing new TCP rules to the current UFW/firewalld backend and records them with the SSH configuration in the same recovery record. Recovery undoes only rules added by this operation; old ports and other rules remain unchanged.

## Validation

Ordinary tests cover configuration parsing, Include, sockets, input validation, process/TCP state, and conditions that reject confirmation.

Real OpenSSH connection tests run as root in private mount and network namespaces:

```sh
unshare -mn -- env TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  LD_LIBRARY_PATH=/private/test/usr/lib/x86_64-linux-gnu \
  /path/to/platform-test-binary \
  management::ssh::connection::real_tests::real_isolated_ssh_connections \
  --exact --ignored --nocapture --test-threads=1
```

Combined firewall and private systemd tests:

```sh
bash scripts/tests/firewall-isolated.sh /path/to/platform-test-binary \
  /private/firewall-package-root ssh /private/openssh-package-root
bash scripts/tests/isolated-systemd.sh /path/to/platform-test-binary \
  0 ssh /private/openssh-package-root
```

These cases verify new connections, old sessions, ControlMaster, incorrect ports, timeout recovery, SIGHUP, socket loading, and firewall rollback, using temporary accounts, keys, configuration, and private services.

References: [sshd](https://man.openbsd.org/sshd.8), [sshd_config](https://man.openbsd.org/sshd_config.5).
