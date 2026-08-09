#!/usr/bin/env bash
set -euo pipefail

# Verify an already built private WezTerm runtime before copying it into a
# distributable. The manifest is intentionally small and exact: accepting
# unknown fields would turn it into an unreviewed extension point.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly host_protocol="2"
readonly wezterm_commit="e378176fd3aa8204ace298157599b5a3b8496ca4"
readonly manifest_name="tundra-wezterm-manifest-v1"

fail() {
  echo "bundled WezTerm runtime verification failed: $*" >&2
  exit 1
}

sha256_file() {
  case "$(uname -s)" in
    Darwin) shasum -a 256 "$1" | awk '{print $1}' ;;
    *) sha256sum "$1" | awk '{print $1}' ;;
  esac
}

[[ $# -eq 1 ]] || fail "usage: $0 <runtime-directory>"
runtime="$1"
[[ -d "$runtime" ]] || fail "runtime directory does not exist: $runtime"
runtime="$(cd "$runtime" && pwd -P)"

bash "$repo_root/scripts/verify-wezterm-submodule.sh"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) binary_name="wezterm-gui.exe" ;;
  *) binary_name="wezterm-gui" ;;
esac
binary="$runtime/$binary_name"
marker="$runtime/tundra-host-protocol"
manifest="$runtime/$manifest_name"

[[ -f "$binary" ]] || fail "WezTerm binary is missing: $binary"
[[ -f "$marker" ]] && [[ "$(tr -d '[:space:]' < "$marker")" == "$host_protocol" ]] || \
  fail "native recovery protocol $host_protocol marker is missing or invalid"
[[ -f "$manifest" ]] || fail "runtime manifest is missing: $manifest"

patch_sha256="$(sha256_file "$repo_root/patches/wezterm-managed-v1.patch")"
binary_sha256="$(sha256_file "$binary")"
expected_manifest="$(printf 'TUNDRA_WEZTERM_MANIFEST_V1\nprotocol=%s\ngit_sha=%s\npatch_sha256=%s\nbinary_sha256=%s\n' \
  "$host_protocol" "$wezterm_commit" "$patch_sha256" "$binary_sha256")"
cmp -s "$manifest" <(printf '%s\n' "$expected_manifest") || \
  fail "manifest does not exactly bind protocol, pin, patch and binary hash"

echo "Verified bundled WezTerm runtime $runtime"
