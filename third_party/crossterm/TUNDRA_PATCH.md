# TundraUX3 crossterm input patch

This directory vendors the crates.io source for `crossterm` **0.29.0**, under
its original MIT license. Upstream repository:
https://github.com/crossterm-rs/crossterm (package VCS commit
`36d95b26a26e64b0f8c12edfe11f410a6d56a812`). The root workspace selects this copy
through `[patch.crates-io]`; no dependency version was upgraded. Text uses
LF line endings, Markdown trailing whitespace is normalized, and the patched
manifest requires Rust 1.85 to match the project toolchain.

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
cargo test --manifest-path third_party/crossterm/Cargo.toml --locked --lib --target-dir target/crossterm-tests
cargo test --manifest-path third_party/crossterm/Cargo.toml --locked --lib --features use-dev-tty --target-dir target/crossterm-tests
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
