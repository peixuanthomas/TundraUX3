# TundraUX3 WezTerm terminal-core patches

This directory vendors `tattoy-wezterm-term` 0.1.0-fork.5 under its original MIT
license. The upstream package metadata and license are retained.

## Screen row ranges

`Screen::with_phys_lines` and `with_phys_lines_mut` use the logical ranges of
`VecDeque`, bounded by its occupied rows. The upstream manual slice offsets
could index the second ring-buffer slice using an absolute row and panic while
reading a scrolled terminal. Reads and mutations now preserve row order,
empty-range callbacks, and clipping at the available rows regardless of the
deque's storage split.

Regression cases in `tests/unit/screen.rs` cover wrapped ranges spanning both
slices, ranges confined to the second slice, empty ranges, and mutation bounds.

## Ordered embedding I/O

`TerminalConfiguration::use_threaded_writer()` defaults to the upstream threaded
writer. Embedders can select a synchronous adapter when they already own an
ordered input worker. Tundra selects that adapter and only enqueues while holding
its terminal-model lock; a watchdog-managed worker performs the actual PTY writes.
Keyboard input, query replies, focus, mouse and paste therefore share one order,
without blocking terminal-output parsing behind a full PTY input buffer.

The embedding regression tests in `crates/terminal-runtime/tests/unit/emulator.rs`
check immediate protocol output, and
`crates/terminal-runtime/tests/unit/input_queue.rs` tests physical write order,
overload/error reporting, closure, and output parsing during input backpressure.
Existing real-PTY and watchdog cleanup tests cover the resource lifetime and
retained-output behavior.

## Unicode stream boundaries

`Performer::flush_print` uses the existing Unicode grapheme segmentation with
the preceding visible cell as context when a print run continues its grapheme.
Combining marks, variation selectors, emoji ZWJ sequences and regional-indicator
pairs therefore survive arbitrary PTY read boundaries. The combined cell keeps
its original attributes, and its width is recalculated before advancing the
cursor or adjusting insert-mode cells. A wide grapheme that no longer fits in
the remaining columns is moved to the next line with clean padding cells.

The Unicode regressions in
`crates/terminal-runtime/tests/unit/emulator.rs` compare whole, every two-part
split and bytewise output, including CJK, accents, emoji, right-margin wrapping,
insert mode, and attributes changed between a base and its combining mark.
