#!/usr/bin/env bash
set -euo pipefail

# Build the private GUI from a clean pinned fork plus Tundra's managed-launch
# patch.  The patch is reversed on exit, so this helper cannot leave the
# submodule dirty and packaging can continue to enforce its clean-pin rule.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_root="$repo_root/third_party/wezterm"
patch_path="$repo_root/patches/wezterm-managed-v1.patch"
output_root="${TUNDRA_WEZTERM_BUILD_DIR:-$repo_root/target/wezterm-bundled}"
readonly host_protocol="2"
readonly wezterm_commit="e378176fd3aa8204ace298157599b5a3b8496ca4"
readonly manifest_name="tundra-wezterm-manifest-v1"

sha256_file() {
  case "$(uname -s)" in
    Darwin) shasum -a 256 "$1" | awk '{print $1}' ;;
    *) sha256sum "$1" | awk '{print $1}' ;;
  esac
}

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    # WezTerm's pinned Windows build requires Strawberry Perl for OpenSSL and
    # the MSVC Rust host. Git Bash otherwise prefers its own incompatible Perl.
    strawberry_perl="/c/Strawberry/perl/bin"
    [[ -x "$strawberry_perl/perl.exe" ]] || {
      echo "Strawberry Perl is required at C:\\Strawberry\\perl\\bin" >&2
      exit 1
    }
    export PATH="$strawberry_perl:$PATH"
    rust_host="$(rustc -vV | sed -n 's/^host: //p')"
    [[ "$rust_host" == *-pc-windows-msvc ]] || {
      echo "Bundled WezTerm requires an MSVC Rust host on Windows; found $rust_host" >&2
      exit 1
    }
    ;;
esac

[[ -f "$patch_path" ]] || {
  echo "Managed WezTerm patch is missing: $patch_path" >&2
  exit 1
}
bash "$repo_root/scripts/verify-wezterm-submodule.sh"
git -C "$source_root" apply --check "$patch_path" || {
  echo "Managed WezTerm patch does not apply to the pinned fork" >&2
  exit 1
}

cleanup() {
  local status=$?
  trap - EXIT
  if ! git -C "$source_root" apply --reverse "$patch_path" >/dev/null 2>&1; then
    echo "Failed to restore the pinned WezTerm worktree after the managed build" >&2
    status=1
  elif [[ -n "$(git -C "$source_root" status --porcelain --untracked-files=all)" ]]; then
    echo "Managed WezTerm build left the pinned worktree dirty" >&2
    status=1
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

git -C "$source_root" apply "$patch_path"
target_dir="${CARGO_TARGET_DIR:-$source_root/target}"
(
  cd "$source_root"
  cargo build --locked --release -p wezterm-gui --features tundra-kiosk
)

binary_name="wezterm-gui"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) binary_name="wezterm-gui.exe" ;;
esac
binary_path="$target_dir/release/$binary_name"
[[ -f "$binary_path" ]] || {
  echo "Patched WezTerm build did not produce $binary_path" >&2
  exit 1
}
mkdir -p "$output_root"
cp "$binary_path" "$output_root/$binary_name"
# Capability marker consumed by every assembler and by launcher preflight.
# A random or stale wezterm-gui binary must never be labeled protocol 2 merely
# because it exists at the expected path.
printf '%s\n' "$host_protocol" > "$output_root/tundra-host-protocol"
# Keep the packaging input deliberately smaller than Cargo's release directory,
# while retaining any sibling runtime libraries emitted by the pinned build.
shopt -s nullglob
for library in \
  "$target_dir/release/"*.dll \
  "$target_dir/release/"*.so \
  "$target_dir/release/"*.so.* \
  "$target_dir/release/"*.dylib
do
  cp "$library" "$output_root/"
done
shopt -u nullglob

# The manifest binds the private artifact to the exact clean source commit,
# managed patch and binary that produced it.  Assemblers and the launcher use
# this rather than trusting a capability marker alone.
patch_sha256="$(sha256_file "$patch_path")"
binary_sha256="$(sha256_file "$output_root/$binary_name")"
cat > "$output_root/$manifest_name" <<EOF
TUNDRA_WEZTERM_MANIFEST_V1
protocol=$host_protocol
git_sha=$wezterm_commit
patch_sha256=$patch_sha256
binary_sha256=$binary_sha256
EOF
printf '%s\n' "Built patched WezTerm runtime: $output_root" >&2
printf '%s\n' "$output_root"
