# terminal-runtime

`terminal-runtime` manages host-terminal modes and isolated child terminals. Command Line, external programs, Vim, and scripts share this implementation of PTY/ConPTY, input encoding, terminal parsing, and process cleanup.

## Modules

| Module | Responsibility |
| --- | --- |
| `session` | `TerminalGuard`, host-terminal modes, and color and graphics capability detection |
| `pty` | Child-process startup, resizing, output reading, and process-tree cleanup |
| `emulator` | WezTerm in-memory terminal, protocol replies, synchronized updates, and image state |
| `input` | Encode keys, paste, and mouse input according to child-terminal modes |
| `input_queue` | Bounded input queue and a watchdog-supervised writer thread |
| `snapshot` | Convert parsed results into renderable cells and shared snapshots |
| `ansi` | ANSI encoding of foreground colors |

Shell decides when to take over or hand back the terminal; this crate applies and restores settings, while UI only renders snapshots.

## Host terminal

`TerminalGuard` manages raw mode, the alternate screen, mouse, focus, bracketed paste, and enhanced key reporting. Normal exit, temporary handoff of the terminal, and catchable failures all restore modes enabled by the program.

Startup detection can read the terminal's base colors, default foreground/background, and true-color capability. Queries support fragmented replies and common terminators; rendering only reads cached results. WSL in Windows Terminal prefers win32-input-mode; other terminals use Kitty or xterm key reporting according to their capabilities.

## Child terminals

`PtyConfig`, `PtyProcess`, and `PtyExitStatus` are general process interfaces. The default configuration adds no application arguments or private environment variables; Shell decides how to launch the built-in CLI.

- `send_key`, `send_paste`, and `send_mouse` encode input according to the child-terminal modes active at send time.
- Mouse coordinates are converted from the host content area to child-terminal coordinates and sent using the Default, UTF-8, or SGR protocol.
- The input queue is capped at 4 MiB. Its sole writer thread is managed by watchdog and writes keys, protocol replies, mouse input, and paste in order.
- `resize` returns immediately when the size has not changed, avoiding redundant notifications and screen-revision changes.
- `into_output` cleans up the process tree and drains output, returning a `TerminalOutput` that supports scrollback, resizing, and snapshots.
- An output receiver can save the complete task history chunk by chunk in the reader thread, without the terminal's scrollback-line limit.

Unix uses process groups, and Windows uses Job Objects to clean up the corresponding child-process tree. macOS also terminates background process groups in the same terminal session, without affecting other terminal instances.

## Terminal parsing and snapshots

Child-program output enters the WezTerm in-memory terminal rather than operating on the host directly. Clipboard operations, titles, unknown OSC sequences, and file downloads are not passed to the host terminal.

`ui_snapshot_with_revision`, `ui_snapshot_if_changed`, and `ui_snapshot_after_exit` can generate UI snapshots directly. Static frames reuse an `Arc`, updating on output, resize, or post-exit refresh. Snapshots preserve per-cell styles, italics, and DECSCUSR cursor shapes.

Kitty keyboard negotiation supports presses, repeats, releases, and extended keys. Kitty, Sixel, and iTerm2 images are parsed into shared RGBA data and crop rectangles, with an image-decoding budget of 20 million pixels and 80 million bytes; text task history does not store image payloads. Synchronized updates publish the screen after the end sequence or a 200ms timeout.

## Command Line host

Shell launches `tundra-cli repl --embedded` from its own binary directory, using `xterm-256color` and 2,000 lines of scrollback. Each page instance has an independent PTY and continues reading output while covered.

Ctrl+C, Esc, and Ctrl+Shift+X are passed to the child CLI; after confirmation, the top-right Back button cleans up only the current instance. A normal `exit` returns to the original Launcher or Explorer, and the command-line host interprets the built-in CLI's special exit codes.

## Validation

```sh
cargo test --locked -p terminal-runtime
```

Input-encoding and protocol unit tests are in `tests/unit/input.rs`, `tests/unit/pty.rs`, and `tests/unit/terminal_keyboard_pty.rs`. Host-mode restoration and key-combination behavior also require validation with a real Unix PTY or Windows ConPTY.

Related documentation: [Shell sessions](../shell/docs/session.md), [Command Line](../cli/docs/commands.md), [Local crossterm patch](../../third_party/crossterm/TUNDRA_PATCH.md), [Local WezTerm patch](../../third_party/wezterm-term/TUNDRA_PATCH.md).
