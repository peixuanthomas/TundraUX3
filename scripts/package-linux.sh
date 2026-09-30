#!/usr/bin/env bash
set -euo pipefail

# Produce only the portable x86_64 tarball from a locked release build.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "Linux packaging currently supports only an x86_64 Linux build host" >&2
  exit 1
fi

if [[ $# -gt 1 || ( $# -eq 1 && "$1" != "--tar-only" ) ]]; then
  echo "Usage: $0 [--tar-only]" >&2
  exit 2
fi

version="${TUNDRAUX3_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Expected a numeric major.minor.patch package version: $version" >&2
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
portable_name="tundraux3-${version}-linux-x86_64"
stage_root="$out_dir/.stage"

rm -rf "$stage_root"
mkdir -p "$stage_root/$portable_name"

cargo build --release --locked -p shell -p cli

install -Dm755 "$release_dir/tundra-shell" "$stage_root/$portable_name/tundra-shell"
install -Dm755 "$release_dir/tundra-cli" "$stage_root/$portable_name/tundra-cli"
cp -a crates/ascii-assets/assets "$stage_root/$portable_name/assets"
for locale in en-US zh-CN; do
  test -s "$stage_root/$portable_name/assets/locales/$locale/manifest.toml"
done
install -Dm644 LICENSE "$stage_root/$portable_name/LICENSE"
install -Dm644 crates/weathr/LICENSE.weathr "$stage_root/$portable_name/LICENSE.weathr"
install -Dm644 docs/packaging/linux/README-LINUX.txt "$stage_root/$portable_name/README-LINUX.txt"
# The updater requires this marker beside both portable binaries.
install -Dm644 packaging/linux/tundra-installation.json "$stage_root/$portable_name/tundra-installation.json"

tar -C "$stage_root" -czf "$out_dir/$portable_name.tar.gz" "$portable_name"

artifacts=("$portable_name.tar.gz")
(
  cd "$out_dir"
  sha256sum "${artifacts[@]}" > SHA256SUMS
)
rm -rf "$stage_root"
echo "Created ${artifacts[*]} in $out_dir"
