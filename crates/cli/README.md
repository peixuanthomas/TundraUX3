# cli

`cli` provides `tundra-cli`: configuration, diagnostics, logs, Linux operations commands, Shell interface control, and an interactive command line. It also hosts update and authorization helper entry points; these internal entry points are not part of the public scripting interface.

## Common entry points

| Usage | Example |
| --- | --- |
| Commands from an external terminal | `tundra-cli doctor` |
| View help | `tundra-cli help`, `tundra-cli users --help` |
| View version | `tundra-cli --version` |
| UX commands in the built-in Command Line | `/doctor`, `/config get` |
| System commands in the built-in Command Line | `ls -la` or `dir` |

Normal REPL input is passed to the system command interpreter; Tundra commands use the `/` prefix. The session retains its working directory and exported environment variables without changing the Tundra main process or system environment configuration.

## Modules and responsibilities

- `src/arguments.rs` parses public arguments and configuration fields.
- `src/commands/` groups commands such as asset, config, debug, doctor, launcher, logs, management, and ui.
- `src/management_client.rs` reuses platform's management connection.
- `src/repl/` provides the interactive command line, completion, and embedded-host protocol.
- `src/runner.rs` coordinates configuration, storage, watchdog, and command execution.

Help and version queries print built-in text directly without starting watchdog or reading configuration. `doctor` is a top-level command; the former top-level debug entry points are grouped under `debug`.

## Shell interface control

`tundra-cli ui` can list the current user's Shell sessions, read interface snapshots, and send clicks, moves, drags, scrolling, keys, and text. This feature is disabled by default; enable it in **Settings → System → External CLI control**.

```sh
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui wake
```

Results use protocol version 1 JSON. Multiple sessions require `--session`; snapshots must be read again after real user input, and timed-out input is not automatically resent. See [UI control](docs/ui-control.md).

## Documentation

- [Commands, configuration, and REPL](docs/commands.md)
- [Operations commands, authorization, and exit codes](docs/operations.md)
- [UI control and JSON protocol](docs/ui-control.md)
- [Log queries, following, and export](../runtime-log/docs/logs.md)
- [Debug interface previews](../shell/docs/previews.md)

## Build and validation

```sh
cargo build --locked -p shell -p cli
cargo test --locked -p cli
```

Shell's built-in Command Line uses `tundra-cli` from the same directory, so rebuild both Shell and CLI after updates. Real Linux PTY validation of UI control uses `python3 scripts/tests/shell-control.py target/debug/tundra-shell`.
