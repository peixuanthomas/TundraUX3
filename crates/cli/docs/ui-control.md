# External CLI control

`tundra-cli ui` reads the interface of a running Shell and sends normal keyboard, mouse, and paste input. Page commands, confirmation dialogs, login, and system authorization remain handled by the existing interface.

## Enabling control and selecting a session

Enable **Settings → System → External CLI control** for immediate effect, or save the configuration before starting Shell:

```sh
tundra-cli config set automation.enabled true
tundra-shell
```

Read sessions and snapshots:

```sh
tundra-cli ui sessions
tundra-cli ui snapshot
tundra-cli ui snapshot --session SESSION_ID
```

A single session is selected automatically; multiple sessions require `--session`. Session IDs change on every startup, and only the same operating-system user can connect.

Control commands do not start watchdog, load or rewrite configuration and storage, or initialize logging. Disabling control or exiting Shell closes the connections.

## Input commands

Coordinates use terminal character cells, with `(0, 0)` at the top left. Read the target ID or rectangle from a snapshot before sending input; the uppercase names below are placeholders to replace.

| Command | Arguments |
| --- | --- |
| `sessions` | List sessions |
| `snapshot` | Read the control tree and character screen |
| `wake` | Close idle weather or the weather lockscreen; on ordinary pages, only read a snapshot |
| `click` | `--id TARGET_ID` or `--x COL --y ROW`, with optional `--button left\|right\|middle` and `--double` |
| `move` | `--id TARGET_ID` or `--x COL --y ROW` |
| `drag` | `--from-id/--from-x/--from-y` specifies the start, `--to-id/--to-x/--to-y` the end; optional `--duration MS` and `--button` |
| `scroll` | A target plus `--direction up\|down\|left\|right`; `--count N` defaults to 1 |
| `key KEY` | A Unicode character or named key, with optional `--modifiers` and `--phase press\|repeat\|release` |
| `text TEXT` | Paste text into the current focus; use `text -- TEXT` for text beginning with a hyphen |
| `text --stdin` | Read UTF-8 text from standard input, preserving newlines |
| `wait --id TARGET_ID` | Wait for `--state present\|absent\|actionable`; defaults to `actionable` |

Drag duration defaults to 300ms, with a range of 16–10000ms; scroll counts range from 1–1000. Modifiers are `ctrl,alt,shift,super,hyper,meta`.

Named keys are case-insensitive and include `enter`, `esc`, `tab`, `backtab`, `backspace`, `space`, `delete`, `insert`, arrow keys, `home`, `end`, `pageup`, `pagedown`, and `F1`–`F24`.

```sh
tundra-cli ui click --id TARGET_ID
tundra-cli ui key s --modifiers ctrl
tundra-cli ui key tab --modifiers shift
tundra-cli ui drag --from-id FROM_ID --to-x 40 --to-y 18 --duration 400
tundra-cli ui scroll --x 40 --y 18 --direction down --count 3
tundra-cli ui text "中文与 emoji 😀"
tundra-cli ui text -- --literal-text
tundra-cli ui wait --id TARGET_ID --state actionable --timeout 10000
```

`present` only requires the target to exist; `absent` requires it to disappear; `actionable` requires it to be enabled and able to receive input. Waiting only reads snapshots; it does not click the target or resend input.

## Snapshot numbers and timeouts

All commands except `sessions` support:

- `--session ID`: select a Shell session.
- `--snapshot NUMBER`: specify the number of a previously read snapshot.
- `--timeout MS`: total time limit for connection, response, or waiting; defaults to 5000ms.

After real keyboard input, clicks, drags, scrolling, pasting, or window resizing, subsequent control commands return `stale_snapshot` until `snapshot` or `wait` is run again. This check also applies when `--snapshot` is omitted.

Automatic redraws, animations, progress, the clock, background results, and simple pointer movement do not invalidate snapshots. Input sent by the external CLI itself does not count as real user input. Input is not automatically resent after a timeout; read a fresh snapshot to check its result.

## JSON output

Each result is one JSON line:

```json
{"protocol":1,"ok":true,"snapshot":{"session":"SESSION_ID","number":42,"page":"home","width":120,"height":30,"animation_running":false,"background_tasks_running":false,"nodes":[],"lines":[]}}
```

`nodes` contains `id`, `parent`, `role`, `name`, `text`, `rect`, `focused`, `selected`, `enabled`, `actionable`, `sensitive`, and `actions`. `rect` uses `x`, `y`, `width`, and `height`. Prefer node IDs for input targets; sensitive input returns only masked content.

`lines` is the text screen arranged by terminal row, without image pixels. `animation_running` and `background_tasks_running` describe status only; read the node fields to determine whether a target is actionable.

Session list:

```json
{"protocol":1,"ok":true,"sessions":[{"session":"SESSION_ID","pid":1234,"endpoint":"LOCAL_ENDPOINT"}]}
```

When control is disabled or Shell exits, it returns `{"protocol":1,"ok":true,"closed":true}` before disconnecting.

Error result:

```json
{"protocol":1,"ok":false,"error":{"code":"stale_snapshot","message":"Shell content changed after user input. Run tundra-cli ui snapshot again before acting."}}
```

Scripts should check `ok` and `error.code`, without depending on the translatable `message`. Normal completion exits with 0; argument errors with 2; connection, rejection, standard-input, or wait failures with 1.

| Error code | Meaning |
| --- | --- |
| `invalid_arguments` | Invalid arguments or request limits exceeded |
| `no_session` | No Shell available to connect to |
| `session_required` | Multiple sessions require `--session` |
| `session_not_found` | The specified session is unavailable |
| `unavailable` | Local connection or session directory is unavailable |
| `not_ready` | Shell has not published a screen yet |
| `stale_snapshot` | No fresh snapshot after user input, or an invalid snapshot number |
| `busy` | Another external input is still in progress |
| `target_unavailable` | The target does not exist or is not currently actionable |
| `unsupported_protocol` / `protocol_error` | Protocol version or response does not meet requirements |
| `timeout` | Response or target wait timed out |
| `stdin_error` | Standard input failed or is not valid UTF-8 |

Request and response types are defined by `shell-control`; connections are provided by `platform::shell_control`. `wait` only sends `snapshot` requests and does not need an additional server-side wait command.

## Validation

```sh
cargo test --locked -p cli --test ui_commands --test command_info
cargo test --locked -p cli --lib ui_commands_complete
python3 scripts/tests/shell-control.py target/debug/tundra-shell
```

Automated tests use temporary session directories and mock Shells to cover connections, JSON, session selection, input sent only once, waiting, and timeouts. Linux PTY tests validate real screens and input.
