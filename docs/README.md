# Documentation index

[English](../README.md) · [Simplified Chinese](README.zh-CN.md)

## Getting started and development

| Topic | Documentation |
| --- | --- |
| Features, installation, and interface controls | [Chinese README](README.zh-CN.md) · [English README](../README.md) |
| Architecture, building, testing, and packaging overview | [Technical documentation](README-TECHNICAL.md) |
| Automated tests, real terminals, and platform acceptance checks | [Testing guide](testing.md) |
| Interface interaction and acceptance criteria | [UI requirements](UI-requirements.md) |
| Linux runtime dependencies and portable updates | [Linux runtime notes](packaging/linux/README-LINUX.txt) |
| Building and releasing bundled Vim | [Bundled Vim](packaging/vim.md) |

## Feature guides

| Topic | Documentation |
| --- | --- |
| Standard editor, Vim, and script debugging | [Editor and script workspace](source-editor.md) |
| Editor computation and benchmarks for large files | [Editor large-file support](editor-scalability.md) |
| List multiselection behavior on each page | [List multiselection](list-selection.md) |
| Linux services, packages, networks, and disks | [Linux system management](../crates/platform/docs/linux-management.md) |
| CLI commands, diagnostics, and UI control | [CLI guide](../crates/cli/README.md) |
| Built-in applications and Launcher | [Application guide](../crates/app/docs/applications.md) |
| Log queries and export | [Log guide](../crates/runtime-log/docs/logs.md) |
| Language resources and recovery | [Localization guide](../crates/i18n/docs/localization.md) |

## Crate documentation

| Crate | Responsibilities and documentation |
| --- | --- |
| [shell](../crates/shell/README.md) | Pages, navigation, input, dialogs, and full-frame composition |
| [app](../crates/app/README.md) | Application state, files, editor, Launcher, and updates |
| [ui](../crates/ui/README.md) | Shared controls, layouts, themes, rendering, and animation |
| [cli](../crates/cli/README.md) | External commands, REPL, diagnostics, and UI control |
| [platform](../crates/platform/README.md) | File, process, package, and operating-system interfaces |
| [terminal-runtime](../crates/terminal-runtime/README.md) | Host and child terminals |
| [vim-runtime](../crates/vim-runtime/README.md) | Vim processes, buffer synchronization, and saving |
| [auto-admin](../crates/auto-admin/README.md) | Authorization, password input, and administrative tasks |
| [storage](../crates/storage/README.md) | Configuration formats, atomic saves, migration, and recovery |
| [identity](../crates/identity/README.md) | System users and local application accounts |
| [system-services](../crates/system-services/README.md) | System and weather snapshots, background services |
| [runtime-log](../crates/runtime-log/README.md) | Logging, queries, and incident reports |
| [watchdog](../crates/watchdog/README.md) | Process and task supervision, recovery records |

See the [complete responsibility table](README-TECHNICAL.md#crate-responsibilities) for the remaining crates. Implementation details are in `crates/<crate>/README.md` or its `docs/` directory.

## Historical records

The following files preserve the state at the time and are not current usage guides:

- [CHANGELOG](CHANGELOG.md) and [release notes](packaging/releases/v1.5.md)
- [Linux administration acceptance record](linux-administration-validation.md)
- [Operations usability acceptance record](operations-usability-validation.md)
- [Historical Linux portable-build validation](packaging/linux/VALIDATION-2026-09-18.md)
