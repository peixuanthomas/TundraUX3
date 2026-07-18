$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$WezTermRoot = Join-Path $RepoRoot "third_party\wezterm"
Set-Location $WezTermRoot

cargo fmt --all -- --check
cargo check -p wezterm-gui
cargo check -p wezterm-gui --features tundra-kiosk
cargo test -p config tundra_kiosk
cargo test -p wezterm-gui --features tundra-kiosk kiosk
cargo build -p wezterm-gui --features tundra-kiosk

Write-Host "Automated checks passed."
Write-Host "Run target\debug\wezterm-gui.exe start -- <program> and verify:"
Write-Host "  - borderless fullscreen covers the current display and taskbar"
Write-Host "  - a second invocation focuses the existing window"
Write-Host "  - Alt-F4, new tab/window, split, launcher and command palette are ignored"
Write-Host "  - Microsoft Pinyin IME, clipboard and terminal mouse reporting work"
Write-Host "  - exit 0 closes; non-zero exit holds a diagnostic until Enter/Escape"
