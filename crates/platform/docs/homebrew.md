# macOS Homebrew package management

Home → Packages on macOS supports Homebrew Formulae (command-line software and libraries) and Casks (macOS applications), reusing the Installed, Updates, Search, and Details pages.

Homebrew must be installed and initialized before use. The application searches for an executable in `PATH`, `/opt/homebrew/bin/brew`, and `/usr/local/bin/brew`, in that order.

## Operations and brew interfaces

| Page action | brew command |
| --- | --- |
| Installed | `brew info --json=v2 --installed` |
| Updates | `brew outdated --json=v2` |
| Search | `brew search --formula` / `brew search --cask` |
| Details | `brew info --json=v2 --formula/--cask -- <name>` |
| Install, remove, or upgrade selected packages | `brew install/uninstall/upgrade --formula/--cask -- <name>` |
| Upgrade all | `brew upgrade` |
| Refresh metadata | `brew update` |

Formulae and Casks use different target identities, so packages with the same name are not confused. Versions are reread before submission. Only package names and `owner/tap/name` are accepted; pinned packages do not offer selected-package upgrades.

Queries and ordinary operations set `HOMEBREW_NO_AUTO_UPDATE`; metadata updates only through the Refresh button. Automatic cleanup and orphaned-dependency removal are off by default. Search displays at most 2,000 items.

## Confirmation and tasks

Writes go through confirmation and AA, run as the current macOS user, and use Homebrew's native confirmation. System passwords for Cask installers are entered through a separate PTY, and input is not saved to history.

While a task runs, users can switch pages or keep it running in the background. Stopping and forced termination use the existing AA and child-process supervision flows. The Homebrew page does not offer Linux repository editing, configuration conflicts, or database repair; `tundra-cli packages` remains Linux-only.

The implementation uses [Homebrew JSON v2](https://docs.brew.sh/Querying-Brew) and [native commands](https://docs.brew.sh/Manpage).

## Validation

```sh
TMPDIR=/private/tmp cargo test --locked --offline -p platform management::packages --lib
```

Tests use temporary brew fixtures, covering type and version identity, fixed commands, failure results, native confirmation, hidden passwords, and stopping flows. Real Cask installation and system authorization are verified in a macOS test environment.
