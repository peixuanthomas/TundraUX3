#!/usr/bin/env bash
set -euo pipefail

# Build the private GUI from a clean pinned fork plus Tundra's managed-launch
# patch.  The patch is reversed on exit, so this helper cannot leave the
# submodule dirty and packaging can continue to enforce its clean-pin rule.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_root="$repo_root/third_party/wezterm"
patch_path="$repo_root/patches/wezterm-managed-v1.patch"
output_root="${TUNDRA_WEZTERM_BUILD_DIR:-$repo_root/target/wezterm-bundled}"

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
  git -C "$source_root" apply --reverse "$patch_path" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

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
printf '%s\n' "Built patched WezTerm runtime: $output_root" >&2
printf '%s\n' "$output_root"
