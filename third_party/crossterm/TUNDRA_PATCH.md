# TundraUX3 crossterm input patch

This directory vendors the crates.io source for `crossterm` **0.29.0**, under
its original MIT license. Upstream repository:
https://github.com/crossterm-rs/crossterm (package VCS commit
`36d95b26a26e64b0f8c12edfe11f410a6d56a812`). The root workspace selects this copy
through `[patch.crates-io]`; no dependency version was upgraded. Text uses
LF line endings, Markdown trailing whitespace is normalized, and the patched
manifest requires Rust 1.85; the application workspace requires Rust 1.95.

## Why this patch exists

Upstream consumes both bytes of `ESC ESC` as a single Escape. When a user
presses Escape immediately before a mouse report, `ESC ESC [ < 35 ; x ; y M`
therefore becomes Escape followed by ordinary characters. In TundraUX3 the
last `M` activates Home's System Status shortcut after leaving Explorer.

Both Unix event sources now share `src/event/source/unix/parser.rs`. It wraps
the upstream decoder with framing and bounded recovery:

- Preserve the second Escape as the next report's prefix.
- Retain a lone Escape for up to **50 ms** to allow a split prefix to arrive;
  after that it is delivered as an ordinary Escape key.
- Accumulate fragmented reports, with a **250 ms idle timeout** and a
  **4,096-byte limit**. Bracketed paste uses a **5 s idle timeout** and a
  **1 MiB limit**, and its contents are never treated as navigation keys.
- Drop malformed, unsupported, timed-out, and oversized reports. After a
  timeout/overflow, discard the remaining report through its terminator;
  a fresh Escape can resynchronize non-opaque reports. Paste/control strings
  require their closing delimiter. This deliberately favors dropping ambiguous
  trailing input over executing it as a shortcut.
- Bound the underlying poll by the pending report deadline so diagnostics are
  emitted even when no further bytes arrive. Preserve caller timeouts and
  nonblocking poll behavior in both Mio and `use-dev-tty` backends. Read only
  once per readiness notification, including on blocking terminal descriptors;
  re-arm Mio so unread or rejected backlogs cannot stall subsequent input.
- Reject zero/overflow coordinates and extra mouse/cursor-position fields.
  Keep upstream's optional trailing semicolon for SGR/rxvt mouse reports.
- Provide a metadata-only `event::set_input_error_handler` callback. Shell
  sends it to the existing bounded runtime logger at Warning level, with
  `UX_TERMINAL_INPUT_MALFORMED`, `UX_TERMINAL_INPUT_INCOMPLETE`, or
  `UX_TERMINAL_INPUT_TOO_LONG`. No raw keyboard, password, or paste bytes are
  included. The callback must not reenter event reading or print to the terminal.

Legacy terminals encode an Escape key and an escape-sequence prefix with the
same byte. A continuation delayed past the Escape deadline is inherently
ambiguous; this patch does not claim to identify the user's intent in that case.
Windows continues to use its native console event decoder.

The Windows decoder also preserves standalone Shift, Ctrl, Alt and CapsLock
press/release records for the screen keyboard's physical-key feedback. Scan
codes distinguish left/right Shift; the enhanced-key bit distinguishes right
Ctrl/Alt, including on release. Alt-code character completion is unchanged.
These regressions are in `src/event/sys/windows/parse.rs` and require Windows.

## Verification

The added parser regressions are in `tests/escape_input.rs`; the original
library tests remain present. The package's separate lockfile pins its test
dependencies and does not add them to the application's dependency graph.

```sh
cargo test --manifest-path third_party/crossterm/Cargo.toml --locked --lib --tests --target-dir target/crossterm-tests -- --test-threads=1
cargo test --manifest-path third_party/crossterm/Cargo.toml --locked --lib --tests --features use-dev-tty --target-dir target/crossterm-tests -- --test-threads=1
cargo test --manifest-path third_party/crossterm/Cargo.toml --locked --lib --tests --features libc,use-dev-tty --target-dir target/crossterm-tests -- --test-threads=1
cargo test --workspace --locked
# On Linux, with the built shell:
python3 scripts/linux-shell-smoke.py target/debug/tundra-shell
```

The Linux PTY smoke exercises Explorer → Escape + mouse → Home, rejection of
incomplete/malformed reports, warning logs on disk, subsequent valid shortcuts,
pointer release behavior, and terminal restoration. Shell's log metadata has a
separate regression in `crates/shell/tests/unit/terminal_input_logging.rs`.

When updating crossterm, review the upstream parsers against these regressions
before removing or rebasing this patch.

## Upstream review: 2026-10-12

The crates.io latest stable version is still **0.29.0**. Reviewed upstream
`master` through [`101fa5dabd409fd02a4468cd997bfe28cde64073`](https://github.com/crossterm-rs/crossterm/commit/101fa5dabd409fd02a4468cd997bfe28cde64073)
(2026-10-10). This copy remains based on the published 0.29.0 source, with the
following selected fixes backported; it is not a full snapshot of master.

| Upstream change | Included behavior |
| --- | --- |
| [#1024](https://github.com/crossterm-rs/crossterm/pull/1024), `014db9ab` | Discard old cursor-position replies before sending a fresh query; adapted to 0.29's internal event functions. |
| [#1026](https://github.com/crossterm-rs/crossterm/pull/1026), `c02a080a` | Open `/dev/tty` for writing when querying keyboard flags, so redirected stdout does not receive the query. |
| [#1040](https://github.com/crossterm-rs/crossterm/pull/1040), `2df28f7a` | Fully qualify `Result::Ok` in `queue!`, so a caller's `Ok` name cannot break the macro. |
| [#1069](https://github.com/crossterm-rs/crossterm/pull/1069), `ee68df63` | Disabled color commands emit nothing, rather than a bare reset that clears text attributes. |
| [#1100](https://github.com/crossterm-rs/crossterm/pull/1100), `08d5799a` | Converting an underline color to `Colors` no longer changes the background. |
| [#1136](https://github.com/crossterm-rs/crossterm/pull/1136), `d903fb75` | Read terminal dimensions from `/dev/tty`, stdout, stderr, then stdin; return an error if none is a terminal, without launching `tput`. Adapted to retain the optional libc backend as well as rustix. |

Also backported two **open, unmerged PRs**, reviewed at these exact revisions:

- [#1073](https://github.com/crossterm-rs/crossterm/pull/1073),
  `954acceea63028799cd9657b266a5647418e55d3`: ignore surrogate key-up records
  during ordinary Windows emoji input. Preserve the separate Alt-code release
  path and standalone modifier feedback.
- [#1103](https://github.com/crossterm-rs/crossterm/pull/1103),
  `a2a3e048b5b8b6a91575598f5e28716bd7826594`: correct the Windows UTF-16
  translation buffer from a two-element array to sixteen zeroed elements.

Upstream [#1049](https://github.com/crossterm-rs/crossterm/pull/1049) addresses
coordinate underflow already covered here by checked subtraction. Keep rejecting
invalid coordinates rather than clamping them into valid clicks. The bounded
local parser already drops finished unknown CSI reports (the problem described
in [#1106](https://github.com/crossterm-rs/crossterm/pull/1106)). Its existing
zero-timeout and readiness handling also remains in place; do not replace it
with the open polling/draining PRs without rechecking blocking descriptors.

Deliberately omitted unrelated new APIs, dependency/CI churn and master changes
that remove `IsTty`, the `windows` feature, or the libc backend. The application
dependency version and root lockfile remain unchanged.

Additional regressions: `tests/no_color.rs` runs in an isolated process;
`tests/terminal_queries.rs` checks macro name shadowing and uses Python 3 plus a
private Linux PTY to check redirected keyboard queries and stale cursor replies.
The upstream size tests use private PTYs and isolated child processes for each
standard descriptor and the no-terminal case. Windows library tests cover
emoji down/up sequences, Alt-code completion and existing modifier events.

The manifest explicitly registers the two integration tests because automatic
test discovery is disabled. Use serial library tests because the original
`Colored` tests change a shared process-wide color flag and can interfere with
one another in parallel runs.

Verified on 2026-10-12:

- WSL Ubuntu: workspace and vendored `cargo fmt --check`; full
  `cargo test --workspace --locked` (2,025 passed, 16 ignored).
- WSL Ubuntu: vendored default, `use-dev-tty`, `libc,use-dev-tty`, and
  all-features tests. Final all-features run: 129 library tests plus three
  integration tests passed; helper child tests are invoked by their PTY parents.
- Windows MSVC: 74 library tests plus two integration tests passed,
  including emoji releases, Alt-code completion and modifier feedback;
  ten existing tests remained ignored.
- Built Shell and CLI together and passed `scripts/linux-shell-smoke.py` with
  the resulting binary, including input recovery, resize and terminal restoration.

These are automated Windows/WSL results; macOS, physical touch devices and
manual Windows emoji-panel interaction were not tested.
