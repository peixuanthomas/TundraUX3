#!/usr/bin/env bash
set -euo pipefail

# Build ordinary-user portable UX and optional distro installation from the same binaries.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo 'Linux packaging supports an x86_64 Linux build host only' >&2
  exit 1
fi
flavor=deb
case "${1:-}" in
  '') ;;
  --rpm) flavor=rpm ;;
  --tar-only) flavor=portable ;;
  *) echo "Usage: $0 [--tar-only|--rpm]" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "Usage: $0 [--tar-only|--rpm]" >&2; exit 2; }
version="${TUNDRAUX3_VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || { echo "Invalid version: $version" >&2; exit 1; }
if [[ "$flavor" == deb ]]; then command -v dpkg-deb >/dev/null; fi
if [[ "$flavor" == rpm ]]; then command -v rpmbuild >/dev/null; fi
out_dir="${TUNDRAUX3_DIST_DIR:-$repo_root/dist}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd -P)"
[[ "$out_dir" != / && "$out_dir" != "$repo_root" ]] || { echo 'Unsafe output directory' >&2; exit 1; }
stage_root="$(mktemp -d "$out_dir/.stage.XXXXXXXX")"
trap 'rm -rf -- "$stage_root"' EXIT
release_dir="${TUNDRAUX3_PREBUILT_BIN_DIR:-${CARGO_TARGET_DIR:-$repo_root/target}/release}"
if [[ -z "${TUNDRAUX3_PREBUILT_BIN_DIR:-}" ]]; then
  if [[ "$flavor" == portable ]]; then
    cargo build --release --locked -p shell -p cli
  else
    cargo build --release --locked -p shell -p cli -p tundra-sessiond -p tundra-greeter -p tundra-privileged -p system-maintenance
  fi
fi
portable_name="tundraux3-${version}-linux-x86_64"
portable="$stage_root/$portable_name"
mkdir -p "$portable"
install -m755 "$release_dir/tundra-shell" "$portable/tundra-shell"
install -m755 "$release_dir/tundra-cli" "$portable/tundra-cli"
cp -a crates/ascii-assets/assets "$portable/assets"
for locale in en-US zh-CN; do test -s "$portable/assets/locales/$locale/manifest.toml"; done
install -m644 LICENSE "$portable/LICENSE"
install -m644 crates/weathr/LICENSE.weathr "$portable/LICENSE.weathr"
install -m644 packaging/linux/README-LINUX.txt "$portable/README-LINUX.txt"

if [[ "$flavor" != portable ]]; then
  # Trust roots are obtained during the trusted build, never during package install
  # and never from a user-supplied update envelope. Offline builders may supply a
  # previously reviewed root file explicitly.
  roots="${TUNDRAUX3_TRUSTED_ROOT:-$stage_root/update-trusted-root.jsonl}"
  if [[ -z "${TUNDRAUX3_TRUSTED_ROOT:-}" ]]; then
    command -v gh >/dev/null
    gh attestation trusted-root > "$roots"
  fi
  test -s "$roots"
  python3 scripts/stage-linux-system.py --root "$portable/system-root" \
    --binaries "$release_dir" --version "$version" --source-sha "$(git rev-parse HEAD)" \
    --trusted-root "$roots" --flavor "$flavor"
fi

tar -C "$stage_root" -czf "$out_dir/$portable_name.tar.gz" "$portable_name"
artifacts=("$portable_name.tar.gz")
if [[ "$flavor" == deb ]]; then
  deb_root="$stage_root/deb"
  mkdir -p "$deb_root"
  cp -a "$portable/system-root/." "$deb_root/"
  install -d "$deb_root/DEBIAN"
  sed "s/@VERSION@/$version/g" packaging/debian/control > "$deb_root/DEBIAN/control"
  install -m755 packaging/debian/postinst "$deb_root/DEBIAN/postinst"
  install -m644 packaging/debian/conffiles "$deb_root/DEBIAN/conffiles"
  name="tundraux3_${version}_amd64.deb"
  dpkg-deb --build --root-owner-group "$deb_root" "$out_dir/$name"
  artifacts+=("$name")
elif [[ "$flavor" == rpm ]]; then
  rpm_root="$stage_root/rpmbuild"
  mkdir -p "$rpm_root"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
  cp "$out_dir/$portable_name.tar.gz" "$rpm_root/SOURCES/"
  rpmbuild -bb --define "_topdir $rpm_root" --define "tundra_version $version" packaging/rpm/tundraux3.spec
  name="tundraux3-${version}-1.x86_64.rpm"
  cp "$rpm_root/RPMS/x86_64/$name" "$out_dir/$name"
  artifacts+=("$name")
fi
(cd "$out_dir" && sha256sum "${artifacts[@]}" > SHA256SUMS)
echo "Created ${artifacts[*]} in $out_dir"
