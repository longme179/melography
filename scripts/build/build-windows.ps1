# MELOGRAPHY — Windows build script (issue #25 session 10; docs/BUILDING.md).
# PowerShell 5.1+ (the Windows default). From the repo root:
#
#   powershell -ExecutionPolicy Bypass -File scripts\build\build-windows.ps1
#
# Options:
#   -Bundles "nsis,msi"   installer targets (default nsis,msi)
#   -BinOnly              executable only (no installer)
#   -Core                 also run the workspace test gate first
#   -NoInstall            skip nothing to install on Windows (kept for symmetry
#                         with build-linux.sh; accepted and ignored)
#
# Prerequisites the script CANNOT install itself (see docs/BUILDING.md):
#   * Visual Studio 2022 Build Tools — "Desktop development with C++"
#     (MSVC v143 + Windows 10/11 SDK); the MSVC linker is required.
#   * Rust via rustup (host x86_64-pc-windows-msvc; rust-toolchain.toml pins
#     the compiler version automatically).
#   * Node.js >= 18 + npm.
#   * WebView2 Runtime to RUN the app (preinstalled on Win 11 / current Win 10).
#
# The output is an UNSIGNED local build — Authenticode signing uses an
# owner-provisioned certificate in the M5 release workflow
# (docs/RELEASING.md §3); this script never signs or publishes anything.

[CmdletBinding()]
param(
  [string]$Bundles = "nsis,msi",
  [switch]$BinOnly,
  [switch]$Core,
  [switch]$NoInstall
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$App = Join-Path $Repo "apps\melography-app"

function Step($msg) { Write-Host "`n== $msg" -ForegroundColor Cyan }
function Need($cmd, $hint) {
  if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
    Write-Error "MISSING: $cmd — $hint"; exit 1
  }
}

# ── 1. Toolchain checks ─────────────────────────────────────────────────────
Step "Toolchain"
Need cargo  "install Rust via https://rustup.rs (MSVC host; rust-toolchain.toml pins 1.99.1)"
Need node   "install Node.js >= 18 (20 LTS recommended)"
Need npm    "comes with Node.js"
& cargo --version
& node --version

# MSVC probe: the linker must be reachable. If cl.exe is not on PATH this is
# normal outside a Developer Prompt — cargo finds MSVC through vswhere, so we
# probe that instead and give a precise fix if absent.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
  $msvc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if (-not $msvc) {
    Write-Warning "No VS 'Desktop development with C++' workload detected — the build will fail if MSVC is really absent."
  } else {
    Write-Host "MSVC toolset: $msvc"
  }
} else {
  Write-Warning "vswhere not found — if the build fails with 'link.exe not found', install the VS 2022 Build Tools C++ workload (docs/BUILDING.md)."
}

# WebView2 runtime (needed to RUN, not build) — informational probe.
$wv2 = Get-ItemProperty -Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
if ($wv2) { Write-Host "WebView2 runtime: $($wv2.pv)" } else { Write-Warning "WebView2 runtime not found (needed to run the app, not to build it)." }

# ── 2. Optional core gate ───────────────────────────────────────────────────
if ($Core) {
  Step "Core workspace test gate"
  Push-Location $Repo
  & cargo test --workspace
  if ($LASTEXITCODE -ne 0) { Write-Error "cargo test failed"; exit 1 }
  Pop-Location
}

# ── 3. Build ────────────────────────────────────────────────────────────────
Step "Building the editor app (Tauri)"
Push-Location $App
& npm --prefix ui ci
if ($LASTEXITCODE -ne 0) { Write-Error "npm ci failed"; exit 1 }
$env:Path = "$App\ui\node_modules\.bin;$env:Path"
if (-not (Get-Command tauri -ErrorAction SilentlyContinue)) {
  Write-Error "tauri CLI missing from ui\node_modules\.bin after npm ci"; exit 1
}

if ($BinOnly) {
  & tauri build --no-bundle
  if ($LASTEXITCODE -ne 0) { Write-Error "tauri build failed"; exit 1 }
  $bin = Join-Path $App "target\release\melography-app.exe"
  Step "Artifact"
  Get-Item $bin | Format-List Name, Length, LastWriteTime
  Get-FileHash $bin -Algorithm SHA256
} else {
  & tauri build --bundles $Bundles
  if ($LASTEXITCODE -ne 0) { Write-Error "tauri build failed"; exit 1 }
  Step "Artifacts (unsigned local build — see docs/RELEASING.md for the signed M5 flow)"
  Get-ChildItem -Recurse (Join-Path $App "target\release\bundle") -File |
    Where-Object { $_.Extension -in ".exe", ".msi" } |
    ForEach-Object {
      Write-Host ("{0}  {1:N0} bytes" -f $_.FullName, $_.Length)
      Get-FileHash $_.FullName -Algorithm SHA256 | Select-Object -ExpandProperty Hash
    }
}
Pop-Location
Write-Host "`nDone. Unsigned installers: run the -setup.exe and accept the SmartScreen prompt ('More info' → 'Run anyway'), or import this build's certificate for local testing."
