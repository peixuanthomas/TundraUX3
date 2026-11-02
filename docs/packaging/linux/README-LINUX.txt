TundraUX3 Linux x86_64 runtime guide
====================================

Running the portable build
--------------------------
Extract the archive and keep both binaries, assets/, vim/, and
tundra-installation.json together. Open a terminal in that directory and run:

  ./tundra-shell
  ./tundra-cli doctor

The archive includes the full Vim engine and runtime, the GNU GPL v3 license,
and the Weathr component license. Vim's upstream license and source details are
in vim/LICENSE and vim/UPSTREAM.json.

Desktop integrations use standard systemd/Freedesktop sessions on GNOME or KDE,
under Wayland or X11.

Runtime dependencies
--------------------
Required desktop helpers:

  - xdg-open from xdg-utils
  - gio from libglib2.0-bin on Debian/Ubuntu or glib2 on Fedora/Arch

Vim mode also needs libtinfo.so.6 from the ncurses runtime, usually provided by
libtinfo6 on Debian/Ubuntu.

For desktop integration, install and run a session D-Bus bus,
xdg-desktop-portal, and polkit. XWayland may be needed for clipboard access when
the Wayland compositor does not provide a data-control clipboard.

`tundra-cli doctor` reports missing helpers and the affected integration. Local
terminal applications can still be used when desktop services are unavailable.

User accounts and permissions
-----------------------------
Start Tundra as your normal Linux user. The launching process UID selects the
NSS account, HOME directory, login shell, and personal profile. File operations,
launched programs, Trash, and settings use that user's operating-system
permissions.

Shell and CLI reject mismatched real/effective UIDs or GIDs before opening the
interface. Root startup displays a warning and requires lowercase `y` to
continue; any other key cancels. Both stdin and stderr must be terminals.

The first launch for a UID opens Appearance setup. Completed setups open Home
directly, and interrupted setups resume on the next launch. Linux uses the
system account directly and does not add an application password or lock screen.

User Management uses AccountsService, polkit, and libxcrypt (libcrypt.so.1).
Ordinary users can edit their own account. AccountsService administrators can
create, edit, lock, unlock, and delete other local login accounts. Deleting an
account keeps its home directory; the account running Tundra cannot delete,
lock, or demote itself.

Privileged writes use the system polkit agent, with pkttyagent as a foreground
terminal fallback when available. Password prompts are handled by the system
agent. Changing your own password runs /usr/bin/passwd with the Tundra interface
suspended.

Package management
------------------
Launcher supports APT/dpkg, DNF, and pacman through AutoAdmin. System package
management and Tundra self-update are separate features.

On Arch, installed-package queries read existing pacman metadata. Repository
queries also require /usr/bin/pacman-conf and readable sync databases with
supported repository Usage settings. Foreign packages can be listed and removed;
AUR helpers and builds are not included.

Pacman install, selected-package upgrade, and full upgrade use -Syu --needed to
avoid partial upgrades. Removal uses -R without recursive or cascade options.
Review .pacnew and .pacsave files reported by pacman after a transaction.

Updating Tundra
---------------
Updates require a writable portable directory owned by the current user, both
binaries, and tundra-installation.json. Existing DEB/RPM installations are not
modified.

Settings -> Update provides two modes:

  - Release downloads the latest stable Linux x86_64 archive and verifies its
    SHA-256 checksum. A Rust toolchain is not needed.
  - Beta downloads a specific master commit and builds it with the local Rust
    toolchain. It also needs Python 3.12+, a C compiler, make, and ncurses
    development headers.

Both modes stage Shell, CLI, and the complete Vim directory together. Personal
data and themes are preserved. The previous executables and engine remain
available until the new Shell starts successfully; failed or interrupted
updates are restored on the next launch.

The current update protocol is v3. Updaters from protocol v2 cannot install v3
packages. When upgrading from those versions, fully extract the new portable
archive once.

Diagnostics and development
---------------------------
`tundra-cli doctor` reports UID/GID, NSS identity, HOME, XDG paths, D-Bus,
logind, polkit, pkttyagent, and portable installation status. Diagnostics only
read the environment and do not elevate privileges.

For development validation, run from the repository root:

  cargo fmt --check
  cargo check --workspace --locked
  cargo test --workspace --locked

Build a portable archive on Linux x86_64 with:

  python3 scripts/build-vim.py --output target/release/vim
  bash scripts/package-linux.sh

Linux-specific tests run on Linux. Use the PTY and isolated-systemd scripts in
scripts/tests/ for terminal and privileged-service checks. Keep package write
tests and root startup tests in disposable environments.
