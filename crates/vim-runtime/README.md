# vim-runtime

`vim-runtime` runs real Vim and synchronizes buffers, terminal frames, and save requests between Vim and the Tundra editor. Vim itself handles modes, editing keys, undo, registers, macros, windows, search, and Ex commands; this crate does not reimplement those features.

## Processes and communication

`VimHost` launches Vim through `terminal-runtime` using a Unix PTY or Windows ConPTY, with process cleanup supervised by watchdog. The bundled `bridge.vim` exchanges JSON lines over a local loopback connection authenticated with a random token.

A full Vim build with `+eval`, `+channel`, and `+timers` is required. Release packages provide `vim/vim` (`vim/vim.exe` on Windows) and `vim/runtime`; callers use `VimConfig` to specify paths, size, user configuration, and initial buffers.

Keys, paste, mouse, and focus use the shared `PtyProcess` interface. `set_cell_size` supplies the actual character-cell dimensions in pixels for image positioning and terminal queries.
The host sends a liveness probe with `request_ping`. Vim's channel callback echoes the probe ID through `VimPoll::ping_ids` without changing editor state or the command sequence.

## Buffer synchronization

- `open_buffer` imports text, encoding, line endings, BOM, trailing newline, cursor, and protection flags; the same `host_id` updates the existing buffer.
- `activate_buffer` switches the current buffer while preserving Vim state; `remove_buffer` removes documents closed by the host.
- `request_export` exports current content; request IDs and state snapshots confirm synchronization completion.
- Vim's `listener_add()` collects line changes for locating script breakpoints; host imports do not create user undo entries.
- `VimHost` is retained while display is suspended, preserving the process, registers, macros, and undo history.

`VimPoll::terminal` shares `Arc` snapshots by PTY output revision. Static frames are reused when there is no new output; output, size changes, and post-exit refreshes generate new snapshots.

## Controlled saving

`BufWriteCmd`, `FileWriteCmd`, and `FileAppendCmd` delegate write requests to the host, including the target path, buffer path, write range, append flag, and force flag.

After completing permission, external-change, configuration-diff, and asynchronous-save checks, the host replies with the result through `respond_save`. Only a successful response clears the modified flag; on failure, `:wq` stays in Vim. Read-only buffers reject editing and saving, and protected configuration does not enter ordinary swap, backup, or persistent undo files.

`request_close(false)` lets Vim check for unsaved changes; `:q!` and `:qa!` discard content according to Vim's rules. Before closing the host, export and handle saving or discarding first, then clean up the process.

## Validation

After building full Vim, run the real-runtime regressions:

```sh
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
cargo test --locked -p vim-runtime --test terminal_cache
```

Tests require a real PTY and a local loopback connection. They cover probe replies, buffer import, cross-buffer undo, controlled saving, `:wq`, forced discard, read-only protection, and static frames, output, resizing, and post-exit refreshes.

See [Bundled Vim](../../docs/packaging/vim.md) for building and packaging, and [Editor and script workspace](../../docs/source-editor.md#real-vim-mode) for user operations.
