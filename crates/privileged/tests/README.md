# Native system-bus negative integration tests

`system_bus.py` tests the installed `org.tundra.Privileged1` service through the
real Linux system bus, using distro `python3-dbus`. It is opt-in and is not part
of `cargo test`. It requires the protected production binary, policy, config and
systemd unit to have been installed by an administrator.

The harness does not shut down, reboot, install updates, execute commands, change
seats or authenticate a desktop. Its only valid action payload asks for one
system-log record; each negative subject must be refused before authorization.
Invalid update identifiers are checked without submitting a valid update.

Run it separately in an actual SSH user session, as root, and as a non-login
ordinary account with no Tundra administrator group:

```sh
python3 crates/privileged/tests/system_bus.py --actor ssh
sudo -n python3 crates/privileged/tests/system_bus.py --actor root
sudo -n runuser -u nobody -- python3 - --actor ordinary < crates/privileged/tests/system_bus.py
```

The SSH actor verifies that logind reports the current process as belonging to
that user's remote session. An ordinary `runuser` actor verifies the
out-of-managed-session boundary; it does **not** prove rejection of a non-admin
inside an active managed local Tundra desktop. The latter requires separate
seat integration tests.

Each run emits JSON containing actual UIDs, groups, logind provenance, unique bus
sender, pinned root service owner, and individual outcomes. The harness fails
if the service owner changes during the run, a malformed request is accepted,
a negative subject becomes eligible, an unexpected method is exposed, or a
non-root caller can request ownership of the production bus name.

`--foreign-operation-id ID` optionally verifies that an **existing** operation
belonging to another live bus sender cannot be read or cancelled. Without such
an operation, this case is explicitly skipped. Random unknown-ID refusal is
checked separately and must not be reported as ownership-boundary coverage.
Creating a live authorized operation or adding a production seeding backdoor is
outside this harness.

The method surface is checked against the five intended methods:
`ProtocolVersion`, `CanRequest`, `Request`, `GetResult`, and `Cancel`.

## Recorded Fedora execution

Executed on `x240s-test`, Fedora 43 with SELinux **Enforcing**, at
`2026-09-11T15:24:09Z`. The installed production binary SHA-256 was:

```text
2cd78d5b9dd0f795e431b637bb9f3e6916b86e0ddaf859d8df64c67937ad2e89
```

The service ran as task-owned transient
`tundra-privileged-negative-test.service`, using every property from the
production unit's `[Service]` section, including `User=root`,
`NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`,
`PrivateDevices=yes`, and the capability boundary
`CAP_DAC_READ_SEARCH CAP_SETGID CAP_SETUID`. The transient test deliberately
excluded production update-recovery **unit ordering** because the recovery
helper was not yet installed. This validates the production service mechanisms
and hardening properties, not production recovery/startup sequencing. The
production sessiond unit remained inactive; independent seat tests were owned
by the sessiond integration task.

| Actual subject | Passed | Failed | Skipped |
| --- | ---: | ---: | ---: |
| SSH user, UID/EUID 1001, logind `Remote=true`, no seat | 19 | 0 | 1 |
| Root, UID/EUID 0 | 18 | 0 | 2 |
| `nobody`, UID/EUID 65534, outside a matching managed session | 19 | 0 | 1 |

All three runs observed the same root service owner. The SSH and ordinary
requests were denied with `AccessDenied` and the active-local-seat requirement;
root requests were denied because root is not a desktop authorization subject.
Both non-root actors were denied production D-Bus name ownership. The ownership
attempt was also independently denied before the service name was owned.

The three foreign-existing-operation checks were skipped because no actual
operation ID was supplied. Root additionally skipped the specifically non-root
name-ownership case. No power or valid update action was submitted. The
transient service was handed to the sessiond integration task for subsequent
non-power consent tests and cleanup.
