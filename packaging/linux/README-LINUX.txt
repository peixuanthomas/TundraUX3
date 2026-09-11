TundraUX3 Linux x86_64 runtime requirements
============================================

Supported desktop sessions are regular systemd/Freedesktop sessions on GNOME or
KDE, under Wayland or X11.  Run the two binaries from a real terminal:

  ./tundra-shell
  ./tundra-cli debug doctor

The portable archive keeps `assets` next to the binaries.  Do not move the
binaries without moving that directory too.  It includes the root MIT license
and the Weathr component license.  The Debian and RPM packages install assets under
/usr/share/tundraux3/assets automatically.

Required: xdg-open (xdg-utils) and gio (libglib2.0-bin on Debian/Ubuntu,
glib2 on Fedora).  Recommended for full
desktop integration: a session D-Bus bus, xdg-desktop-portal, polkit, and
XWayland when the Wayland compositor does not expose a data-control clipboard.

Use `tundra-cli debug doctor` after installation.  It reports missing optional desktop
services and gives the relevant package/service hint; a missing desktop helper
degrades only the affected integration, never stored data.

Package installation
--------------------
Debian/Ubuntu: sudo apt install ./TundraUX3-v1.3.1-linux-amd64.deb
Fedora: sudo dnf install ./TundraUX3-v1.3.1-fedora-x86_64.rpm

The release RPM is built and installation-tested on Fedora 43 x86_64. Other
Fedora derivatives must satisfy its generated library dependencies; compatibility
with RHEL/CentOS/Rocky/AlmaLinux is not assumed. Build on the target distribution
with `bash scripts/package-linux.sh --rpm` (requires rpm-build) when necessary.
The application updater replaces binaries only; package-manager version records
are updated by installing a newer package through apt/dnf.

Linux system login
------------------
The Linux shell runs as root for every authenticated user. When launched as a
regular user it re-executes through /usr/bin/sudo -H before opening storage or
the UI. A root terminal can launch it directly. The initial sudo prompt grants
process privileges; the subsequent UX login verifies the selected Linux user's
password. Cancelling/denying sudo prevents startup. UX login does not change the
process UID, groups, HOME or desktop session to the selected user.

Login accounts are enumerated with /usr/bin/getent passwd (NSS). The list includes
root and ordinary users within UID_MIN/UID_MAX from /etc/login.defs (defaults:
1000..60000), excluding accounts with non-login shells. Password locks, expiry
and access policy are evaluated by PAM at login; the list does not inspect shadow
passwords and cannot predict whether PAM will allow a login. NSS providers that
disable account enumeration will not expose their users in this list.

The Debian package installs /etc/pam.d/tundraux3 using the distribution's
common-auth and common-account stacks. The Fedora RPM uses system-auth and
marks this configuration as noreplace so upgrades preserve local changes.
Portable installs use the existing PAM
login service if tundraux3 is absent. A missing/broken PAM service fails login;
there is no fallback to UX passwords. Password-only PAM conversations are
supported; additional secret prompts (such as MFA) fail with an explicit error.
Use passwd outside UX when PAM requires an expired password to be changed.

Required runtime packages: libpam0g/libpam-modules/libpam-runtime and libc-bin on
Debian/Ubuntu; pam and glibc on Fedora/Arch. sudo is needed for non-root launches.
No PAM development headers are needed to build. The portable binary requires
libpam.so.0 from the host distribution.

UX skips local-account setup and never authenticates Linux users with saved UX
passwords. Old UX records are retained, but are absent from the Linux login list.
Only appearance, dashboard and login timestamps are attached to linux-uid-<UID>
profiles; clocks also use this UID key. New profiles open the existing Appearance
setup page after PAM authentication. Home remains unavailable until preferences
and the completion marker are saved together. Interrupted or failed setup is
resumed on the next login; older saved profiles keep their existing preferences.
The initial icon mode is ASCII unless terminal image support is available.
Default-theme image support is checked again for each Linux user login. Linux names are case-sensitive. Account
creation, deletion, password and role changes belong to Linux tools; the UX user
page lists system accounts and explains that these actions are managed by Linux.
All authenticated Linux users have the same UX Admin role and root OS authority.

With sudo -H, UX storage normally lives under root's home/XDG directories, so
existing user-home UX data is not automatically migrated. Root's desktop session
may lack the calling user's clipboard, portals and D-Bus services. User selection
changes UX preferences, not the OS home directory or desktop session.

Explorer personal folders and the Command Line's initial Documents directory
follow the selected Linux account: NSS supplies its home, and that home's
.config/user-dirs.dirs supplies localized or custom folder paths. Root's HOME
and XDG environment do not override these account directories. Switching users
resolves the new account's folders without changing the process privileges.
If a setting is missing or invalid, existing standard English, Simplified Chinese
and Traditional Chinese folders in that account's home are checked before using
the English default. Explicit paths, including $HOME to disable a folder, take
precedence even if temporarily unavailable. Quoted values may include trailing
comments and a leading ${HOME}; no shell commands are evaluated.

Validation: cargo test -p identity -p shell -p ui. For real-machine acceptance,
start ./tundra-shell from a terminal, authenticate sudo, select an existing Linux
account and enter its system password. Check wrong passwords, a locked/expired
account, logout/relogin, per-user appearance and terminal `id -u` (must print 0).
Run account lock/expiry checks only with disposable test accounts.
