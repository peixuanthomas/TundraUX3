#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
wezterm_root="$repo_root/third_party/wezterm"

cd "$wezterm_root"
cargo fmt --all -- --check
cargo check -p wezterm-gui
cargo check -p wezterm-gui --features tundra-kiosk
cargo test -p config tundra_kiosk
cargo test -p wezterm-gui --features tundra-kiosk kiosk
cargo build -p wezterm-gui --features tundra-kiosk

printf '%s\n' \
  "Automated checks passed." \
  "Run target/debug/wezterm-gui start -- <program> for the interactive checklist:" \
  "  - borderless simple fullscreen with no tab bar or scrollbar" \
  "  - a second invocation focuses the existing window" \
  "  - Cmd-Q, Cmd-N, Cmd-T, split and close shortcuts are ignored" \
  "  - Chinese IME, clipboard and terminal mouse reporting work" \
  "  - exit 0 closes; non-zero exit holds a diagnostic until Enter/Escape"
