# script-runtime

`script-runtime` provides the backend for running Linux scripts and debugging with bashdb. Normal execution uses the interpreter, arguments, and directory specified by the caller; breakpoint debugging requires Bash 5.2 or newer.

## Session interface

`ScriptSession::spawn` creates a run, `command` receives a `RuntimeCommand`, and `poll` returns a `RuntimeEvent`:

- Startup, running, paused, expression results, exit, and failure;
- Continue, step, step into, step out, breakpoint, and expression commands;
- Pause locations, variables, call stacks, and breakpoint states.

Queries complete asynchronously through a private channel, without waiting for the debugger on the UI thread. Shell handles document saving, page instances, read-only state, Vim switching, and the interface.

`terminal()` provides a shared `PtyProcess`; keys and paste are sent according to the child program's current terminal modes. An output receiver can pass raw output to task history chunk by chunk.

`into_output` cleans up the process, PTY, and debugging temporary directory, returning terminal output with scrollback and resizing support; `into_final_snapshot` returns the last screen.

## bashdb integration

The scripts and license for the pinned bashdb 5.2-1.2.0 are compiled into the program and written to a private temporary directory during debugging. Source and local fixes are in `third_party/bashdb`; ordinary Cargo builds do not download the debugger.

User scripts run from their original paths; BASH_ENV only loads the debugger, preserving `$0`. Debugger commands use a separate FIFO, and output uses a separate file, without occupying the user script's stdin/stdout/stderr. Variables and arrays are parsed as Bash declarations, without executing variable values.

bashdb does not provide a complete list of executable lines, so a breakpoint is marked as verified only after execution actually pauses at that location. Locations rejected by the debugger are shown as Not active; the rest remain Pending verification. A new debugging run resets these states.

## Validation

```sh
cargo test --locked -p script-runtime
python3 crates/script-runtime/tests/native_bridge.py
```

Real tests cover interpreter arguments and input, exit codes, function breakpoints, variables, arrays, `source` files, expressions, parallel sessions, and child-process cleanup. See [Script workspace](../../docs/source-editor.md#linux-script-workspace) for interface operations.
