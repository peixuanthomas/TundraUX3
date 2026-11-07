# TundraUX3

**A terminal workspace for managing your device.**

English | [Simplified Chinese](docs/README.zh-CN.md)

TundraUX3 brings system tools, files, editors, and everyday utilities into one terminal interface. It is designed for Linux devices without a graphical desktop, with keyboard, mouse, and touchscreen support. Written in Rust with Ratatui and crossterm.

## Features

- **Linux system management:** manage services, processes, packages, networks, disks, users, firewall rules, SSH, and scheduled tasks. Review configuration history and restore previous changes.
- **Cross-platform packages:** search, install, remove, and upgrade software using native Linux package tools, Homebrew on macOS, or WinGet on Windows.
- **Files and editing:** browse directories, share copied or cut files between pages, undo same-filesystem moves and renames, and manage permissions. Edit text with syntax highlighting, search and replace, soft wrap, and JSON formatting. Press F8 to switch between the built-in editor and Vim.
- **Development tools:** run programs in an embedded terminal and debug Bash scripts with breakpoints, stepping, variables, and call stacks.
- **System activity:** monitor resource usage, inspect logs and notifications, and track background tasks and their output.
- **Personal workspace:** search Launcher by name or command, organize custom entries, choose themes and languages, and use the built-in clock and weather apps.

| Platform | Main support |
| --- | --- |
| Linux | Device operation and system management |
| macOS | Terminal workspace and Homebrew package management |
| Windows | Terminal workspace and WinGet package management |

## Get started

Use an interactive terminal of at least **108 columns × 20 rows**. Mouse and touch input require a terminal that forwards pointer events.

### Run a Linux portable build

1. Download and extract the Linux x86_64 archive from [GitHub Releases](https://github.com/peixuanthomas/TundraUX3/releases).
2. Keep the extracted files together and open a terminal in that directory.
3. Start the application:

```sh
./tundra-shell
```

To check the runtime environment:

```sh
./tundra-cli doctor
```

Run TundraUX3 as your normal user. Administrative operations ask for authorization when needed.

### Build from source

Install **Rust 1.95 or newer**. Building the bundled Vim also requires **Python 3.12 or newer** and a native C build toolchain.

From the repository root:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

On Windows, use `python` if `python3` is unavailable. After pulling updates, rebuild both Shell and CLI; rebuild Vim when `packaging/vim.json` changes.

To run the environment check from a source build:

```sh
cargo run --locked -p cli --bin tundra-cli -- doctor
```

## Using the interface

Open supported system tasks directly from **Home**, or open the remaining applications from **Launcher**. Applications on Home are not repeated in Launcher. Common file actions and editor tools stay visible. Management pages offer an expert area with F3 that stays open when you select another item.

Home cards show application names without letter shortcuts. Click a card, or select it with the arrow keys and press Enter/Space.

| Input | Action |
| --- | --- |
| Arrow keys / Tab | Move between items and controls |
| Enter / Space | Open, confirm, or toggle |
| Esc / Back button | Return or close the current dialog |
| F1 | Show help for the current page |
| F2 | Organize Launcher entries |
| F4 | Configure how an external Launcher item runs |
| F8 in the editor | Switch between the standard editor and Vim |

On the dashboard, arrow keys move between cards by their visual position, including mixed-size and staggered layouts.

Mouse and touchscreen users can click or tap controls, scroll lists, and drag scrollbars. An on-screen keyboard is available for text fields.

In management lists, Ctrl/Cmd+click toggles individual selections, Shift+click selects a range, and Shift+Enter toggles the current row. Tab/Shift+Tab moves between regions and controls; arrow keys operate the focused region. See [list selection](docs/list-selection.md).

Use the title-bar **Tasks** button to view running work or approval requests, read saved output, return to the related page, or cancel a task. Press it again to close Tasks and return to the previous page. Language, appearance, animations, weather, and application options are available in **Settings**.

Linux updates default to Release with release notes and download progress. Beta adds build progress and saves complete compiler output for troubleshooting; see [application updates](crates/app/docs/update.md).

Linux uses ASCII icons by default. If your terminal supports images, you can switch to image icons in **Settings → Appearance → Theme**.

## Companion CLI

`tundra-cli` provides diagnostics, configuration, logs, system-management commands, and control of a running interface:

```sh
tundra-cli --help
tundra-cli doctor
```

To control the interface from another terminal, enable **Settings → System → External CLI control**, then use commands such as:

```sh
tundra-cli ui snapshot
tundra-cli ui wake
```

See the [UI control guide](crates/cli/docs/ui-control.md) for click, input, scrolling, and automation examples.

## Documentation

- [Simplified Chinese README](docs/README.zh-CN.md)
- [Build, architecture, testing, and packaging](docs/README-TECHNICAL.md)
- [Linux system management](crates/platform/docs/linux-management.md)
- [Editor and script debugging](docs/source-editor.md)
- [CLI commands and operations](crates/cli/docs/operations.md)
- [Documentation index](docs/README.md)

## License

The project code is licensed under [GPL-3.0-only](LICENSE). Bundled third-party components retain their respective licenses.
