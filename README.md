# TundraUX3

**Visual device control, entirely in the terminal.**

English | [简体中文](docs/README.zh-CN.md)

TundraUX3 helps you operate a device when only a terminal is available and there is no graphical desktop. It brings menus, buttons, lists, forms, and system information into a terminal interface, making everyday tasks easier to discover and carry out without memorizing every command.

The project has two priorities: **practical Linux system control** and **independent operation with a keyboard, a mouse, or a touchscreen**. Each input method should be sufficient on its own, so users can work with whichever their device provides.

TundraUX3 is written in Rust, using Ratatui and crossterm. It is under active development; some workflows and system integrations are still incomplete.

## Platform focus

| Platform | Role |
| --- | --- |
| **Linux** | The main target for device operation and system management in terminal-only environments. |
| **Windows and macOS** | A demonstration platform for exploring the interface and interaction design. |

System-management development focuses on Linux. Windows and macOS builds let you try the experience, but do not promise the same system controls. Available Linux operations depend on the installed tools, running services, and your user's permissions.

## What you can do

- **Show weather while idle:** Settings → System lets you choose Never, 30 seconds, or 1/2/5/10/30 minutes (default: 5 minutes). Press a key or tap to return to the same page without signing in again.

- **Let agents operate the interface:** enable external CLI control in Settings → System, then use `tundra-cli ui` to inspect the live UI tree and text screen, click, drag, scroll, or type. Connections are local and restricted to the same OS user. See [CLI control](crates/cli/docs/ui-control.md).

- **Manage a Linux device:** inspect and control services and processes, manage software packages, configure supported network connections, and inspect disks and mount points.
- **Browse packages:** open Installed by default, cycle Installed/Updates/Search with Tab or Shift+Tab, and select packages with Up/Down. Expanded details scroll with all four arrow keys; existing shortcuts remain available.
- **Find page actions:** common operations appear at the bottom right, with the main action highlighted and destructive actions in the danger color. F1 or the title-bar Help button opens page shortcuts.
- **Keep track of selections:** selected list rows and the current file shortcut stay highlighted when using a mouse or moving to another control. File Explorer quick access uses the same spacious rows as the application list in Settings.
- **Follow page changes:** pages, dialogs and menus expand from their entry button's border and shrink back to it with a mouse or touchscreen; keyboard actions are immediate. Details and the screen keyboard keep their existing motion. Settings offers overall speed, phase times, reset and reduced motion.
- **Set up Linux:** the clock follows system time and timezone, including existing installations. Change timezone with the map in Region & Time, and manage system automatic synchronization and timesyncd/chrony servers there. Operating System Settings contains the host name.
- **Manage access and schedules:** inspect listening ports, manage UFW/firewalld allow rules, configure SSH with confirmation from a new connection, and manage systemd timers and existing cron schedules.
- **Review and recover changes:** inspect grouped configuration history and restore selected operations, including local accounts and groups.
- **Work with files:** browse directories with the path directly above search, perform file operations with a single confirmation dialog for deletion, and edit plain text with syntax highlighting, search and replace, soft wrap and JSON formatting. F8 switches to the bundled real Vim, with rainbow toggle text when enabled. The editor opens 106 source and configuration suffixes by default. File Explorer, Editor, Launcher, Settings, System Status, and Clock use matching themed borders. System configuration changes use a diff, a configuration check, an authorized save and recovery copies in the existing editor.
- **Manage Linux file access:** edit permissions and ownership for selected files, with separate file/directory permissions and a reviewed recursive range. Toggle permission boxes with Space or Enter.
- **Open tools:** use the application launcher and built-in command line when a task calls for commands. Move between launcher icon rows with Up/Down and between adjacent items with Left/Right.
- **Understand system activity:** view system status, resource usage, logs, notifications, and diagnostics.
- **Make the interface your own:** choose English or Simplified Chinese, adjust appearance, and use the clock and weather applications. Application Settings lists File Explorer and Editor on the left and shows the selected application's options on the right. Cancel new alarms with the visible button or keyboard.

See [Linux system management](crates/platform/docs/linux-management.md) for supported operations and their requirements.

Vim mode displays Vim's own terminal interface, including Visual modes, counts, registers, macros, windows, command line and `:help`. `Ctrl+W` operates Vim windows. F8 returns to the ordinary editor without saving the document; F8 again resumes the same Vim process and its existing state. Document writes such as `:w` and `:wq` use Tundra's existing save workers and protected-configuration checks; `:q!` follows Vim's discard behavior. See the [editor guide](docs/source-editor.md#真实-vim-模式).

## Keyboard, mouse, or touchscreen

Independent use of all three input methods is a core design requirement:

| Input | Intended experience |
| --- | --- |
| **Keyboard alone** | Move between controls, select actions, enter text, and navigate with keys and shortcuts. |
| **Mouse alone** | Use visible buttons, menus, selectors, and scrollbars without depending on keyboard shortcuts. |
| **Touchscreen alone** | Tap controls and drag scrollbars without needing a physical keyboard or mouse. |

Support is still being improved. Mouse and touch need a terminal that forwards pointer events. Running AA dialogs, embedded Command Line, management forms and management search share a screen-keyboard button at the bottom right, above the status and clock bar. Opening it animates the page title down to one row, hides the bottom bar, and resizes content above the keys. Paste and Hide occupy normal-sized keys at the bottom-left and bottom-right corners. AA keeps its dimmed background and title unchanged and moves only its dialog above the keys. These transitions follow the configured animation speed and reduced-motion setting. Other application forms need a physical keyboard or an input method supplied by the terminal/system. The external-terminal `tundra-cli debug screen-keyboard` command remains a standalone QWERTY and AA input demo. See [keyboard input](crates/shell/docs/session.md#aa-与命令行的屏幕键盘) and [preview details](crates/shell/docs/previews.md).

Click the message area of the bottom status bar to read its full text in a popup. Long messages wrap and can be scrolled; the clock keeps its separate action. File Explorer keeps the specific error reason in the status bar when you change the selection.

AA output includes a draggable scrollbar for reviewing retained output while a task runs and after it finishes.

## Get started

Use an interactive terminal with at least **108 columns × 20 rows** for the default interface. Some applications, including the built-in command line, need more vertical space. Mouse and touch support depend on the terminal in use.

Shrinking a running window pauses interaction behind a size reminder and preserves your work until the window is enlarged. First-use setup and management forms support **F10** to finish; search fields show when **Enter** submits a query.

### Run a Linux portable build

1. Download the Linux x86_64 portable archive from [GitHub Releases](https://github.com/peixuanthomas/TundraUX3/releases).
2. Extract it into a directory owned by your normal Linux user.
3. Keep `tundra-shell`, `tundra-cli`, `assets/`, `vim/`, and `tundra-installation.json` together.
4. Open a terminal in the extracted directory and run:

```sh
./tundra-shell
```

To check the environment:

```sh
./tundra-cli doctor
```

Linux uses the account that launched the program and that account's system permissions. Run it as your normal user; operations that need additional permission request authorization through the relevant system tools. Exiting TundraUX3 closes the application and returns to your terminal.

Linux releases use portable archives. In **Settings → Update**, **Release** downloads a stable portable release, while **Beta** builds the latest `master` commit with a local Rust toolchain. Self-updates require a writable, user-owned portable installation and preserve personal data and themes. See the [Linux runtime notes](docs/packaging/linux/README-LINUX.txt) for dependencies and update details.

For the first upgrade from an older build using update protocol v2, fully extract the new portable archive once. Those updaters reject protocol v3, which also deploys the bundled Vim runtime.

### Build from source

Install Rust **1.95 or newer** with Cargo and the build tools required by your platform. From the repository root:

```sh
cargo build --locked -p shell -p cli
python3 scripts/build-vim.py --output target/debug/vim
cargo run --locked -p shell --bin tundra-shell
```

Rebuild Shell and CLI after pulling updates, and rerun the Vim step when `packaging/vim.json` changes. `cargo run -p shell` alone does not rebuild the companion `tundra-cli` executable used by Command Line.

The Vim step explicitly downloads and builds the pinned upstream version; ordinary Cargo builds do not download Vim. It needs Python 3.12+ and native C build tools; Windows can use `python` instead of `python3`. Build prerequisites, release paths and offline source archives are described in [bundled Vim](docs/packaging/vim.md). `TUNDRA_VIM` can select another compatible full Vim executable; `TUNDRA_VIM_CLEAN=1` disables personal Vim configuration and plugins for isolated tests.

Explore the companion CLI:

```sh
cargo run --locked -p cli --bin tundra-cli -- --help
cargo run --locked -p cli --bin tundra-cli -- doctor
```

Preview shared controls with `tundra-cli debug ui-test` in a terminal of at least 88 × 30 cells. F1–F6 switch pages; Esc exits. It uses built-in sample data, theme and English text without reading or writing configuration, language files, logs or saved state.

The [shared UI library](crates/ui/README.md) exposes `animate` for value and Ratatui layout/style animations, including its attribute macro. Animated pages share frame snapshots and skip unnecessary image checks to reduce CPU work.

Run `tundra-cli debug test-rainbow-text` to preview flowing rainbow text. Rainbow text keeps flowing at a fixed speed regardless of animation settings or button state. Esc or the Exit button closes the demo; it uses built-in samples without saving settings.

Build both packages: the shell uses the companion CLI for the built-in command line and Linux management tasks. For build, test, and packaging details, see the [technical documentation](docs/README-TECHNICAL.md).

## Current limits

- The terminal interface is the focus. Features that launch graphical applications or use desktop services still require those applications or services; they may be unavailable in a terminal-only session.
- Linux management features have specific requirements, such as systemd for service control and supported distribution tools for package management. Operating-system settings explain missing systemd or system DBus capabilities in the failure details.
- Network operations are in Launcher → Network. The unavailable Sound, Display, Wi-Fi and Bluetooth settings pages have been removed; account management is in Launcher → Users.
- Input coverage and real-device behavior are still being refined. The project does not claim complete support for every terminal, touchscreen, or Linux distribution.
- Vim runs as a terminal program. Linux has real PTY regression coverage; the Windows and macOS builds and runtime checks require their corresponding CI runners. Linux validation does not establish results on those platforms.

## Documentation

Most detailed documentation is currently in Chinese. Start with the [documentation index](docs/README.md); each crate keeps its own implementation guide.

- [简体中文 README](docs/README.zh-CN.md)
- [V1.4.1 release notes](docs/packaging/releases/v1.4.1.md) — page help, visible actions, and interaction fixes.
- [Technical documentation](docs/README-TECHNICAL.md) — architecture, building, testing, and packaging.
- [Source editor](docs/source-editor.md) — search, replace, soft wrap, JSON formatting, and supported text formats.
- [Linux system management](crates/platform/docs/linux-management.md) — services, processes, packages, networks, and disks.
- [Operations CLI](crates/cli/docs/operations.md) — reusable commands, authorization, task IDs and exit codes.
- [System configuration editor](crates/platform/docs/system-config.md) and [user management](crates/platform/docs/user-management.md).
- [Configuration backups](crates/platform/docs/config-backups.md), [firewall and ports](crates/platform/docs/firewall.md), [SSH management](crates/platform/docs/ssh-management.md), and [scheduled tasks](crates/platform/docs/scheduled-tasks.md).
- [Linux runtime notes](docs/packaging/linux/README-LINUX.txt) — runtime dependencies, permissions, and portable updates (English).
- [Logs and diagnostics](crates/runtime-log/docs/logs.md)
- [Localization and resource recovery](crates/i18n/docs/localization.md)
- [Linux update tests](crates/app/docs/update-tests.md)

## License

Copyright (c) 2026 HanPeixuan. The project's own code is licensed under [GNU GPL v3 only](LICENSE) (`GPL-3.0-only`). The Weathr component retains its [GPL-3.0-or-later license](crates/weathr/LICENSE.weathr). Check the applicable component and third-party asset licenses when redistributing or reusing them.

Bundled Vim retains its upstream Vim License in `vim/LICENSE`, with its source and version recorded in `vim/UPSTREAM.json`.
