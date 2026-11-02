# Windows packages

Launcher → Packages on Windows uses WinGet, offering Installed, Updates, Search, and Details, with support for installing, removing, and upgrading selected packages and upgrading all packages.

## Prerequisites

Install App Installer, which provides `winget.exe`, and ensure the current user can run WinGet in a terminal. On first use, check the version and source, then follow WinGet prompts to initialize and accept agreements:

```powershell
winget --version
winget search --id Microsoft.PowerToys --exact --source winget
```

Tundra always uses the `winget` source. If WinGet is missing, the source is unavailable, or agreements have not been accepted, the page shows the specific failure reason.

List parsing supports English and Simplified Chinese. Unrecognized headers and truncated package IDs or versions are rejected to avoid operating on the wrong target.

## Operations and permissions

Installed opens by default; Tab/Shift+Tab switches between Installed, Updates, and Search, and details show available actions. The Windows page does not offer Linux repository editing, database repair, or configuration-conflict features.

Package changes require AA confirmation, with output and exit results shown in the task window. AA handles approval within the application; Windows/UAC handles administrator privileges required by installers. Some installers open their own graphical windows.

Keep Tundra running during execution and wait for the task to finish. After failure, first check the actual state using the output and a refreshed list. Completed installations, removals, or upgrades are not automatically rolled back.

## Validation

```sh
cargo test --locked -p platform --lib winget
```

Run the read-only real-machine check in a regular Windows user's terminal after completing WinGet setup:

```powershell
cargo test --locked -p platform --lib windows_winget_readonly_smoke -- --ignored --nocapture
```

This case queries only the version, installed packages, search, details, and update lists. Verify real writes in a recoverable Windows environment, checking AA, UAC, installer windows, success and failure output, and list refreshes after changes separately.
