# TundraUX3 vt100 patch

This directory vendors `vt100` 0.15.2 under its original MIT license.

TundraUX3 uses `ratatui` 0.29, which pins `unicode-width` 0.2.0. The upstream
`vt100` 0.16 releases contain the required scrollback fix but require
`unicode-width` 0.2.1 or newer, so they cannot be selected in the current
dependency graph.

`src/grid.rs` backports the `Grid::visible_rows` fix present in upstream
`vt100` 0.16.0:

- limit scrollback rows to the viewport height;
- use saturating subtraction when the scrollback offset exceeds that height.

`src/grid.rs` and `src/screen.rs` also support `CSI 3 J` (erase saved lines)
by clearing the retained scrollback rows and resetting the scrollback offset.

The regression tests live in
`crates/shell/src/session/command_line_runtime.rs`.

Tundra's embedded REPL additionally uses a private numeric CSI protocol:

- `CSI 777 ; 0 z`, immediately after a printed `○`, attaches a pending command
  marker to that cell (including when rustyline redraws the prompt).
- `CSI 777 ; 1 ; 0 z` completes the latest marker successfully;
  `CSI 777 ; 1 ; 1 z` records failure.

The marker metadata follows cells into scrollback and is removed when cells
are overwritten or erased. Completion also updates retained history. No cursor
movement, clipboard action, terminal title or host-side operation is performed.
Only these exact numeric forms are recognized; OSC strings remain filtered.
The private sequences are emitted only by `repl --embedded`. The numeric CSI
form lets rustyline calculate prompt width correctly on Unix and Windows.
The UI turns the metadata into themed spans; terminal output colors are retained.
Regression coverage is in `crates/shell/tests/unit/session/command_line_runtime/tests.rs`
and `crates/ui/tests/command_line.rs`, with real PTY coverage in
`scripts/tests/command-line-input.py`.
