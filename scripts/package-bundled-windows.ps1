[CmdletBinding()]
param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repoRoot

function Fail([string]$Message) {
    throw "Bundled Windows packaging failed: $Message"
}

$pin = "e378176fd3aa8204ace298157599b5a3b8496ca4"
$gitlink = (git ls-tree HEAD -- third_party/wezterm | ForEach-Object { ($_ -split '\s+')[2] })
if ($gitlink -ne $pin) { Fail "gitlink is '$gitlink'; expected $pin" }
if (-not (Test-Path third_party/wezterm/.git)) { Fail "third_party/wezterm is not initialized; run git submodule update --init --recursive" }
$actual = (git -C third_party/wezterm rev-parse HEAD).Trim()
if ($actual -ne $pin) { Fail "submodule is '$actual'; expected $pin" }
git -C third_party/wezterm diff --quiet
if ($LASTEXITCODE -ne 0) { Fail "third_party/wezterm has tracked changes" }
git -C third_party/wezterm diff --cached --quiet
if ($LASTEXITCODE -ne 0) { Fail "third_party/wezterm has staged changes" }
$dirty = (git -C third_party/wezterm status --porcelain --untracked-files=normal) -join "`n"
if (-not [string]::IsNullOrWhiteSpace($dirty)) { Fail "third_party/wezterm has untracked or modified files" }

if ([string]::IsNullOrWhiteSpace($env:TUNDRA_WEZTERM_RUNTIME_DIR)) {
    Fail "TUNDRA_WEZTERM_RUNTIME_DIR must name an explicit bundled WezTerm build directory"
}
$weztermRuntime = Resolve-Path $env:TUNDRA_WEZTERM_RUNTIME_DIR -ErrorAction SilentlyContinue
if ($null -eq $weztermRuntime -or -not (Test-Path (Join-Path $weztermRuntime "wezterm-gui.exe") -PathType Leaf)) {
    Fail "explicit bundled WezTerm directory must contain wezterm-gui.exe"
}
$weztermProtocol = Join-Path $weztermRuntime "tundra-host-protocol"
if (-not (Test-Path -LiteralPath $weztermProtocol -PathType Leaf) -or
    ((Get-Content -LiteralPath $weztermProtocol -Raw) -ne "2`n")) {
    Fail "bundled WezTerm directory is missing native recovery protocol 2 capability"
}
$patchPath = Join-Path $repoRoot "patches\wezterm-managed-v1.patch"
$manifestPath = Join-Path $weztermRuntime "tundra-wezterm-manifest-v1"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Fail "bundled WezTerm directory is missing its supply-chain manifest"
}
$patchHash = (Get-FileHash -LiteralPath $patchPath -Algorithm SHA256).Hash.ToLowerInvariant()
$binaryHash = (Get-FileHash -LiteralPath (Join-Path $weztermRuntime "wezterm-gui.exe") -Algorithm SHA256).Hash.ToLowerInvariant()
$expectedManifest = "TUNDRA_WEZTERM_MANIFEST_V1`nprotocol=2`ngit_sha=$pin`npatch_sha256=$patchHash`nbinary_sha256=$binaryHash`n"
if ((Get-Content -LiteralPath $manifestPath -Raw) -cne $expectedManifest) {
    Fail "bundled WezTerm manifest does not exactly bind protocol, pin, patch and binary hash"
}

cargo build --release --locked --target $Target -p launcher -p shell -p cli
if ($LASTEXITCODE -ne 0) { Fail "Cargo build failed" }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
if ([string]::IsNullOrWhiteSpace($version)) { Fail "could not determine version" }
$releaseDir = Join-Path $repoRoot "target\$Target\release"
$out = Join-Path $repoRoot $OutputDirectory
New-Item -ItemType Directory -Force -Path $out | Out-Null
$out = (Resolve-Path $out).Path
$stage = Join-Path $out ".stage-bundled-windows"
if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
$runtime = Join-Path $stage "runtime"
New-Item -ItemType Directory -Force -Path (Join-Path $runtime "wezterm") | Out-Null

Copy-Item (Join-Path $releaseDir "tundra.exe") (Join-Path $stage "tundra.exe")
Copy-Item (Join-Path $releaseDir "tundra-shell.exe") (Join-Path $runtime "tundra-shell.exe")
Copy-Item (Join-Path $releaseDir "tundra-cli.exe") (Join-Path $runtime "tundra-cli.exe")
Copy-Item crates/ascii-assets/assets (Join-Path $runtime "assets") -Recurse
Copy-Item (Join-Path $weztermRuntime "*") (Join-Path $runtime "wezterm") -Recurse -Force
Copy-Item packaging/wezterm/tundra.lua (Join-Path $runtime "wezterm/tundra.lua") -Force
Set-Content -Path (Join-Path $runtime "launcher-protocol-version") -Value "2" -NoNewline -Encoding ascii
Copy-Item LICENSE (Join-Path $stage "LICENSE")
Copy-Item crates/weathr/LICENSE.weathr (Join-Path $stage "LICENSE.weathr")
Copy-Item third_party/wezterm/LICENSE.md (Join-Path $stage "LICENSE.wezterm")

$archive = Join-Path $out "TundraUX3-$version-experimental-windows-x64.zip"
if (Test-Path $archive) { Remove-Item -LiteralPath $archive -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $archive
$hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $(Split-Path $archive -Leaf)" | Set-Content -Path "$archive.sha256" -NoNewline -Encoding ascii
Remove-Item -LiteralPath $stage -Recurse -Force
Write-Host "Created experimental bundled archive: $archive"
