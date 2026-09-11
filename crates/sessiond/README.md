# tundra-sessiond

Linux system D-Bus owner `org.tundra.Session1`, path `/org/tundra/Session1`.
The default invocation provides discovery and rejects session mutations. An
administrator explicitly starts `--seat` to supervise a dedicated seat0 greeter
on tty8 and one user desktop on tty9. It does not stop SDDM or replace the current
display manager automatically.

## Trust and lifecycle

Each PAM transaction runs in a freshly exec'd root worker over an inherited
AF_UNIX socketpair. Both the worker channel and the greeter channel require a
root peer; neither has a filesystem socket or public authentication RPC.
Authentication supports independent multi-step prompts, account policy,
expired-password changes, credential establishment, session opening, closing,
credential deletion and pam_end. The login worker remains privileged for cleanup;
kmscon and UX run after setresgid/setresuid, supplementary group initialization,
capability retention disabling, no_new_privs and descriptor closure.

The worker verifies logind's UID and seat, then derives HOME and other personal
paths from NSS and XDG_RUNTIME_DIR from system-owned runtime state. It never
inherits launcher DISPLAY, SUDO variables or arbitrary PAM environment entries.
Authentication results are checked against the expected UID, including unlock.

Session state binds UID and logind session ID. Lock preserves the user worker;
unlock runs a new authentication transaction without opening another session.
Logout drains only the verified session cgroup using pidfds, excludes the root
PAM worker, then closes PAM. User switching performs logout first. CloseSession
is root-only and writes the maintenance marker only after successful cleanup.

System-operation consent is root-only, requires a real D-Bus sender in the
managed active logind session, binds the action and
request ID, and restores the source session before returning. The privileged
service enforces its configured administrator groups before the request and
revalidates sender identity and policy after consent returns.

## Trusted display requirements

The backend requires a kmscon build with libseat/logind, mouse and Pango support.
Both terminal renderers run without root. The greeter runs as a separate
`tundra-greeter` account and acknowledges initial rendering over its inherited
private channel. Immutable frontend executable/resources must be installed by
root. The backend uses these fixed executable locations:

- `/usr/libexec/tundra/kmscon` (pinned libseat build with a matching sibling
  `kmscon-capabilities.json` digest declaration)
- `/usr/libexec/tundra/tundra-greeter`
- `/usr/bin/tundra-shell`

Root switches to tty8, issues kernel VT_LOCKSWITCH, and checks VT_GETSTATE before
claiming a protected handover. CAP_SYS_TTY_CONFIG is required to unlock, so user
VT_ACTIVATE and physical VT hotkeys cannot return to the old desktop during the
lock. A failure or process crash deliberately leaves the kernel gate locked;
crash recovery must happen through the preserved root SSH channel, not by an
automatic unlock in a destructor. An orderly SIGTERM/SIGINT shutdown instead
drains both desktop and greeter PAM workers, then explicitly restores the original
VT recorded before startup. Socket reads poll the stop flag while preserving
partial frames; writes and shutdown/reaping have bounded deadlines. `dev.tty.legacy_tiocsti=0` is required; users with raw
input/tty/video group membership are rejected. Persistent raw framebuffer or
card-device access cannot be revoked by a logind seat handover; the render group
alone does not grant the same display-control permissions.

Actual kmscon device release, evdev revocation, DRM master handover, keyboard,
mouse, Chinese rendering and crash recovery require physical integration tests.
A successful compile or PAM probe alone does not establish those guarantees.
Do not enable the installed service as a display-manager replacement until that
machine's end-to-end tests have passed. The systemd unit starts `--seat`, but packaging does not start or enable it;
explicit service activation expresses the decision to enter an independent seat.

## Native PAM probe

`tests/pam_lifecycle.py` is a root-only integration harness for disposable normal
accounts. Its credential JSON must be root-owned and mode 0600. Execute the
harness via a detached systemd transient unit, so pam_systemd can register a new
session rather than inheriting an existing SSH session:

```
sudo systemd-run --wait --pipe --collect \
  python3 /path/pam_lifecycle.py /root/test-credential.json session valid
```

The harness tests the real distribution PAM stack, checks UID/HOME/user bus and
runtime directory, then verifies the logind session disappears after cleanup.
`authenticate invalid` tests rejection. It does not print passwords or pass them
through command arguments/environment. Fedora uses `tundra-session.fedora`
installed as `/etc/pam.d/tundra-session`; Debian/Ubuntu uses `tundra-session`.

The worker protocol adds root-private `Authenticated {uid}` and
`SessionOpened {identity, environment}` lifecycle messages around shared greeter
PAM prompts. Those messages are never accepted from UX or exposed on system D-Bus.

## Native hardware evidence (x240s-test, Fedora 43)

The root-injected keyboard completed a real distribution PAM login into UID 1002,
logind session 345, with the ordinary shell's real/effective/saved UID and GID all
1002, empty permitted/effective/ambient capabilities and NoNewPrivs enabled.
HOME, personal XDG directories, runtime directory and user D-Bus matched that UID.
The final pinned static-libtsm backend recipe rendered the dedicated greeter and
accepted input through libseat without raw input ACL or group exceptions.

`tests/device_revocation.py PID` duplicated five existing user-renderer evdev FDs
and its DRM FD with pidfd_getfd before switching to the trusted VT. After handover,
every retained evdev FD returned ENODEV and the retained DRM FD ceased to be DRM
master. The snapshot preserved UID/session identity and entered Locked. The
`tests/vt_gate.py` probe additionally confirmed even root VT_ACTIVATE could not
leave the gated VT, while an unprivileged child could neither unlock the gate,
use TIOCSTI, nor open raw input/uinput devices.

The run exposed and verified a repair for an upstream libseat input-resume defect:
revoked input nodes could be deleted before the pause callback. The private build
recipe now preserves brokered input nodes on revoke and acknowledges seat disable.
With that repair, session 406 remained Locked after a wrong password, then returned
to Active with the same UID/session after successful PAM authentication. Logout
removed session 406 and its user processes before a new admin login created 426.
Admin SwitchUser likewise closed session 426 and returned to the login greeter.

A root-only uinput mouse delivered real libseat/kmscon pointer events to the trusted
buttons. Cancel produced operation status Cancelled; Confirm produced
AwaitingConfirmation → Running → Completed for a bounded five-record system-log
request, without another password. The ordinary managed account's CanRequest was
false; the separately configured admin account's was true. Independent SSH and
root D-Bus senders both received AccessDenied when reading or cancelling the admin's
existing operation. These are actual PAM/seat and pointer-path tests using
root-generated test input, not human mouse-click evidence. The final test restored
tty1 and stopped the task's temporary seat, input and privileged services.

Earlier PAM lifecycle probes independently passed valid/invalid authentication,
full session open/environment registration and close cleanup for ordinary/admin
accounts. Latest native sessiond unit tests passed all ten cases, including persistent-video access policy, failed-worker
reaping and the one-time transition out of maintenance after marker removal. The tests above used protected task-installed helper binaries; the installed-RPM
validation below separately verifies the final versioned runtime and service units.

The final lifecycle run started with the maintenance marker present, removed it
as root, and then completed an admin login into session 518. A deliberately wrong
password left only the supervisor and greeter worker, with no zombie child.
Managed logout removed 518 while an independently created same-UID PAM/logind
session 498 and its sleep process remained active. That independent fixture was
then stopped explicitly. Orderly systemctl stop restored tty1 and completed
successfully; a subsequent start reacquired tty8. Stopping again while a PAM
password prompt was pending completed PAM cancellation and restored tty1 within
three seconds. SDDM remained running throughout. The final tested backend used the
patched pinned recipe with static libtsm, matching the package artifact inputs.

## Installed RPM verification and cleanup

The corrected RPM was tested with SELinux Enforcing, using the formal systemd
units and `/var/lib/tundra/runtime/versions/v1.3.0/bin/tundra-sessiond`. The packaged
greeter's NSS HOME was `/nonexistent`. A real PAM login created admin UID 1003
session 592. A root-injected mouse click on the trusted Confirm button completed
a bounded five-record log operation through the freshly packaged privileged
worker: AwaitingConfirmation → Running → Completed. The source session resumed
with its original identity. Screenshot evidence is available in the task's
`/tmp/tundra-package-consent.png` artifact.

Orderly stopping the production sessiond removed session 592 and restored tty1.
Both Tundra daemons were left inactive and disabled; SDDM remained active and
enabled. All temporary keyboard/mouse, seat, independent-session and recovery
watchdog units were stopped. After confirming no disposable-account logind
sessions remained, the task's `tundra-it-user` UID 1002 and `tundra-it-admin` UID
1003 accounts and their HOME directories were removed. The root-only credential
JSON and all `/run/tundra-integration` fixtures were removed. Real users 1000 and
1001, the installed RPM, the production greeter account and `tundra-admin` group
were preserved. Independent readback confirmed this cleanup and the final service
states; no Tundra display-manager service was enabled automatically.
