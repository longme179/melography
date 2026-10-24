# MELOGRAPHY

Offline-first desktop image editor that makes ordinary (non-RAW) photographs convincingly resemble analog film photographs — built on a **Rust image-processing core**, running locally on **Linux, Windows, and macOS**.

> **Status: M0–M2 merged to `main`** — the color-managed pipeline (decode → AP1 → edit → export), the `.cube` LUT workflow, and the film-emulation core (stages A–I, five artist-authored stock profiles, deterministic grain, CPU↔GPU parity gates) are in the tree and exercised by CI on all three OSes. The Tauri editor shell opens photos, previews LUT + film looks, and exports JPEG/PNG/TIFF. **No binary release has been published yet** — build from source (below). AI assistance (auto-lighting + optional local models) is the current milestone; see [`docs/PLAN.md`](docs/PLAN.md).

## Principles

- **Offline-first, privacy by default** — no accounts, no telemetry, no uploads; the only optional network activity is explicit update checks / model downloads with user consent.
- **Non-destructive** — original image files are never modified; edits live in versioned sidecar recipes.
- **Rust core** — all pixel processing happens in Rust (GPU via wgpu/WGSL where available, deterministic CPU fallback everywhere).
- **Local-only AI** — optional small ONNX models for lighting suggestions and masks; CPU always supported.
- **English + Vietnamese UI** from the first beta; accessibility and high-DPI are first-class.

## V1 non-goals

RAW development, video, mobile apps, accounts, cloud editing/sync, generative scene alteration, subscription infrastructure, plugin marketplace, full DAM/catalog, printer driver support.

## Building from source

Everything builds from a plain `git clone` on all three desktop platforms. Three toolchains are involved: **Rust** (the processing core and app host), **Node.js** (the in-app webview UI build), and each OS's native C/C++ toolchain (required by the webview and crypto stacks).

### Prerequisites (all platforms)

| Tool | Version | Notes |
|---|---|---|
| Git | any recent | |
| [Rust](https://rustup.rs) | **1.99.1** | pinned by [`rust-toolchain.toml`](rust-toolchain.toml); rustup installs/uses the pin automatically — no manual version juggling |
| Node.js + npm | **≥ 18** (20 LTS recommended) | only needed for the editor app's UI; [Vite 5](apps/melography-app/ui/package.json) sets the floor |
| C/C++ toolchain + linker | per-OS, see below | pulled in by the webview (Tauri) and `lcms2`/`ring` build scripts |
| Free disk | ≥ 4 GB | `target/` dirs for the workspace + app trees get large (wgpu and wry dependency trees) |

No other language runtimes are required. Python is used only by developer tooling (`scripts/`, `tools/`) and CI, never to build the product.

### Linux (x86_64: Debian 12+, Ubuntu 22.04+, and equivalents)

The editor app links WebKitGTK 4.1 / GTK 3 at build and run time. On Debian/Ubuntu (the exact set CI uses):

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential pkg-config curl file libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

On other distributions install the equivalent packages: a C/C++ toolchain (`base-devel` / `gcc-c++`), `pkg-config`, WebKitGTK **4.1** development files, GTK 3 development files, the Ayatana (or legacy) appindicator development files, `librsvg` development files, and OpenSSL development files. For example on Arch:

```sh
sudo pacman -S --needed base-devel pkgconf webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg
```

The processing core (`crates/*`) and CLI need none of the webview packages — only `build-essential` + `pkg-config` (+ `libssl-dev` where a dependency's build script wants it).

### Windows (10 19041+ / 11, x64)

1. **Visual Studio 2022 Build Tools** with the *Desktop development with C++* workload (MSVC v143 toolset + Windows 10/11 SDK) — [aka.ms/vs/17/release/vs_BuildTools.exe](https://aka.ms/vs/17/release/vs_BuildTools.exe). The MSVC linker is required; the GNU toolchains are not supported by the wry/webview2 stack.
2. **Rust via rustup** — accept the default host triple `x86_64-pc-windows-msvc` (rustup pairs it with the Build Tools above).
3. **WebView2 Runtime** — preinstalled on Windows 11 and current Windows 10; if missing, install the [Evergreen runtime](https://developer.microsoft.com/microsoft-edge/webview2/). Only needed to *run* the app, not to build it.
4. Node.js ≥ 18 (LTS recommended).

### macOS (12+, Apple Silicon and Intel)

1. **Xcode Command Line Tools**: `xcode-select --install` (the full Xcode app is not required).
2. **Rust via rustup** (native target for your Mac; both `aarch64-apple-darwin` and `x86_64-apple-darwin` are supported).
3. Node.js ≥ 18 (LTS recommended). The app uses the system `WKWebView` — nothing extra to install.

### 1. Core workspace — build, test, CLI

```sh
git clone https://github.com/longme179/melography.git
cd melography
cargo test --workspace        # fmt+clippy+deny run in CI; locally this is the main gate
cargo build --release         # release binaries under target/release/
```

The workspace covers `crates/` (core, color, io, lut, film, gpu, ai, presets, cli) plus two spikes. Three crates are **deliberately excluded** from the root workspace and build standalone (see [`Cargo.toml`](Cargo.toml)): the Tauri app (below), `spikes/s4-updater`, and `spikes/heif-probe` (needs `libheif-dev` + HEVC/AV1 decoder plugins — only for the optional HEIC/AVIF format evidence).

The headless CLI exercises the same pipeline the app uses:

```sh
cargo run -p melography-cli -- chart --out chart.png            # synthetic test chart
cargo run -p melography-cli -- roundtrip --input photo.jpg      # decode→AP1→encode
cargo run -p melography-cli -- lut parse --file look.cube       # strict .cube validation
cargo run -p melography-cli -- apply --input in.jpg --lut look.cube --output out.jpg
cargo run -p melography-cli -- film list                        # stock profile registry
cargo run -p melography-cli -- film apply --input in.jpg --stock warm-400 --output out.png
```

Heavy film-statistical gates (grain RMS/PSD, tiled≡monolithic, determinism) are `#[ignore]`d release-mode tests; CI runs them explicitly:

```sh
cargo test --release -p melography-film --test gates -- --ignored
```

GPU parity tests (wgpu↔CPU) run where a real adapter exists (CI: macOS Metal, Windows WARP) and report an honest skip otherwise.

### 2. Editor app — development run

The app is a Tauri 2 host (`apps/melography-app`) with a Svelte + TypeScript webview UI (`apps/melography-app/ui`). The webview never computes pixels — it renders previews produced by the Rust core (ADR-0001).

```sh
cd apps/melography-app
npm --prefix ui ci                     # UI deps incl. the pinned @tauri-apps/cli
PATH="$PWD/ui/node_modules/.bin:$PATH" tauri dev     # POSIX shells
# Windows PowerShell:  $env:Path = "$pwd\ui\node_modules\.bin;$env:Path"; tauri dev
```

Alternative if you prefer a global CLI (any OS): `cargo install tauri-cli --version 2 --locked`, then `cargo tauri dev` from `apps/melography-app`.

`tauri dev` starts the Vite dev server (`localhost:5173`) and launches the debug app binary with hot UI reload. On a machine **without webkit development headers** (build servers, containers), the app still type-checks headless: `cargo check --manifest-path apps/melography-app/Cargo.toml --no-default-features`.

### 3. Release build & deploy

```sh
cd apps/melography-app
npm --prefix ui ci
PATH="$PWD/ui/node_modules/.bin:$PATH" tauri build --no-bundle
# binary: target/release/melography-app (Linux), melography-app.exe (Windows),
#         melography-app (macOS)
```

- **Installers/bundles (AppImage, `.dmg`, `.exe` setup) are intentionally disabled until milestone M5** (`bundle.active = false` in [`tauri.conf.json`](apps/melography-app/tauri.conf.json)) — see [`docs/RELEASING.md`](docs/RELEASING.md) for the planned artifact matrix, signing layers, and updater channels. `--no-bundle` builds just the executable.
- **Runtime dependencies of the shipped binary**: Linux — `libwebkit2gtk-4.1`, `libgtk-3` (the same runtime set every WebKitGTK app needs; the packages above provide it); Windows — WebView2 Runtime; macOS — none beyond the OS.
- **Updater/signing**: no update channel exists yet. When it does, artifacts are signed by the owner-held keypair and served exclusively from this repository's GitHub Releases (ADR-0005).
- **Models**: the app ships with **zero bundled AI models**; optional local models (M3+) are opt-in downloads, checksum-verified, never required (ADR-0004).

### Platform support matrix

| OS | Status | Verified by |
|---|---|---|
| Linux x86_64 (Debian/Ubuntu; others best-effort) | core+CLI+app built and tested in CI | CI `rust-tests` + `app-build` jobs |
| Windows 10/11 x64 | core+CLI built and tested in CI; app follows the standard Tauri v2 MSVC toolchain above | CI `rust-tests` |
| macOS 12+ (arm64 + x64) | core+CLI built and tested in CI; app follows the standard toolchain above | CI `rust-tests` (Metal adapter runs the GPU parity gates) |

Honest gaps: owner-owned reference machines are not yet declared, so GUI smoke tests on real Windows/macOS hardware are part of the M5 beta checklist (see `docs/PLAN.md` §4/§5).

## Repository layout

```
apps/melography-app/    Tauri 2 editor shell (Rust host + Svelte/TS UI in ui/)
crates/core/            shared types, tiling/executor primitives
crates/color/           ICC/lcms2, AP1/scene-referred transforms (S1/S5)
crates/io/              decode/encode (JPEG/PNG/TIFF/WebP), orientation, ICC
crates/lut/             .cube parsing, tetrahedral interp, WGSL mirror
crates/film/            film emulation stages A–I + §L statistical gates
crates/gpu/             wgpu device, WGSL kernels, CPU↔GPU parity harness
crates/ai/              model manifest + vetting gate (ADR-0004); inference at M3
crates/presets/         sidecar recipes, stock profiles, LUT library
crates/cli/             melography-cli — headless pipeline + shared pipeline lib
spikes/                 S1 transport · S3 ort · S4 updater · HEIF probe (evidence)
docs/                   PLAN, ARCHITECTURE, COLOR_PIPELINE, FILM_MODEL, AI_MODELS,
                        UX_SPEC, RELEASING, ADRs, quality reports, agent logs
profiles/               five artist-authored film stock profiles (JSON, schema-checked)
schemas/                JSON schemas (stock profiles, sidecars)
tools/                  calibration/fitting harness (H&D, crosstalk, look-match)
skills/                 project-authored engineering workflows (skills/README.md)
```

## License

**GPL-3.0-or-later** — selected by the project owner on 2026-10-24 (recorded in `docs/adr/0003-licensing.md`); the full text is in [`LICENSE`](LICENSE). This project is a personal, non-commercial passion project and reuses code from GPL-3.0 projects (darktable, spektrafilm, agx-emulsion) and permissively licensed libraries (MIT / Apache-2.0 / BSD), with original copyright headers preserved and attribution tracked in `THIRD_PARTY_NOTICES.md`. Non-code assets (LUT files, film scans, model weights) are licensed independently from source code and are vetted individually before distribution.
