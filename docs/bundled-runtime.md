# Experimental bundled runtime

The experimental distribution has one visible entry point: `tundra` on
Windows/Linux and `TundraUX3.app` on macOS. It is a launcher, not the Shell;
double-clicking it resolves only paths within its installation, starts the
pinned Tundra WezTerm fork with `start -- <absolute private child>` and passes
it the private `tundra-shell` program.
Neither a system terminal nor `PATH`, `~/.wezterm.lua` or a user-installed
WezTerm participates in startup.

The outer launcher owns process-level recovery. After the automatic retry
budget is exhausted it starts a fresh bundled WezTerm with its private
`tundra-recovery` command, passes only a versioned, sanitized
`RecoveryHandoffV1`, and accepts a one-time incident-bound restart credential
only after native recovery observes Enter. This command owns the kernel-panic
scene and QR pixel rendering itself: it does not allocate a terminal PTY and
does not spawn a `tundra-recovery` helper binary. Closing the window without a
matching credential is treated as another recovery failure.

## Layout contract

Portable Linux and Windows archives contain:

```text
tundra[.exe]
runtime/
  tundra-shell[.exe]
  tundra-cli[.exe]
  assets/
  launcher-protocol-version
  wezterm/
    wezterm-gui[.exe]
    tundra-host-protocol
    tundra-wezterm-manifest-v1
    tundra.lua
    ...required WezTerm libraries/resources...
```

The Debian package exposes only `/usr/bin/tundra`; its private tree is
`/usr/lib/tundra/runtime`. The macOS app puts the launcher at
`TundraUX3.app/Contents/MacOS/tundra` and the same private tree at
`Contents/Resources/runtime`.

`launcher-protocol-version` and the build-emitted
`wezterm/tundra-host-protocol` capability marker are both currently `2`. The
build also writes an exact five-line `tundra-wezterm-manifest-v1` record:

```text
TUNDRA_WEZTERM_MANIFEST_V1
protocol=2
git_sha=e378176fd3aa8204ace298157599b5a3b8496ca4
patch_sha256=<current root patch SHA-256>
binary_sha256=<wezterm-gui SHA-256>
```

The manifest is copied verbatim inside the private runtime. Before packaging,
every platform verifies the clean gitlink/pin, protocol marker, exact manifest,
current managed-patch hash and GUI binary hash. The launcher embeds that same
managed-patch hash at build time. At startup it rejects missing or malformed
manifests, any protocol, pin or patch mismatch, and any binary whose SHA-256
differs from the manifest. Assemblers also reject a GUI directory
without the marker, so an ordinary WezTerm binary cannot be mislabeled as the
native recovery host. This is intentional: there is no fallback to a host
terminal.

## Building an experimental artifact

Initialize the pinned fork first, then build its kiosk-enabled GUI through the
helper. It checks that the root `patches/wezterm-managed-v1.patch` applies to
the clean pin, applies it only for the build, and reverses it before returning.
The assembler accepts a GUI only through the explicit
`TUNDRA_WEZTERM_RUNTIME_DIR` variable. It must name a directory containing the
fork-built `wezterm-gui` binary (and any platform libraries/resources needed by
that binary) together with its `tundra-host-protocol` marker and
`tundra-wezterm-manifest-v1`; it is copied verbatim into the private runtime.

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
submodule, and fail if the marker, manifest, current patch hash or explicitly
supplied binary hash is missing or mismatched. They never download WezTerm and
never search `PATH`.
