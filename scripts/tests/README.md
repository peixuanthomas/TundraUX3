# Disposable Fedora PackageKit fixture

Run these fixtures on a Fedora Linux test host with rootless Podman. They do not
replace the host's Tundra installation. Keep an SSH login session open when the
host stops user containers after logout. Use a fresh container name for each
independent transaction test.

Build the native test driver from the exact source under test, then prepare an
isolated system bus, PackageKit, polkit, and signed local RPM repository:

```sh
cargo build --locked -p platform --example packagekit-update-probe
podman build -f scripts/tests/Fedora.Containerfile -t localhost/tundra-packagekit-fixture .
podman run -d --name tundra-packagekit-fixture --systemd=always localhost/tundra-packagekit-fixture
podman exec tundra-packagekit-fixture mkdir -p /srv/tundra-fixture
podman cp target/debug/examples/packagekit-update-probe tundra-packagekit-fixture:/srv/tundra-fixture/probe
podman cp scripts/tests/packagekit-fixture.py tundra-packagekit-fixture:/srv/tundra-fixture/setup.py
podman exec tundra-packagekit-fixture python3 /srv/tundra-fixture/setup.py
podman exec --user tundra-test --env HOME=/home/tundra-test tundra-packagekit-fixture /opt/tundra-fixture/bin/packagekit-update-probe preview
```

`preview` must show `tundraux3` 1.0.0-1 → 2.0.0-1 and a new `tundra-runtime`
dependency. It must omit `tundra-unrelated`. The driver checks that its own
executable belongs to the installed `tundraux3` RPM; running the uninstalled
build directly is intentionally unsupported.

Replace the final `preview` argument with `execute`, `cancel`, `check`, or `query`
to exercise those fixed operations. Inspect `RESULT`/`QUERY` and query the actual
RPM database; a process exit code alone is not evidence of installation success:

```sh
podman exec tundra-packagekit-fixture rpm -q tundraux3 tundra-runtime tundra-unrelated
```

Successful execution must report `Installed` after rereading RPM, with Tundra
2.0.0-1, its dependency 1.0.0-1, and unrelated package still 1.0.0-1. A subsequent
check must have no candidate. Cancellation invokes only PackageKit's `Cancel`
when `AllowCancel` is true. Late cancellation may leave already installed
packages; it does not promise rollback.

For result-loss recovery, preserve the recorded target, timestamp, UID, and
transaction hint in the test user's state directory, change only the journal
state from `finished` to `pending` to simulate loss of the final frontend write,
restart PackageKit **after the transaction finishes**, then run `query`. Recovery
must use new history queries and installed RPM evidence, without updating again.

The fixture generates an ephemeral signing key inside the container, imports its
public key, and verifies RPM signatures. Never export its private key. It disables
the container's other repositories and uses a fixed local repository with package
signature checking enabled. The narrow fixture polkit rule permits only the
PackageKit system-update action for the fixture user; remove it when testing real
authentication agents. No password is needed for these backend transaction tests.

The script installs a **container-only** polkit service override because Fedora's
service seccomp/mount isolation requires capabilities unavailable in rootless
containers. The real polkit daemon and policy evaluation remain in use. This
override is never shipped in application packages and does not validate the
host's systemd service confinement.

The fixture refuses to reset an already prepared container. A stopped-container
snapshot can provide a repeatable baseline; select and remove only your explicitly
named test containers and images when finished. Never run these operations on the
host RPM database or its PackageKit service.

## Real Shell and authentication agents

For the frontend cases, build `cargo build --locked -p shell --bin tundra-shell`
and use a **fresh** fixture container. Copy `target/debug/tundra-shell` to the
fixed `/srv/tundra-fixture/probe` path instead of the backend example, and copy
`crates/ascii-assets/assets` to `/srv/tundra-fixture/assets` **before** running
`packagekit-fixture.py`. The asset contents must match the exact Shell build.
The setup script installs this ELF in the signed fixture RPM. Its fixture name
is intentional: ownership detection must inspect the actual running executable.

```sh
podman cp scripts/tests/authorization-pty.py tundra-packagekit-fixture:/srv/tundra-fixture/authorization-pty.py
podman exec tundra-packagekit-fixture python3 /srv/tundra-fixture/authorization-pty.py success
```

Supported cases are `preview`, `success`, `cancel`, `denied`, `crash`, `interrupt`,
`eof`, `package_error`, `gui`, and `power_denied`. Start each independent case from installed
Tundra 1.0.0 with the signed 2.0.0 candidate. Success and GUI cases install 2.0.0;
use a fresh prepared-container baseline before another case. The harness does
not reset installed versions or kill PackageKit/RPM to cancel transactions.
`package_error` enables a fixed fixture-only failing RPM `%pre` script and
removes its marker in `finally`; the real application RPM has no such script.

The harness runs only as root **inside the disposable container** to provision
one disposable password for `tundra-test`, then enters Fedora's `/bin/login`
session and executes the Shell with real/effective UID 1000. The OS login is
necessary for Fedora 43 polkit's fallback-agent lookup; `podman exec --user`
alone has no logind session and cannot validate this path. This is test setup,
not application session management. Never supply a real password to this script.

The `gui` case starts an isolated Xvfb display with TCP disabled and KDE's
polkit agent in the same user session, then uses xdotool to answer only that
agent's windows. KDE requires Kirigami and Qt Quick Controls, included in the
fixture image. It checks there is no text authentication prompt. Policies that
do not retain authorization can prompt in both the agent availability check and
the actual PackageKit request; the fixture observed two graphical prompts.
The other update authentication cases require the real Fedora `pkttyagent` prompt.
`power_denied` installs a container-only explicit polkit NO rule for the fixture
user and power-off actions, checks the denied request restores the TUI without
an agent prompt, and removes the rule in `finally`. It must not shut down the
container or host.

Each invocation uses fresh XDG paths so old frontend journals cannot contaminate
the next case. Assertions cover ordinary ownership of generated XDG data, credential absence
from Shell output/state/logs, actual installed RPM version, resumed raw mode,
alternate screen, mouse/focus reporting, hidden cursor and terminal restoration
on exit, absence of watchdog incidents, and preservation of the unrelated package.
The `success` case also activates the restart action and verifies the new Home
frame and the running executable inode against the installed RPM payload. This
catches Linux restart failures after RPM unlinks the running executable.
The emulator answers cursor-position queries required during redraw.
Only the disposable credential is typed, and failure captures redact it. The
`crash` case kills only the Shell's own text agent; interruption targets only
the fixture frontend. Monitor captures contain PackageKit property changes,
never authentication prompt responses. Artifacts remain under
`/srv/tundra-fixture` in the test container.

## Package metadata and payload validation

`package-artifacts.py` checks the actual RPM spec and DEB control using already
built native Shell/CLI payloads. In a **separate fresh Fedora fixture container**,
copy those programs to `/srv/tundra-package-fixture/tundra-shell` and `tundra-cli`,
and copy the source `packaging/`, `crates/ascii-assets/assets/`,
`crates/weathr/LICENSE.weathr`, and `LICENSE` under
`/srv/tundra-package-fixture/source/`. Copy the checker into the same fixture
folder, then execute it with container Python. The image includes rpmbuild and
dpkg-deb. The checker refuses to overwrite its existing work directory.

It builds the portable payload with its formal marker, builds and installs the
RPM inside that container, verifies dependency/provider/ownership/root-rejection
contracts and absence of PAM/session services or installation scripts, and
inspects the DEB metadata and payload. It does not install the DEB on Fedora.
These checks use stripped debug programs; release-profile builds and Debian
runtime validation remain separate CI checks in `scripts/package-linux.sh` and
the release workflows. No artifacts from this checker are release artifacts.
