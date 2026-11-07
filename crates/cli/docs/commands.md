# CLI commands and interaction

## Program entry points

```sh
tundra-shell
tundra-cli <command> [options]
```

`tundra-shell` with no arguments enters the full-screen UI; `--help` and `--version` print the requested information. `tundra-cli` provides standalone commands and can also use `ui` to send normal input to a Shell with external control enabled.

Shell/CLI help and version queries do not start watchdog, read configuration, or enter the full-screen interface.

## Public commands

| Command | Purpose |
| --- | --- |
| `doctor` | Check the system, terminal, permissions, paths, storage, and assets |
| `config` / `config get [field]` | View all public configuration or a specified field |
| `config set <field> <value>` | Change public configuration |
| `config reset <field>` | Restore a field's default value |
| `config options [field]` | List the available values for fields |
| `launcher [list]` | List pinned applications and their status |
| `launcher pin <path>` | Check and pin an executable target |
| `launcher unpin <id>` | Remove a pinned entry while keeping the application files |
| `ui …` | Shell sessions, snapshots, and input control; see [UI control](ui-control.md) |
| `services` / `processes` / `packages` / `network` / `disks` | Linux management commands |
| `users` / `system-config` / `operations` | Account, configuration, and task commands |
| `firewall` / `ssh` / `backups` / `scheduled-tasks` | Firewall, SSH, backups, and scheduled tasks |
| `logs …` | Query, follow, and export logs |
| `repl` | Start the interactive command line; exit with `exit` or EOF |
| `debug asset …` | Inspect, render, or output embedded assets unchanged |
| `debug paths` / `debug explain` | View resolved paths or an explanation of startup boundaries |
| `debug ui-test` / `debug screen-keyboard` | Preview shared controls and the on-screen keyboard |
| `debug view-ui-style [1\|2\|3]` | Preview interface and animation styles |
| `debug test-rainbow-text` | Preview rainbow text |
| `debug test-frost` / `debug test-matrix` | Preview startup screens |
| `debug test-aa-style1/2/3` | Demonstrate AA styles |
| `debug test-watchdog-error/critical/panic` | Deliberately generate test failures |
| `cls` | Clear terminal history and visible contents |
| `help [command …]` | View command help |
| `new` | Reset Tundra user configuration and state |

Use `tundra-cli <command> --help` or `tundra-cli help <command>` for the exact arguments. For debug previews, see [Shell debug previews](../../shell/docs/previews.md).

## Configuration

`config set` supports:

- `automation.enabled`: `true` / `false`, controls external CLI input.
- `border-shape`, `border-color`, `accent-color`, `icon-mode`: appearance.
- `motion`: `full` / `reduced`.
- `animation-speed`: multiples of 25 from 50–200; default 100.
- `language`, `timezone`, `address`, `weather-location`: language, timezone, and weather location.
- `update-mode`: `release` / `beta` on Linux.

`theme` is a read-only summary. Restart an existing Shell after changing settings through the CLI; the external-control switch in Settings takes effect immediately. Use `auto` for `weather-location`, or `config reset weather-location`, to restore location selection based on timezone.

Common examples:

```sh
tundra-cli config set timezone Asia/Shanghai
tundra-cli config set border-shape rounded
tundra-cli config set accent-color "#38bdf8"
tundra-cli config set motion reduced
tundra-cli config set animation-speed 125
tundra-cli config set weather-location "Shanghai, China"
tundra-cli launcher pin "/home/user/My App/run.sh"
```

Launcher CLI and UI share target checks, rejecting missing files, symbolic links, and unsupported targets. Relative paths in the REPL follow the current session directory.

## Command recognition and completion

Command Line and standalone `repl` run system commands by default. Tundra UX commands use `/`:

```sh
ls -la
/help
/config set motion reduced
```

Normal input is still executed as a system command even if its name matches a UX command. After failure, a suitable `/` prefix may be suggested, but execution does not automatically switch to another command. Unix absolute paths can be entered directly; a path to an executable directly under the root directory can be quoted.

Tab shows or accepts completions; Right accepts the suggestion after the cursor. Typo correction only shows suggestions and does not change the input. System suggestions come from common commands, Git/Cargo subcommands, and background PATH scans, which read only file names and metadata.

Lookup locations are refreshed after `export PATH`, `set PATH`, `set PATHEXT`, or `cd`; old scan results cannot overwrite the new locations. PATH scans run in the background, and the input thread only reads the cache.

## Session directory and environment

System command sessions retain the working directory and exported environment variables:

```sh
export PROJECT_MODE=dev
cd "/path/with spaces"
echo "$PROJECT_MODE"
```

The Windows equivalents are `set PROJECT_MODE=dev` and `cd /d "C:\path with spaces"`. UX commands do not clear this state, and the Tundra main process and system environment configuration remain unchanged.

Linux/macOS select a valid Shell on first execution, and subsequent commands retain the session state; Windows uses cmd.exe. Unexported local variables, functions, aliases, and Shell options are not preserved across commands. Use `. script` or `call script.cmd` when a script needs to change the current environment.

Each embedded Command Line page has its own child terminal and directory. The prompt shows the current absolute path, and command-completion markers remain in scrollback. A normal `exit` closes the current page; the Back button only cleans up the current instance.

## Diagnostics and failure tests

`doctor` checks required runtime conditions and reports optional desktop, system-command, and graphics-protocol capabilities. Missing optional features produce WARN; a failed requirement returns 1. View path templates with `debug paths`.

`debug test-watchdog-error` and `critical` deliberately generate test reports. `debug test-watchdog-panic` triggers the real failure workflow. The outermost watchdog handles cleanup for standalone CLI; embedded Command Line uses an internal exit code to ask Shell to display the panic page.

## Data reset

`new` deletes user configuration and state. Before running it, use `debug paths` to identify and back up the directories, then enter exactly `RESET` at confirmation.

`repl --embedded` is an entry point specifically for Command Line. Embedded CLI uses exit code 75 to ask Shell to restore the terminal, finish tasks, reset storage, and restart.
