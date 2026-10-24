# MELOGRAPHY — Building the app (scripts)

**Version:** M4 session 10 (issue #25) · **Audience:** contributors and the
owner building local installers · **Authority for releases:** `docs/RELEASING.md`
(the scripts below build **unsigned local artifacts only** — every signing,
notarization and publish step is owner-gated at M5 and never happens here).

The README §"Building from source" documents the same steps by hand; these
scripts automate them per platform. All three build the Tauri 2 editor app
(`apps/melography-app`) with the pinned Rust toolchain and the committed
UI lockfile, then list the artifacts with checksums.

## Invocation directory matters (by-hand builds)

On startup the Tauri CLI resolves two directories: the **Tauri dir** — the
folder holding `tauri.conf.json`, `apps/melography-app` in this repo — and
the **frontend dir** — the nearest `package.json`, `apps/melography-app/ui`.
The `beforeBuildCommand`/`beforeDevCommand` hooks run **in the frontend
dir**, and `frontendDist` is relative to the **Tauri dir** (`ui/dist`).
That is why every script (and the README) invokes `tauri build`/`tauri dev`
from `apps/melography-app`: from the repo root the CLI cannot see
`ui/package.json` within its search depth and falls back to the wrong
frontend dir (first symptom: `npm error path …/ui/ui/package.json`).

## Linux — `scripts/build/build-linux.sh`

Supports the three distro families CI and the README cover: **Ubuntu/Debian**
(apt), **Fedora** (dnf), **Arch** (pacman). The distro is detected from
`/etc/os-release`; the dependency step needs `sudo` and can be skipped.

```sh
scripts/build/build-linux.sh                    # deps + deb,rpm,appimage
scripts/build/build-linux.sh --no-install       # skip the package step
scripts/build/build-linux.sh --bundles appimage # one target only
scripts/build/build-linux.sh --bin-only         # executable, no installer
scripts/build/build-linux.sh --core             # cargo test --workspace first
MELO_BUNDLES="deb" scripts/build/build-linux.sh # env override
```

System packages installed per distro (same sets as README §Linux):

| Distro | Packages |
|---|---|
| Ubuntu/Debian | `build-essential pkg-config curl file libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev` |
| Fedora | `gcc gcc-c++ glibc-devel pkgconf-pkg-config openssl-devel webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel` |
| Arch | `base-devel pkgconf webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg` |

Artifacts land in `apps/melography-app/target/release/bundle/`
(`.AppImage`, `.deb`, `.rpm`). The `.deb`/`.rpm` packages do not
self-update (stated in-app; RELEASING §7).

## Windows — `scripts/build/build-windows.ps1`

PowerShell 5.1+ (the Windows default). Prerequisites the script cannot
install itself: **VS 2022 Build Tools** with the *Desktop development with
C++* workload, **Rust (MSVC host)**, **Node.js ≥ 18**. WebView2 is probed
informationally (needed to run, not to build).

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build\build-windows.ps1
powershell ... -File scripts\build\build-windows.ps1 -Bundles nsis   # NSIS setup only
powershell ... -File scripts\build\build-windows.ps1 -BinOnly        # .exe only
powershell ... -File scripts\build\build-windows.ps1 -Core           # tests first
```

Artifacts: `target/release/bundle/nsis/*-setup.exe` and
`target/release/bundle/msi/*.msi` (Tauri downloads NSIS/WiX on demand).
Unsigned installers trigger SmartScreen — "More info" → "Run anyway" for
local builds.

## macOS — `scripts/build/build-macos.sh`

macOS 12+; needs Xcode Command Line Tools (`xcode-select --install`),
Rust, Node. Native by default; `--arch universal` adds the second Rust
target and produces a fat `.app`/`.dmg`.

```sh
scripts/build/build-macos.sh                     # native: .app + .dmg
scripts/build/build-macos.sh --arch universal    # aarch64 + x86_64
scripts/build/build-macos.sh --arch x64          # Intel target on Apple Silicon
scripts/build/build-macos.sh --bin-only
MELO_BUNDLES="app" scripts/build/build-macos.sh  # .app only, no .dmg
```

Unsigned `.app` first launch: right-click → Open. Codesigning and
notarization use owner-provisioned certificates at M5 (RELEASING §3).

## What these scripts deliberately do NOT do

* **No signing keys.** Updater signatures, Authenticode, and Apple
  notarization all require owner-held material (RELEASING §3) and live in
  the gated M5 release workflow only.
* **No publishing.** Nothing is uploaded, tagged, or attached to a
  Release. `latest.json` manifests are generated (and schema-checked) by
  the M5 workflow, never hand-written.
* **No telemetry, no network fetches beyond** the toolchain/package
  managers shown above and Tauri's on-demand NSIS/WiX download.

## Troubleshooting

* **`link.exe not found` (Windows)** — the VS C++ workload is missing or
  the script ran from a shell without the MSVC environment; install the
  workload (docs + `vswhere` probe output above tell you which).
* **`webkit2gtk-4.1` package not found (older Fedora/Ubuntu)** — the
  4.1 API split arrived with Fedora 38+/Ubuntu 23.04+; on older releases
  upgrade, or build the core crates + CLI only (they need no webview).
* **First build is slow** — the wry/wgpu dependency trees are large;
  subsequent builds are incremental. Disk budget ≥ 4 GB (README).
* **Headless build servers (no webkit headers)** — the app still
  type-checks: `cargo check --manifest-path apps/melography-app/Cargo.toml
  --no-default-features` (this is what the sandbox verification uses).
