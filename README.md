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
| **macOS** | Interface exploration and Homebrew package management for Formulae and Casks. |
| **Windows** | WinGet package management, with other system controls remaining demonstration features. |

System-management development focuses on Linux. Windows supports software packages through WinGet, and macOS through Homebrew; other system controls on those platforms do not promise the same coverage as Linux. Available operations depend on the installed tools, running services, and your user's permissions.

## What you can do

- **Show weather while idle:** Settings → System lets you choose Never, 30 seconds, or 1/2/5/10/30 minutes (default: 5 minutes). Press a key or tap to return to the same page without signing in again.

- **Let agents operate the interface:** enable external CLI control in Settings → System, then use `tundra-cli ui` to inspect controls, click, drag, scroll, or type. User operations require a new snapshot; animations and automatic updates do not. Use `tundra-cli ui wake` to close idle weather or the weather lockscreen. Connections are local and restricted to the same OS user. See `tundra-shell --help` and [CLI control](crates/cli/docs/ui-control.md).

- **Manage a Linux device:** inspect and control services and processes, manage software packages, configure supported network connections, and inspect disks and mount points. Services, processes and scheduled tasks support Shift+click or Shift+Enter for individual selections and batch actions; F1 explains the controls.
- **Browse packages:** open Installed by default, cycle Installed/Updates/Search with Tab or Shift+Tab, and move through packages with Up/Down. Shift+click or Shift+Enter toggles individual selections; install, remove and upgrade act on all selected packages after review. F1 explains selection controls. Expanded details scroll with all four arrow keys.
- **Manage macOS packages:** use Homebrew to search, install, remove and upgrade Formulae and Casks, with its default confirmation behavior and task-terminal input for native prompts. See [Homebrew support](crates/platform/docs/homebrew.md).
- **Manage Windows packages:** use the same Packages page with WinGet to search, install, uninstall, upgrade one package, or upgrade all available packages from the `winget` source. App Installer and an initialized WinGet source are required; see [Windows packages](crates/platform/docs/windows-packages.md).
- **Find page actions:** common operations appear at the bottom right, with the main action highlighted and destructive actions in the danger color. F1 or the title-bar Help button opens page shortcuts.
- **Keep track of selections:** selected list rows and the current file shortcut stay highlighted when using a mouse or moving to another control. File Explorer quick access uses the same spacious rows as the application list in Settings.
- **Follow page changes:** pages, dialogs and menus expand from their entry button's border and shrink back to it with a mouse or touchscreen; keyboard actions are immediate. Animations target 30 FPS to reduce CPU use while retaining their duration. Moving pages, panels and keyboards show their borders first; text appears together when the transition ends. Settings offers overall speed, phase times, reset and reduced motion.
- **Set up Linux:** the clock follows system time and timezone, including existing installations. Change timezone with the map in Region & Time, and manage system automatic synchronization and timesyncd/chrony servers there. Operating System Settings contains the host name.
- **Manage access and schedules:** inspect listening ports, manage UFW/firewalld allow rules, configure SSH with confirmation from a new connection, and manage systemd timers and existing cron schedules.
- **Review and recover changes:** inspect grouped configuration history and restore selected operations, including local accounts and groups.
- **Work with files:** browse directories with the path directly above search, perform file operations with a single confirmation dialog for deletion, and edit plain text with syntax highlighting, search and replace, soft wrap and JSON formatting. F8 switches to the bundled real Vim, with an enabled/disabled status label and rainbow text when enabled. The editor opens 106 source and configuration suffixes by default. File Explorer, Editor, Launcher, Settings, System Status, and Clock use matching themed borders. System configuration changes use a diff, a configuration check, an authorized save and recovery copies in the existing editor.
- **Manage Linux file access:** edit permissions and ownership for selected files, with separate file/directory permissions and a reviewed recursive range. Toggle permission boxes with Space or a click.
- **Organize Launcher:** use F2 to add custom shell commands and drag icons into a saved order. Right-click a user entry to change its name, command or icon, or remove it; click outside the menu to close it. Custom commands and embedded terminal programs also offer a **Keep screen after exit** switch in this menu; system-launched programs do not. New custom commands use the same on/off switch, off by default; when on, the last screen stays until you press Enter. Text icons use a fixed frame with two editable rows of five terminal cells. Shift+Left/Right also reorders; the mouse wheel and PageUp/PageDown change pages.
- **Open tools:** run TUI applications in the built-in command line, or select an external Launcher item and use **Run settings (F4)** to save an embedded terminal, arguments and working directory. Embedded terminals support enhanced keyboard input and Kitty, Sixel and iTerm2 images, with character-block previews on text-only terminals. Closing a terminal stops its process tree. Move between launcher icon rows with Up/Down and between adjacent items with Left/Right.
- **Edit and debug Linux scripts:** Script Editor reuses the text editor and bundled bashdb, with breakpoints, stepping, variables, call locations, expressions and interactive output in a tdb-style workspace. Run with the script's interpreter; debug with Bash 5.2 or newer. See the [script editor guide](docs/source-editor.md#linux-脚本工作区).
- **Understand system activity:** view system status, resource usage, logs, notifications, and diagnostics. UX logs combine events and incidents in one list, with incidents at the highest severity and all levels shown by default. Filter by minimum level or open the log folder in File Explorer. Normal refreshes keep the panels stable.
- **Make the interface your own:** choose English or Simplified Chinese, adjust appearance, and use the clock and weather applications. In Settings, Tab/Shift+Tab cycles the main categories, Up/Down selects options, and Left/Right moves between the application list and its settings. Enter or Space changes a switch; Enter or a click opens a number dialog for adjustment or direct input, followed by Enter to confirm. AA policy and screen timeout use selection dialogs. Application Settings lists File Explorer and Editor on the left and shows the selected application's options on the right. Cancel new alarms with the visible button or keyboard.

See [Linux system management](crates/platform/docs/linux-management.md) for supported operations and their requirements.

Use **Tasks** in Launcher to find running work, read saved output, return to its page or request cancellation. Esc or the title-bar Back button returns from saved task output to the task list. Scheduled tasks offer guided actions and daily, weekly or one-time schedules. Configuration backups have filters, 50-record pages, retained records and a cleanup preview. See the [operations acceptance notes](docs/operations-usability-validation.md) for checks and remaining device tests.

Vim mode displays Vim's own terminal interface, including Visual modes, counts, registers, macros, windows, command line and `:help`. `Ctrl+W` operates Vim windows. F8 or the VIM mode enabled button returns to the ordinary editor without saving the document; F8 again resumes the same Vim process and its existing state. Document writes such as `:w` and `:wq` use Tundra's existing save workers and protected-configuration checks; `:q!` follows Vim's discard behavior. See the [editor guide](docs/source-editor.md#真实-vim-模式).

If the Vim engine is missing, enabling Vim opens a cancellable installation dialog with download progress. It downloads the matching prebuilt engine from this project's `vim-v<version>` GitHub Release into your application data. The engine release must be published first; users do not need a compiler.

## Keyboard, mouse, or touchscreen

Independent use of all three input methods is a core design requirement:

| Input | Intended experience |
| --- | --- |
| **Keyboard alone** | Move between controls, select actions, enter text, and navigate with keys and shortcuts. |
| **Mouse alone** | Use visible buttons, menus, selectors, and scrollbars without depending on keyboard shortcuts. |
| **Touchscreen alone** | Tap controls and drag scrollbars without needing a physical keyboard or mouse. |

Mouse and touch need a terminal that forwards pointer events. Packages, services, processes, scheduled tasks and files have a visible **Select multiple** button, checkboxes and a selection count. The shared screen keyboard covers built-in text fields, including setup, login, accounts, settings, the ordinary editor, search/replace and file names and paths. Opening it leaves the current form and its submit/cancel buttons above the keys. AA and embedded Command Line use the same keyboard. Real-device coverage is recorded in the [operations acceptance notes](docs/operations-usability-validation.md); see [keyboard input](crates/shell/docs/session.md#aa-与命令行的屏幕键盘) for details.

Built-in text fields use a blinking underscore cursor, including the login password field. Input dialogs keep the active field in the accent color. Up/Down switches fields; Left/Right moves within the text. Number dialogs retain their −/+ buttons.

The QWERTY screen keyboard shows navigation keys beside the main keys when space permits. Smaller windows use **Fn / ABC** to switch between letters and function/navigation keys. Paste sends clipboard text as a paste event.

Click the message area of the bottom status bar to read its full text in a popup. Long messages wrap and can be scrolled; the clock keeps its separate action. File Explorer keeps the specific error reason in the status bar when you change the selection.

AA output includes a draggable scrollbar for reviewing retained output while a task runs and after it finishes.

## Get started

Use an interactive terminal with at least **108 columns × 20 rows** for the default interface. Some applications, including the built-in command line, need more vertical space. Mouse and touch support depend on the terminal in use.

Shrinking a running window pauses interaction behind a size reminder and preserves your work until the window is enlarged. Input forms use **Enter** to move to the next field and submit from the last field; switches use **Space** or a click. **Ctrl+Enter** submits directly, and default form buttons also provide **F10**. Search fields show when **Enter** submits a query.

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

Automated tests focus on core workflows, data safety, and permission boundaries. They do not make HTTP requests or start HTTP test servers; see the [testing guide](docs/testing.md).

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
- [Windows packages](crates/platform/docs/windows-packages.md) — WinGet setup, supported operations, and verification.
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
