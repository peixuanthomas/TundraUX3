# Platform paths, identity, and system integration

## Application data paths

| Purpose | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Configuration | `%APPDATA%\TundraUX3\config.toml` | `~/Library/Application Support/TundraUX3/config.toml` | `$XDG_CONFIG_HOME/TundraUX3/config.toml`, default `~/.config/TundraUX3/config.toml` |
| State | `%LOCALAPPDATA%\TundraUX3\state` | `~/Library/Application Support/TundraUX3/state` | `$XDG_DATA_HOME/TundraUX3/state`, default `~/.local/share/TundraUX3/state` |
| Cache | `%LOCALAPPDATA%\TundraUX3\cache` | `~/Library/Caches/TundraUX3` | `$XDG_CACHE_HOME/TundraUX3`, default `~/.cache/TundraUX3` |
| Logs | `%LOCALAPPDATA%\TundraUX3\logs` | `~/Library/Logs/TundraUX3` | `$XDG_STATE_HOME/TundraUX3/logs`, default `~/.local/state/TundraUX3/logs` |
| Temporary files | `%TEMP%\TundraUX3` | `TundraUX3` under the system temporary directory | `$XDG_RUNTIME_DIR/TundraUX3`; if unavailable, a private `/tmp` directory containing the UID |

Run `tundra-cli debug paths` to view path templates and resolved absolute paths.

## Linux startup identity

Shell and CLI check UID/EUID and GID/EGID before accessing storage or entering fullscreen mode.

| Startup identity | Behavior |
| --- | --- |
| Normal user | Read the current UID's NSS record and use its HOME, Shell, supplementary groups, and XDG environment |
| root | Show a privilege warning; when stdin and stderr are both terminals, press lowercase `y` to continue |
| Mismatched set-ID credentials | Refuse to start |

Identity comes from NSS, not USER, LOGNAME, Tundra roles, or account enumeration. Linux uses `LinuxCurrentProcess`; Windows/macOS use `LocalAccount`.

On Linux, `linux-uid-<UID>` stores personal preferences such as appearance, dashboard, and clock settings. The home page opens after the initial appearance settings are saved; an interrupted setup resumes on the next start. Normal users do not read old data from `/root`; a confirmed root session uses root's NSS HOME and `linux-uid-0`.

Personal directories are read from NSS HOME and `user-dirs.dirs`, with support for Chinese characters, spaces, and the convention of using HOME to disable a directory. The local interface can still open without XDG_RUNTIME_DIR, logind, or D-Bus; the affected system features report their unavailability separately.

## System services and authorization

Linux management, account, and power requests use fixed interfaces:

- Management writes use Shell's private authorization connection; the interface does not accept arbitrary commands.
- Accounts use AccountsService or fixed system tools; power operations use logind.
- Other callers can handle polkit through `pkttyagent` bound to a process and its start time.
- Normal users changing their own profile or password use the existing system authorization and `passwd` flows.
- Cancellation, timeout, and disconnection do not automatically replay writes.

D-Bus operations distinguish permission denial, canceled authorization, service unavailability, busy state, network errors, disconnection, and unknown results. See [Authorization and helper connections](authorization.md) for detailed connection rules, and [Portable updates](../../app/docs/update.md) for application updates.

## Desktop and file integration

| Feature | Windows | Linux |
| --- | --- | --- |
| Open a file or URI with the default application | Native interfaces | `xdg-open`, executed in the background |
| Launcher | Native programs and shortcuts | ELF, AppImage, scripts, and validated `.desktop` files |
| Clipboard | Native backend | Wayland data-control or X11/XWayland |
| Local volumes and trash | Native backend | mountinfo/statvfs/sysfs and Freedesktop Trash |
| Critical errors | Platform alerts and logs | Desktop notifications, watchdog reports, and stderr |
| Reboot/shutdown | Native APIs | logind + polkit |

The volume list includes local fixed disks and mounted removable filesystems, filtering out network and pseudo filesystems. Btrfs subvolumes are traced back to their actual block devices; labels show the device name and mount path.

Trash operations use the current user's permissions and check private directories and symbolic links. Ordinary text and image files follow the file-opening configuration even if they have the executable bit set; ELF, scripts, AppImage, EXE, and `.desktop` files go to Launcher.

Run `tundra-cli doctor` to check the terminal, `/proc`, `/sys`, management commands, D-Bus, polkit, and desktop integration. See [Linux runtime notes](../../../docs/packaging/linux/README-LINUX.txt) for dependencies.

## Migrating from Windows to Linux

1. Close the application on both systems, run `tundra-cli debug paths` on Windows, and back up configuration and state.
2. Run the same command on Linux and copy the backups to the displayed config and state paths respectively, keeping the original backups.
3. Global settings can be reused; appearance and clock settings from old local accounts are not automatically bound to a Linux UID.
4. Windows absolute paths need to be selected again; Launcher and recent files initially show Missing.
