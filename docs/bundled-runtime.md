# Experimental bundled runtime

The experimental distribution has one visible entry point: `tundra` on
Windows/Linux and `TundraUX3.app` on macOS. It is a launcher, not the Shell;
double-clicking it resolves only paths within its installation, starts the
pinned Tundra WezTerm fork with `start -- <absolute private child>` and passes
it the private `tundra-shell` program.
Neither a system terminal nor `PATH`, `~/.wezterm.lua` or a user-installed
WezTerm participates in startup.

The outer launcher owns process-level recovery. After the automatic retry
budget is exhausted it starts the private `tundra-recovery` child inside a
fresh bundled WezTerm, passes only a versioned, sanitized
`RecoveryHandoffV1`, and accepts an incident-bound atomic restart credential
written after Enter. The recovery program renders the high-error-correction QR
as an integer-scaled PNG through WezTerm's image protocol; it never builds the
QR from terminal glyphs. Closing that window without the credential is treated
as another recovery failure.

This is the currently testable experimental implementation, but it still uses
a private PTY child for the recovery program. The planned native WezTerm
no-PTY recovery renderer cannot be built or validated from this checkout until
the pinned submodule is available, and is therefore not represented as a
completed release capability.

## Layout contract

Portable Linux and Windows archives contain:

```text
tundra[.exe]
runtime/
  tundra-shell[.exe]
  tundra-cli[.exe]
  tundra-recovery[.exe]
  assets/
  launcher-protocol-version
  wezterm/
    wezterm-gui[.exe]
    tundra.lua
    ...required WezTerm libraries/resources...
```

The Debian package exposes only `/usr/bin/tundra`; its private tree is
`/usr/lib/tundra/runtime`. The macOS app puts the launcher at
`TundraUX3.app/Contents/MacOS/tundra` and the same private tree at
`Contents/Resources/runtime`.

`launcher-protocol-version` is currently `1`. The launcher preflights every
listed component and refuses to start if the protocol or any private component
is missing. This is intentional: there is no fallback to a host terminal.

## Building an experimental artifact

Initialize the pinned fork first, then build its kiosk-enabled GUI through the
helper. It checks that the root `patches/wezterm-managed-v1.patch` applies to
the clean pin, applies it only for the build, and reverses it before returning.
The assembler accepts a GUI only through the explicit
`TUNDRA_WEZTERM_RUNTIME_DIR` variable. It must name a directory containing the
fork-built `wezterm-gui` binary (and any platform libraries/resources needed by
that binary); it is copied verbatim into the private runtime.

```console
git submodule update --init --recursive
bash scripts/build-bundled-wezterm.sh
TUNDRA_WEZTERM_RUNTIME_DIR="$PWD/target/wezterm-bundled" \
  bash scripts/package-bundled-linux.sh
```

On Windows run `scripts/package-bundled-windows.ps1` after setting the same
environment variable to the directory containing `wezterm-gui.exe`. On macOS
run `scripts/package-bundled-macos.sh`; set `TUNDRA_MACOS_TARGET` when building
for a specific architecture. The macOS result is intentionally unsigned and
not notarized. Release signing, notarization, IME/multi-display testing and
shutdown/logout testing remain required before promoting these artifacts.

All three assemblers validate the pinned submodule commit
`e378176fd3aa8204ace298157599b5a3b8496ca4`, reject a dirty or uninitialized
submodule, and fail if the explicitly supplied binary is absent. They never
download WezTerm and never search `PATH`.
