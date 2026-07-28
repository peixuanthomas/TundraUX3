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
