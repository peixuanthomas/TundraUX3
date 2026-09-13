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
