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
| **Windows && MacOS** | A demonstration platform for exploring the interface and interaction design. |

System-management development focuses on Linux. Windows and macOS builds let you try the experience, but do not promise the same system controls. Available Linux operations depend on the installed tools, running services, and your user's permissions.

## What you can do

- **Manage a Linux device:** inspect and control services and processes, manage software packages, configure supported network connections, and inspect disks and mount points.
- **Work with files:** browse directories, perform file operations, and edit plain text. System configuration changes use a diff, a configuration check, an authorized save and recovery copies in the existing editor.
- **Open tools:** use the application launcher and built-in command line when a task calls for commands.
- **Understand system activity:** view system status, resource usage, logs, notifications, and diagnostics.
- **Make the interface your own:** choose English or Simplified Chinese, adjust appearance, and use the clock and weather applications.

See [Linux system management](docs/linux-management.md) for supported operations and their requirements.

## Keyboard, mouse, or touchscreen

Independent use of all three input methods is a core design requirement:

| Input | Intended experience |
| --- | --- |
| **Keyboard alone** | Move between controls, select actions, enter text, and navigate with keys and shortcuts. |
| **Mouse alone** | Use visible buttons, menus, selectors, and scrollbars without depending on keyboard shortcuts. |
| **Touchscreen alone** | Tap controls and drag scrollbars without needing a physical keyboard or mouse. |

Support is being improved across pages. Mouse and touch interaction require a terminal that forwards the corresponding pointer events. Text entry in application forms still needs an input method, such as a physical keyboard or a terminal/system-provided on-screen keyboard. An English QWERTY on-screen keyboard demo is available through `tundra-cli debug screen-keyboard` in an external terminal. It fills the lower half of the terminal with keys that adapt to its width, including digits, symbols, F1–F12, Shift, Tab, CapsLock, Ctrl, Alt and right Ctrl. Modifier keys stay lit until clicked again and can be combined with subsequent keys. Each character key shows only the character it currently types, updating with Shift and CapsLock; taller terminals use taller keys. Hide/Show slides the keyboard at the saved global animation speed and respects reduced motion. Physical keys light their matching keycaps while the keyboard is visible. The Test AA button opens a harmless AA input popup; its Show keyboard button slides the keyboard up from the bottom. The final compositor moves the popup into the remaining space, keeping it on screen and above the keyboard, which may cover the status and time bars. Small terminals preserve the popup and ask for a larger window. It displays typed text and key combinations, and offers Hide/Show, Copy, Paste, Clear and Exit buttons. Copy copies all text and Paste appends clipboard text. It is not yet connected to application forms or embedded Command Line. Complete mouse-only or touch-only text-entry workflows therefore depend on the surrounding environment.

Click the message area of the bottom status bar to read its full text in a popup. Long messages wrap and can be scrolled; the clock keeps its separate action.

## Get started

Use an interactive terminal with at least **108 columns × 20 rows** for the default interface. Some applications, including the built-in command line, need more vertical space. Mouse and touch support depend on the terminal in use.

### Run a Linux portable build

1. Download the Linux x86_64 portable archive from [GitHub Releases](https://github.com/peixuanthomas/TundraUX3/releases).
2. Extract it into a directory owned by your normal Linux user.
3. Keep `tundra-shell`, `tundra-cli`, `assets/`, and `tundra-installation.json` together.
4. Open a terminal in the extracted directory and run:

```sh
./tundra-shell
```

To check the environment:

```sh
./tundra-cli debug doctor
```

Linux uses the account that launched the program and that account's system permissions. Run it as your normal user; operations that need additional permission request authorization through the relevant system tools. Exiting TundraUX3 closes the application and returns to your terminal.

Linux releases use portable archives. In **Settings → Update**, **Release** downloads a stable portable release, while **Beta** builds the latest `master` commit with a local Rust toolchain. Self-updates require a writable, user-owned portable installation and preserve personal data and themes. See the [Linux runtime notes](docs/packaging/linux/README-LINUX.txt) for dependencies and update details.

### Build from source

Install a current stable Rust toolchain with Cargo and the build tools required by your platform. From the repository root:

```sh
cargo build --locked -p shell -p cli
cargo run --locked -p shell --bin tundra-shell
```

Repeat both commands after pulling updates. `cargo run -p shell` alone does not rebuild the companion `tundra-cli` executable used by Command Line.

Explore the companion CLI:

```sh
cargo run --locked -p cli --bin tundra-cli -- --help
cargo run --locked -p cli --bin tundra-cli -- debug doctor
```

Build both packages: the shell uses the companion CLI for the built-in command line and Linux management tasks. For build, test, and packaging details, see the [technical documentation](docs/README-TECHNICAL.md).

## Current limits

- The terminal interface is the focus. Features that launch graphical applications or use desktop services still require those applications or services; they may be unavailable in a terminal-only session.
- Linux management features have specific requirements, such as systemd for service control and supported distribution tools for package management. Missing components are reported in the interface.
- Network operations are in Launcher → Network. The unavailable Sound, Display, Wi-Fi and Bluetooth settings pages have been removed; account management is in Launcher → Users.
- Input coverage and real-device behavior are still being refined. The project does not claim complete support for every terminal, touchscreen, or Linux distribution.

## Documentation

Most detailed documentation is currently in Chinese.

- [简体中文 README](docs/README.zh-CN.md)
- [Technical documentation](docs/README-TECHNICAL.md) — architecture, building, testing, and packaging.
- [Linux system management](docs/linux-management.md) — services, processes, packages, networks, and disks.
- [Operations CLI](docs/operations-cli.md) — reusable commands, authorization, task IDs and exit codes.
- [System configuration editor](docs/system-config.md) and [user management](docs/user-management.md).
- [Linux runtime notes](docs/packaging/linux/README-LINUX.txt) — runtime dependencies, permissions, and portable updates (English).
- [Logs and diagnostics](docs/LOGS.md)
- [Localization and resource recovery](docs/LOCALIZATION.md)
- [Linux update tests](docs/scripts/tests/README.md)

## License

Copyright (c) 2026 HanPeixuan. The project's own code is licensed under [GNU GPL v3 only](LICENSE) (`GPL-3.0-only`). The Weathr component retains its [GPL-3.0-or-later license](crates/weathr/LICENSE.weathr). Check the applicable component and third-party asset licenses when redistributing or reusing them.
