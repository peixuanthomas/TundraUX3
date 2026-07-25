#!/usr/bin/env bash
set -euo pipefail

# Produce the portable x86_64 tarball and Debian package from a locked release
# build.  Fedora users consume the tarball; Debian/Ubuntu users consume the deb.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "Linux packaging currently supports only an x86_64 Linux build host" >&2
  exit 1
fi

build_deb=true
case "${1:-}" in
  "")
    ;;
  --tar-only)
    build_deb=false
    ;;
  *)
    echo "Usage: $0 [--tar-only]" >&2
    exit 2
    ;;
esac
if [[ $# -gt 1 ]]; then
  echo "Usage: $0 [--tar-only]" >&2
  exit 2
fi

version="${TUNDRAUX3_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
if [[ -z "$version" ]]; then
  echo "Could not determine TundraUX3 version" >&2
  exit 1
fi

out_dir="${TUNDRAUX3_DIST_DIR:-$repo_root/dist}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
if [[ "$out_dir" == "/" || "$out_dir" == "$repo_root" ]]; then
  echo "Refusing unsafe distribution directory: $out_dir" >&2
  exit 1
fi
release_dir="${CARGO_TARGET_DIR:-$repo_root/target}/release"
package_name="tundraux3_${version}_amd64"
portable_name="tundraux3-${version}-linux-x86_64"
stage_root="$out_dir/.stage"

rm -rf "$stage_root"
mkdir -p "$stage_root/$portable_name"

cargo build --release --locked -p shell -p cli

install -Dm755 "$release_dir/tundra-shell" "$stage_root/$portable_name/tundra-shell"
install -Dm755 "$release_dir/tundra-cli" "$stage_root/$portable_name/tundra-cli"
cp -a crates/ascii-assets/assets "$stage_root/$portable_name/assets"
install -Dm644 LICENSE "$stage_root/$portable_name/LICENSE"
install -Dm644 crates/weathr/LICENSE.weathr "$stage_root/$portable_name/LICENSE.weathr"
install -Dm644 packaging/linux/README-LINUX.txt "$stage_root/$portable_name/README-LINUX.txt"

tar -C "$stage_root" -czf "$out_dir/$portable_name.tar.gz" "$portable_name"

artifacts=("$portable_name.tar.gz")
if [[ "$build_deb" == true ]]; then
  deb_root="$stage_root/deb"
  install -Dm755 "$release_dir/tundra-shell" "$deb_root/usr/bin/tundra-shell"
  install -Dm755 "$release_dir/tundra-cli" "$deb_root/usr/bin/tundra-cli"
  install -d "$deb_root/usr/share/tundraux3"
  cp -a crates/ascii-assets/assets "$deb_root/usr/share/tundraux3/assets"
  install -Dm644 packaging/debian/tundraux3.desktop "$deb_root/usr/share/applications/tundraux3.desktop"
  install -Dm644 LICENSE "$deb_root/usr/share/doc/tundraux3/copyright"
  install -Dm644 crates/weathr/LICENSE.weathr "$deb_root/usr/share/doc/tundraux3/LICENSE.weathr"
  install -Dm644 packaging/linux/README-LINUX.txt "$deb_root/usr/share/doc/tundraux3/README-LINUX.txt"

  install -d "$deb_root/DEBIAN"
  sed "s/@VERSION@/$version/g" packaging/debian/control > "$deb_root/DEBIAN/control"
  dpkg-deb --build --root-owner-group "$deb_root" "$out_dir/$package_name.deb"
  artifacts+=("$package_name.deb")
fi

(
  cd "$out_dir"
  sha256sum "${artifacts[@]}" > SHA256SUMS
)
rm -rf "$stage_root"
echo "Created ${artifacts[*]} in $out_dir"
