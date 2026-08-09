#!/usr/bin/env bash
set -euo pipefail

# The bundled terminal is a product dependency, not an optional developer
# convenience.  Keep this check deliberately independent of PATH and of the
# caller's current directory so every assembler and CI job enforces the same
# fork revision.
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly tundra_wezterm_commit="e378176fd3aa8204ace298157599b5a3b8496ca4"
readonly submodule_path="$repo_root/third_party/wezterm"

fail() {
  echo "bundled WezTerm verification failed: $*" >&2
  exit 1
}

git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null 2>&1 || \
  fail "repository metadata is unavailable"

tree_commit="$(git -C "$repo_root" ls-tree HEAD -- third_party/wezterm | awk '{print $3}')"
[[ "$tree_commit" == "$tundra_wezterm_commit" ]] || \
  fail "gitlink is ${tree_commit:-missing}; expected $tundra_wezterm_commit"

[[ -d "$submodule_path" ]] || \
  fail "third_party/wezterm is not initialized; run git submodule update --init --recursive"
[[ -e "$submodule_path/.git" ]] || \
  fail "third_party/wezterm is not initialized; run git submodule update --init --recursive"

submodule_top="$(git -C "$submodule_path" rev-parse --show-toplevel 2>/dev/null || true)"
[[ "${submodule_top//\\//}" == "${submodule_path//\\//}" ]] || \
  fail "third_party/wezterm does not contain its own Git worktree"

actual_commit="$(git -C "$submodule_path" rev-parse HEAD 2>/dev/null || true)"
[[ "$actual_commit" == "$tundra_wezterm_commit" ]] || \
  fail "working tree is ${actual_commit:-uninitialized}; expected $tundra_wezterm_commit"

git -C "$submodule_path" diff --quiet || fail "third_party/wezterm has tracked changes"
git -C "$submodule_path" diff --cached --quiet || fail "third_party/wezterm has staged changes"
[[ -z "$(git -C "$submodule_path" status --porcelain --untracked-files=normal)" ]] || \
  fail "third_party/wezterm has untracked or modified files"

status="$(git -C "$repo_root" submodule status --cached -- third_party/wezterm)"
[[ "$status" == " $tundra_wezterm_commit "* ]] || \
  fail "submodule index is not pinned to $tundra_wezterm_commit"

echo "Verified bundled WezTerm submodule $tundra_wezterm_commit"
