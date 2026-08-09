TundraUX3 experimental bundled Linux x86_64 runtime
===================================================

Launch TundraUX3 from the application menu or by clicking/double-clicking the
public `tundra` executable. It starts the private WezTerm runtime and enters
Tundra Shell automatically. Do not run tundra-shell directly and do not move
private files out of the `runtime` directory.

The portable layout is `tundra` plus `runtime/`. The experimental Debian package
installs the public launcher at `/usr/bin/tundra` and private components under
`/usr/lib/tundra/runtime`. Neither layout searches PATH for WezTerm or reads a
user `.wezterm.lua`.

Use `runtime/tundra-cli doctor` only for portable diagnostics. Required desktop
helpers are xdg-open (xdg-utils) and gio (libglib2.0-bin). A session D-Bus bus,
xdg-desktop-portal, polkit, and XWayland are recommended.

The archive includes the root MIT license, the Weathr component license, and
the bundled WezTerm license. Keep the complete runtime together when moving or
upgrading an installation.
