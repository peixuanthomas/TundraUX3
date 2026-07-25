TundraUX3 Linux x86_64 runtime requirements
============================================

Supported desktop sessions are regular systemd/Freedesktop sessions on GNOME or
KDE, under Wayland or X11.  Run the two binaries from a real terminal:

  ./tundra-shell
  ./tundra-cli doctor

The portable archive keeps `assets` next to the binaries.  Do not move the
binaries without moving that directory too.  It includes the root MIT license
and the Weathr component license.  The Debian package installs assets under
/usr/share/tundraux3/assets automatically.

Required: xdg-open (xdg-utils) and gio (libglib2.0-bin).  Recommended for full
desktop integration: a session D-Bus bus, xdg-desktop-portal, polkit, and
XWayland when the Wayland compositor does not expose a data-control clipboard.

Use `tundra-cli doctor` after installation.  It reports missing optional desktop
services and gives the relevant package/service hint; a missing desktop helper
degrades only the affected integration, never stored data.
