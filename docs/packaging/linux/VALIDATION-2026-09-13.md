# Linux ordinary-user and Fedora update frontend validation record

Validation date: 2026-09-13. The working baseline was `master` at
`8504a7a9962a6b5d291ba436fbdc62aaa8a5c369`; no experimental branches were switched to, merged, or modified, and nothing was pushed.
See the delivery record for this work for implementation commits and final worktree status.

## Environment and isolation scope

- Local macOS: common code, the existing local identity model, UI, and workspace regressions.
- `x240s-test`: confirmed Fedora 43 KDE, x86_64, ordinary-user UID/GID 1001,
  Rust/Cargo 1.98, PackageKit 1.3.6, polkit 126.
- Source was synchronized only to `/home/x240s-test/tundra-validation/ordinary-user-8504a7a`;
  the commit under test, uncommitted diff, and per-file SHA-256 were recorded. Builds and temporary files used this validation directory.
- RPM writes, trusted test repositories, system authentication, and package installation all took place in explicitly named rootless Podman
  containers. The host's everyday Tundra installation was not replaced, and no host shutdown, reboot, or network changes were performed.
- Graphical authentication used a real KDE polkit agent, isolated Xvfb, and a Fedora login session inside the container;
  TTY authentication used a real PTY and the system pkttyagent. The host user's graphical authentication dialogs were not accessed.

## Validation results

| Scope | Result and evidence |
| --- | --- |
| Linux identity boundary | Ordinary-user startup: Appearance → Home on first launch, Home on subsequent launch; spoofing USER/HOME did not change NSS identity; the embedded Terminal's actual UID/GID and HOME/USER were correct. UID/EUID/GID mismatch validation had automated coverage; real UID 0, sudo startup, and missing NSS records were all rejected. |
| Missing system session services | A real PTY without system/session buses could still start and complete personal setup; missing services affected only the corresponding capabilities. |
| Update backend isolation | Unrecognized installations rejected replacement; production portable fixtures used real release markers and ran the new CLI helper; checks preventing crossover between SystemRpm and PortableUser passed. |
| Real PackageKit | Signed local test repository, pinned Tundra 1.0.0 → 2.0.0; trusted dependency preview; installed necessary runtime while retaining the old unrelated version. Success required both a successful Finished result and a new RPM query. No candidate was a normal result. |
| Cancellation and recovery | Called the official Cancel operation; late cancellation was not presented as rollback. Saved non-secret transaction clues, simulated loss of the final log write, and restarted PackageKit after the transaction finished; recovered the result through fresh history queries and RPM information without replaying installation. Service disconnection during a real transaction remained covered by the simulated backend. |
| Existing graphical agent | Real KDE authentication completed the update with no TTY password prompt; the actual RPM version matched expectations. The test policy observed two graphical authentications. |
| TTY agent | Success, Ctrl-C, incorrect password, agent crash, Shell interruption, EOF, and RPM installation failure after authentication paths passed; restored raw/alternate screen/mouse/focus/cursor and canonical/echo after exit. |
| Passwords and data ownership | Used only one-time credentials for the isolated test account; they did not enter Shell output, user state, or logs. Generated personal data belonged to UID 1000. Tundra neither read system passwords nor called PAM. |
| Production package definitions | Actual RPM spec build, dependency resolution, isolated installation, file ownership, pkttyagent provider package, and root-rejection checks passed; DEB control/content and portable-marker checks passed. No Tundra PAM, sudo runtime dependency, system account, or session/seat installation scripts. |
| Installed-program diagnostics | The ordinary user ran the installed doctor: UID/EUID/GID/EGID 1000, NSS/HOME/XDG, SystemRpm, and tundraux3 1.3.1-1 were correct; storage and resource checks passed. Missing runtime/session buses or power authorization produced warnings. |
| UI reuse | Update cards, scrollable preview confirmation, and restart entry reused existing components and Theme; new page logic had no Buffer drawing, hardcoded colors, or synchronous system calls. Keyboard, mouse hit detection, and focus reused shared layouts. |

Common and platform checks:

- `cargo fmt --check` and local `cargo check --workspace --locked`: passed.
- Full macOS workspace tests, serial: 86 test targets, 1396 passed, 0 failed, 2 already ignored.
- Full Fedora workspace tests, serial: 86 test targets, 1452 passed, 0 failed, 2 already ignored.
  The two native file/directory Trash round-trip tests were then explicitly run in an isolated XDG Trash; both passed.
- A small power-authorization fix was added after the full Fedora regression run; the final incremental run passed 51 targeted Linux
  tests, including 4 power tests, and workspace `cargo check`. Full-regression and incremental records were retained separately.
- The final program passed KDE graphical authorization, successful TTY authorization, Ctrl-C cancellation, and power-denial paths under an explicit polkit NO rule;
  the TUI recovered after power denial, and no shutdown occurred.
- The final native Shell build passed. The standard PTY smoke test passed: a single-character sentinel after 64 mouse events
  appeared within 0.011 seconds (the threshold remained 0.25 seconds), followed by completion of Appearance → Home and verification
  of full termios, alternate-screen, mouse, and cursor restoration after SIGTERM.
- Localization checks: 1906 call sites and 1896 en-US/zh-CN message pairs passed.
- Workflow YAML, Unix shell-script syntax, weathr dependency boundary, and `git diff --check`: passed.

## Reproducible scripts and evidence

- `scripts/tests/packagekit-fixture.py`: signed, fixed RPMs, dependencies, and unrelated packages.
- `scripts/tests/authorization-pty.py`: real Shell, system-agent, and PTY validation.
- `scripts/tests/package-artifacts.py`: production package definitions and installed RPM validation.
- `docs/scripts/tests/README.md`: isolated containers, operation order, prerequisites, and reset requirements.
- `scripts/linux-shell-smoke.py`: keyboard priority after 64 mouse events and terminal restoration on exit.

Raw logs are retained on the test machine in `/home/x240s-test/tundra-validation`:
`final-native-workspace-check.log`, `final-native-workspace-tests.log`,
`authorization-final-success.log`, `authorization-final-gui.log`,
`auth-pty-{cancel,denied,crash,interrupt,eof,package_error}.log`,
`packagekit-real-{preview,execute,current,recovery}.log`,
`final-package-artifact-check.log`, `final-installed-rpm-doctor.log`,
`final-native-power-tests.log`, `final-native-delivery-check.log`, `final-native-trash.log`,
`final-native-pty.log`, `final-authorization-{power_denied,cancel,success,gui}.log`.
The logs contain no real-user credentials.

## Validation limits and known conditions

- Package-content validation used natively built debug binaries after stripping; this does not establish a completed
  release-profile build. DEB validation checked only contents and did not claim Debian runtime validation on Fedora.
  Release workflows passed YAML and shell-syntax checks; no remote release or CI was triggered.
- There was no physical Windows machine in this round, and no real shutdown/reboot was performed. Power requests used only the fixed logind
  interface; tests covered authorization challenges, denial, cancellation, and the requirement not to replay uncertain outcomes.
- Default parallel macOS tests encountered four existing local HTTP wait timeouts in system-services;
  the full serial run passed. Setting NO_PROXY alone did not eliminate all parallel failures, so proxy
  configuration cannot be identified as the sole cause. These system-time tests and application code were not changed.
- The PTY sentinel initially included the first save and Home initialization in the input threshold; it was changed to separately measure
  an ordinary character in an existing field. Matching multiple characters in raw output was also misleading under Ratatui's incremental rendering, so the final check used
  a single-character marker absent from the initial screen, retaining the 250-millisecond threshold and separately asserting initial-setup completion.
- The existing logind signal-listener thread still recorded watchdog shutdown timeouts in some exit logs;
  process-exit and terminal-restoration checks passed. This round did not refactor that existing background-thread lifecycle.
- Rootless containers used test-only service-isolation overrides for polkit; these overrides were not included in
  application packages, and container validation was not treated as equivalent to host systemd service-sandbox validation.

logind chooses its own authorization action based on basic, multi-session, and inhibitor state. The frontend starts fallback only on its explicit
`InteractiveAuthorizationRequired` response and resends the same request at most once.
This follows systemd v258's
[logind authorization selection](https://github.com/systemd/systemd/blob/v258/src/login/logind-dbus.c) and
[polkit response handling](https://github.com/systemd/systemd/blob/v258/src/shared/bus-polkit.c).
