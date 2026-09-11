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
managed active logind session and tundra-admin membership, binds the action and
request ID, and restores the source session before returning. The privileged
service must revalidate sender identity and policy after consent returns.

## Trusted display requirements

The backend requires a kmscon build with libseat/logind, mouse and Pango support.
Both terminal renderers run without root. The greeter runs as a separate
`tundra-greeter` account and acknowledges initial rendering over its inherited
private channel. Immutable frontend executable/resources must be installed by
root. The backend uses these fixed executable locations:

- `/usr/bin/kmscon`
- `/usr/libexec/tundra/tundra-greeter`
- `/usr/bin/tundra-shell`

Root switches to tty8, issues kernel VT_LOCKSWITCH, and checks VT_GETSTATE before
claiming a protected handover. CAP_SYS_TTY_CONFIG is required to unlock, so user
VT_ACTIVATE and physical VT hotkeys cannot return to the old desktop during the
lock. A failure or process crash deliberately leaves the kernel gate locked;
recovery must happen through the preserved root SSH channel, not by an automatic
unlock in a destructor. `dev.tty.legacy_tiocsti=0` is required; users with raw
input/tty group membership are rejected.

Actual kmscon device release, evdev revocation, DRM master handover, keyboard,
mouse, Chinese rendering and crash recovery require physical integration tests.
A successful compile or PAM probe alone does not establish those guarantees.
Do not enable the installed service as a display-manager replacement until that
machine's end-to-end tests have passed. The systemd unit intentionally starts the
discovery-only mode.

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
