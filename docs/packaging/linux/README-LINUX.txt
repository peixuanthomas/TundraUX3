TundraUX3 Linux x86_64 runtime requirements
============================================

Supported desktop sessions are regular systemd/Freedesktop sessions on GNOME or
KDE, under Wayland or X11.  Run the two binaries from a real terminal:

  ./tundra-shell
  ./tundra-cli debug doctor

The portable archive keeps `assets` next to the binaries.  Do not move the
binaries without moving that directory too.  It includes the root GNU GPL v3
and the Weathr component license. Future releases provide only portable archives.

Required: xdg-open (xdg-utils) and gio (libglib2.0-bin on Debian/Ubuntu,
glib2 on Fedora/Arch).  Recommended for full
desktop integration: a session D-Bus bus, xdg-desktop-portal, polkit, and
XWayland when the Wayland compositor does not expose a data-control clipboard.

Use `tundra-cli debug doctor` after installation.  It reports missing optional desktop
services and gives the relevant package/service hint; a missing desktop helper
degrades only the affected integration, never stored data.

V1.3.3 release
--------------
This release provides only the Linux x86_64 portable archive, built on Ubuntu
24.04. Extract it and run ./tundra-shell from the extracted directory. Keep
assets/ and tundra-installation.json beside the binaries. No DEB or RPM
installer is published for V1.3.3.

For local builds, scripts/package-linux.sh produces only a portable archive.

Ordinary Linux user session
--------------------------
Log in normally through Fedora, then start tundra-shell in that user's terminal.
Shell and CLI reject mismatched real/effective UIDs and GIDs before storage,
recovery, watchdog, or fullscreen terminal initialization. Root execution shows
a warning: file operations and launched programs have root privileges and may
modify or delete system files. Press lowercase y to continue (no Enter needed);
any other key cancels. Both stdin and stderr must be terminals; piped input
cannot confirm. The prompt temporarily uses raw mode and restores it afterward.
Every root startup requires confirmation; ordinary-user startup has no prompt.
It never retries startup through sudo or su.

The current process UID is resolved directly through NSS. USER, LOGNAME, saved UX
roles, and account enumeration do not select an identity. NSS supplies HOME and
the login shell; valid XDG paths are honored with standard fallbacks. Explorer,
Editor, Launcher, embedded Terminal, child processes, Trash, and personal settings
use the current user's operating-system permissions. Permission failures remain
permission failures. Ordinary-user runs do not read or migrate old /root data.
Confirmed root runs use root's NSS HOME, valid XDG paths and linux-uid-0 profile.

On first use, the current UID's profile opens Appearance. After its preferences
and completion marker are saved, Home opens. Interrupted setup resumes next time;
completed profiles enter Home directly. Weather remains a separate application.
The Linux UI has no application password login, screen lock, user switching, or system logout.
Exit closes only Tundra. User Management requires AccountsService, polkit and
libxcrypt (libcrypt.so.1). Ordinary users see and edit only their own account.
AccountsService administrators can create, edit, lock/unlock and delete other
local login accounts, including changing User/Admin status. Root, service and
remote accounts are excluded. Locking affects password login only; existing
sessions and key-based login are not terminated. Deletion keeps the home
directory and files; the
account running Tundra cannot be deleted, locked or demoted. Setting another
user's password also unlocks that account, as defined by AccountsService.

Privileged writes use the system's polkit authorization agent, with the existing
terminal-agent fallback. Changing your own password runs /usr/bin/passwd with
the TUI suspended; the system prompts for passwords and enforces its own rules.
If account creation succeeds but password setup fails, the new account remains
visible for repair. Missing services produce an error, never a local UX account
fallback. Windows/macOS keep their existing application-local identity model.

The profile key linux-uid-<UID> owns appearance, dashboard, clock, and other personal
preferences. Historical UX passwords, Admin roles, and lock states cannot grant
Linux permissions. Tundra's application session ID is not a logind session ID.
No Tundra PAM policy, system account/group, system daemon, seat, VT, DRM, or input
ownership component is installed. The application does not authenticate with PAM
or have a sudo runtime dependency.

Missing XDG_RUNTIME_DIR, user D-Bus, system D-Bus, or logind does not prevent local
TUI applications from opening. Individual integrations report Unavailable when
the service or environment they require is absent.

Updates and system authorization
--------------------------------
Linux updates require a writable portable directory owned by the current user,
with tundra-installation.json and both binaries. Old system package installations
are not updated or overwritten. Tundra self-update no longer uses PackageKit,
apt, pacman or RPM. System package management is separate: the Launcher supports
APT/dpkg, DNF and pacman through the existing AutoAdmin authorization and helper.

On Arch, package lists, search, details and updates read existing pacman metadata
as the ordinary user, preserving repository priority. Repository candidate queries
also need the official /usr/bin/pacman-conf bundled with pacman; its resolved effective
configuration includes Include files. Repository previews/search/updates/install
and targeted upgrades require Search, Install and Upgrade for every configured
repository (default Usage=All works). Different roles or fully disabled repositories
report unsupported. Any sync-query stderr diagnostic rejects incomplete results,
even an exit-zero missing-database warning; local -Qi can tolerate sync warnings.
If pacman-conf is missing, sync cache is unavailable or repository configuration is
unsupported, local installed lists/details and removal remain available, along with
explicitly confirmed full-system upgrade.
Foreign installed packages can be listed and removed; no AUR build, install, update
or helper is provided.
Install, selected-package upgrade, and refresh/full-system upgrade all use
/usr/bin/pacman -Syu --needed (with -- package-name for a target). They upgrade the
whole system to avoid partial upgrades; no standalone -Sy refresh is offered.
Cached versions are previews. Stale installed/candidate identities are rejected
before task launch, and pacman confirms the refreshed final versions, dependencies
and full transaction in the AutoAdmin terminal. Removal uses only -R -- package-name,
retaining .pacsave behavior without recursive or cascade removal. Check .pacnew
and .pacsave files according to pacman output. No new password retention or root
infrastructure is added.

Settings -> Update -> Update mode is saved between runs; Release is the default.
Release checks the latest published stable GitHub release, downloads the Linux
x86_64 portable archive and verifies its SHA-256 checksum. Rust is not required.
Beta checks the latest master commit and displays its hash as the build identifier.
It downloads that exact source commit and builds locally with your existing Rust
compiler and development libraries. Neither mode installs Rust or invokes sudo.
Switching modes clears the previous check and checks the new source. Switching is
disabled during an update. A beta build can explicitly return to the stable release.

Both modes replace only Shell and CLI, preserving personal data and themes. The
helper retains the previous executables until the new Shell reports readiness.
Failed replacement, first startup failure or timeout restores the previous build.
Interrupted updates are recovered on the next launch. Downloads/builds use the
user cache. Keep assets/ and the portable marker beside the executables.

An existing system authentication agent is
used first. Only a required authorization with no available agent and a safe
foreground controlling TTY can start the pkttyagent fallback. Password input goes
directly to the system agent, never to Tundra's input events or logs. The TUI pauses
its input loop and restores canonical terminal state for authentication, then
restores raw mode, alternate screen, mouse, focus, paste, cursor, and redraw.
Power requests remain fixed logind operations using the same authorization
strategy. No privileged shell-command fallback is provided.

Diagnostics and validation
--------------------------
`tundra-cli debug doctor` reports real/effective UID/GID, NSS user, HOME, XDG paths,
user/system buses, logind, polkit, pkttyagent and portable installation status. Diagnosis only observes state and never elevates.

Run cargo fmt --check, cargo check --workspace --locked, and
cargo test --workspace --locked for development validation. Linux-specific tests
must run on Linux. A normal terminal session should report the launching user's
id -u, id -ru, id -g, HOME, and USER in Tundra Terminal. Root launch must fail before
entering the UI. Test forged USER/HOME, missing buses, first/repeated Appearance,
file permissions, and terminal restoration as well as successful paths.

Automated rollback tests use temporary portable directories. Arch package smoke
tests are explicitly read-only; fixture and query results do not validate real
package writes. Do not run destructive package transactions on a development host.
WSL tests do not substitute for host desktop-session, hardware or
physical-terminal checks.
