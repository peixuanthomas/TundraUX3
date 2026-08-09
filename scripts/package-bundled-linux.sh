#!/usr/bin/env bash
set -euo pipefail

# Assemble the experimental private-runtime distribution.  This script never
# discovers wezterm-gui from PATH: callers must explicitly provide a directory
# produced by the pinned Tundra WezTerm fork.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "Bundled Linux packaging supports only an x86_64 Linux build host" >&2
  exit 1
fi

build_deb=true
case "${1:-}" in
  "") ;;
  --tar-only) build_deb=false ;;
  *) echo "Usage: $0 [--tar-only]" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "Usage: $0 [--tar-only]" >&2; exit 2; }

bash "$repo_root/scripts/verify-wezterm-submodule.sh"

wezterm_runtime="${TUNDRA_WEZTERM_RUNTIME_DIR:-}"
[[ -n "$wezterm_runtime" ]] || {
  echo "TUNDRA_WEZTERM_RUNTIME_DIR must name an explicit bundled WezTerm build directory" >&2
  exit 1
}
[[ -d "$wezterm_runtime" ]] || {
  echo "Explicit bundled WezTerm directory does not exist: $wezterm_runtime" >&2
  exit 1
}
wezterm_runtime="$(cd "$wezterm_runtime" && pwd -P)"
[[ -x "$wezterm_runtime/wezterm-gui" ]] || {
  echo "Bundled WezTerm binary missing or not executable: $wezterm_runtime/wezterm-gui" >&2
  exit 1
}

version="${TUNDRAUX3_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
[[ -n "$version" ]] || { echo "Could not determine TundraUX3 version" >&2; exit 1; }
out_dir="${TUNDRAUX3_DIST_DIR:-$repo_root/dist}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
[[ "$out_dir" != "/" && "$out_dir" != "$repo_root" ]] || {
  echo "Refusing unsafe distribution directory: $out_dir" >&2
  exit 1
}

release_dir="${CARGO_TARGET_DIR:-$repo_root/target}/release"
package_name="tundraux3-bundled_${version}_amd64"
portable_name="tundraux3-bundled-${version}-linux-x86_64"
stage_root="$out_dir/.stage-bundled"

rm -rf "$stage_root"
mkdir -p "$stage_root/$portable_name/runtime/wezterm"

cargo build --release --locked -p launcher -p shell -p cli -p recovery

portable_root="$stage_root/$portable_name"
runtime_root="$portable_root/runtime"
install -Dm755 "$release_dir/tundra" "$portable_root/tundra"
install -Dm755 "$release_dir/tundra-shell" "$runtime_root/tundra-shell"
install -Dm755 "$release_dir/tundra-cli" "$runtime_root/tundra-cli"
install -Dm755 "$release_dir/tundra-recovery" "$runtime_root/tundra-recovery"
cp -a crates/ascii-assets/assets "$runtime_root/assets"
cp -a "$wezterm_runtime/." "$runtime_root/wezterm/"
install -Dm644 packaging/wezterm/tundra.lua "$runtime_root/wezterm/tundra.lua"
printf '1\n' > "$runtime_root/launcher-protocol-version"
install -Dm644 LICENSE "$portable_root/LICENSE"
install -Dm644 crates/weathr/LICENSE.weathr "$portable_root/LICENSE.weathr"
install -Dm644 third_party/wezterm/LICENSE.md "$portable_root/LICENSE.wezterm"
install -Dm644 packaging/linux/README-BUNDLED-LINUX.txt "$portable_root/README-LINUX.txt"

tar -C "$stage_root" -czf "$out_dir/$portable_name.tar.gz" "$portable_name"
artifacts=("$portable_name.tar.gz")

if [[ "$build_deb" == true ]]; then
  deb_root="$stage_root/deb"
  deb_runtime="$deb_root/usr/lib/tundra/runtime"
  install -Dm755 "$release_dir/tundra" "$deb_root/usr/bin/tundra"
  install -Dm755 "$release_dir/tundra-shell" "$deb_runtime/tundra-shell"
  install -Dm755 "$release_dir/tundra-cli" "$deb_runtime/tundra-cli"
  install -Dm755 "$release_dir/tundra-recovery" "$deb_runtime/tundra-recovery"
  cp -a crates/ascii-assets/assets "$deb_runtime/assets"
  mkdir -p "$deb_runtime/wezterm"
  cp -a "$wezterm_runtime/." "$deb_runtime/wezterm/"
  install -Dm644 packaging/wezterm/tundra.lua "$deb_runtime/wezterm/tundra.lua"
  printf '1\n' > "$deb_runtime/launcher-protocol-version"
  install -Dm644 packaging/debian/tundraux3-bundled.desktop "$deb_root/usr/share/applications/tundraux3-bundled-experimental.desktop"
  install -Dm644 LICENSE "$deb_root/usr/share/doc/tundraux3-bundled-experimental/copyright"
  install -Dm644 crates/weathr/LICENSE.weathr "$deb_root/usr/share/doc/tundraux3-bundled-experimental/LICENSE.weathr"
  install -Dm644 third_party/wezterm/LICENSE.md "$deb_root/usr/share/doc/tundraux3-bundled-experimental/LICENSE.wezterm"
  install -Dm644 packaging/linux/README-BUNDLED-LINUX.txt "$deb_root/usr/share/doc/tundraux3-bundled-experimental/README-LINUX.txt"
  install -d "$deb_root/DEBIAN"
  sed "s/@VERSION@/$version/g" packaging/debian/control.bundled > "$deb_root/DEBIAN/control"
  dpkg-deb --build --root-owner-group "$deb_root" "$out_dir/$package_name.deb"
  artifacts+=("$package_name.deb")
fi

(
  cd "$out_dir"
  sha256sum "${artifacts[@]}" > "${portable_name}.SHA256SUMS"
)
rm -rf "$stage_root"
echo "Created experimental bundled artifacts in $out_dir: ${artifacts[*]}"
