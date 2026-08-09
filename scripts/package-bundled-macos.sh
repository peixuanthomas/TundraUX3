#!/usr/bin/env bash
set -euo pipefail

# Assemble an unsigned experimental .app.  Signing and notarization are release
# responsibilities; this script deliberately does not substitute a host or
# PATH WezTerm when the private artifact is absent.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
[[ "$(uname -s)" == "Darwin" ]] || { echo "Bundled macOS packaging requires macOS" >&2; exit 1; }

bash "$repo_root/scripts/verify-wezterm-submodule.sh"

wezterm_runtime="${TUNDRA_WEZTERM_RUNTIME_DIR:-}"
[[ -n "$wezterm_runtime" && -d "$wezterm_runtime" ]] || {
  echo "TUNDRA_WEZTERM_RUNTIME_DIR must name an explicit bundled WezTerm build directory" >&2
  exit 1
}
wezterm_runtime="$(cd "$wezterm_runtime" && pwd -P)"
[[ -x "$wezterm_runtime/wezterm-gui" ]] || {
  echo "Bundled WezTerm binary missing or not executable: $wezterm_runtime/wezterm-gui" >&2
  exit 1
}

target="${TUNDRA_MACOS_TARGET:-}"
target_args=()
release_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
if [[ -n "$target" ]]; then
  target_args=(--target "$target")
  release_dir="$release_dir/$target"
fi
release_dir="$release_dir/release"
version="${TUNDRAUX3_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
[[ -n "$version" ]] || { echo "Could not determine TundraUX3 version" >&2; exit 1; }
out_dir="${TUNDRAUX3_DIST_DIR:-$repo_root/dist}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
[[ "$out_dir" != "/" && "$out_dir" != "$repo_root" ]] || {
  echo "Refusing unsafe distribution directory: $out_dir" >&2
  exit 1
}

architecture="${target:-$(uname -m)}"
archive_name="TundraUX3-${version}-experimental-macos-${architecture}"
stage_root="$out_dir/.stage-bundled-macos"
app_root="$stage_root/TundraUX3.app"
runtime_root="$app_root/Contents/Resources/runtime"
rm -rf "$stage_root"
mkdir -p "$app_root/Contents/MacOS" "$runtime_root/wezterm"

cargo build --release --locked "${target_args[@]}" -p launcher -p shell -p cli -p recovery

install -m 755 "$release_dir/tundra" "$app_root/Contents/MacOS/tundra"
install -m 755 "$release_dir/tundra-shell" "$runtime_root/tundra-shell"
install -m 755 "$release_dir/tundra-cli" "$runtime_root/tundra-cli"
install -m 755 "$release_dir/tundra-recovery" "$runtime_root/tundra-recovery"
cp -a crates/ascii-assets/assets "$runtime_root/assets"
cp -a "$wezterm_runtime/." "$runtime_root/wezterm/"
install -m 644 packaging/wezterm/tundra.lua "$runtime_root/wezterm/tundra.lua"
printf '1\n' > "$runtime_root/launcher-protocol-version"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app_root/Contents/Info.plist"
install -m 644 LICENSE "$app_root/Contents/Resources/LICENSE"
install -m 644 crates/weathr/LICENSE.weathr "$app_root/Contents/Resources/LICENSE.weathr"
install -m 644 third_party/wezterm/LICENSE.md "$app_root/Contents/Resources/LICENSE.wezterm"

ditto -c -k --sequesterRsrc --keepParent "$app_root" "$out_dir/$archive_name.zip"
shasum -a 256 "$out_dir/$archive_name.zip" > "$out_dir/$archive_name.sha256"
rm -rf "$stage_root"
echo "Created unsigned experimental app bundle: $out_dir/$archive_name.zip"
