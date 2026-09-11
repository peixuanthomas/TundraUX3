# Standalone Linux user-session verification

These tests run the installed UX in a PTY and inspect the real system identity.
They do not start a display manager, switch a VT, create a PAM session, or request
privileged system actions.

Run from the repository as an ordinary SSH login user. `python3-dbus` is required
for the independent logind comparison; the PTY and root tests use the Python
standard library only.

```sh
python3 scripts/linux-shell-smoke.py /usr/bin/tundra-shell \
  --assets crates/ascii-assets/assets
python3 scripts/linux-user-session-smoke.py ordinary target/debug/tundra-cli
sudo python3 scripts/linux-user-session-smoke.py root-refusal /usr/bin/tundra-shell
```

`--assets` copies the specified matching resources into the temporary user
profile. It never repairs the source tree or installed resources. Omit this flag
to test the installed resource layout as well. Every XDG directory is isolated;
the real account HOME is retained. Temporary resources and profiles are removed
after the PTY test.

The PTY implements a text-terminal status response and acknowledges the expected
ASCII-icon fallback notice. It waits for the real appearance setup screen, moves
focus to the existing custom-theme control, queues 64 mouse events, and checks
that Space opens its dialog within 250 ms. It checks real/effective/saved/filesystem
UID and GID, supplementary groups, generated file ownership, and absence of
`/root/` paths in generated TOML/JSON configuration. SIGTERM must exit successfully
and restore terminal flags, mouse capture, alternate screen and cursor.

The identity test compares CLI JSON with NSS and independently queries the current
process's logind session over the system bus. It requires `Remote=true` and no
managed Tundra desktop. The root test traces fork, vfork, clone and exec events:
root must fail before any descendant or second exec, with the explicit ordinary
user requirement and without entering fullscreen mode. This dynamically verifies
that root rejection does not launch sudo or a watchdog.

## Fedora hardware evidence

Validated on `x240s-test`, Fedora 43, SELinux Enforcing, on 2026-09-11 around
15:43 UTC:

- Ordinary PTY: UID/GID 1001/1001, 13 generated user-owned profile files, 64 queued
  mouse events consumed in 1 ms and the keyboard sentinel visible in 12–14 ms
  across the initial and supplementary-group verification runs.
  Terminal restoration and normal SIGTERM exit passed.
- CLI: username `x240s-test`, HOME `/home/x240s-test`, shell `/bin/bash`, UID/GID
  1001/1001. CLI and independent logind lookup agreed on SSH session `380`;
  `managed` was `null`. Session IDs vary with each new SSH connection.
- Root: exit code 1, one exec (the tested shell), zero forks/vforks/clones, and
  `Tundra UX requires an unprivileged, non-setuid user process` diagnostic.

Tested executable SHA-256 values:

```text
44fbd74e9fa1abc08ddc6c562c3bef41e61448770167ce9318392baeb3e952e6  /usr/bin/tundra-shell
1c4bd2817c3123745b7f16b9ab8a1c93ea178a390129bff42aaf3d09a6977239  target/debug/tundra-cli
```

The initial test without `--assets` correctly surfaced a resource recovery modal:
the host's old `/usr/share/tundraux3/assets` lacked or mismatched four English
locale files from the new build. The ordinary process could not rewrite those
root-owned files. Matching copied resources were therefore used for the UX
validation above. Package installation must supply matching resources before an
installed-layout test can pass.

This evidence covers a standalone SSH user session. PAM login, credential
establishment, trusted greeter isolation, managed desktop environment, locking,
switching users and consent require the separate physical-seat integration
tests; this smoke does not claim to validate those paths.
