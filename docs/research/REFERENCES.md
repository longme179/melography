# References Registry

**All entries verified via GitHub REST API / crates.io API on 2026-10-24 (UTC).**
Pinned `head_sha` = HEAD of the default branch **at verification time** (12-hex prefix; full SHAs in `references/manifest.toml`). Repositories move — `scripts/fetch_refs.sh` re-verifies pinned revisions on demand.

**Usage policy legend** (per ADR-0003, project = GPL-3.0-or-later):
- **REUSE** — license permits code reuse in this project (GPL-3.0-compatible), subject to header preservation + `THIRD_PARTY_NOTICES.md` + `tools/provenance.json`.
- **STUDY** — read for algorithms/architecture only; no code copying (license incompatible, absent, or assets separately restricted).
- **CONVENTION** — format/convention reference; do not copy content.

⚠️ **Assets are not code.** LUT files, film scans, ICC profiles, datasets and model weights carry licenses independent of the repository they live in (verified example below: spektrafilm code = GPL-3.0, its LUTs = custom "no resale" license). Every asset is vetted individually before redistribution regardless of the repository's code license.

## A. Owner-specified skill / methodology repositories

| Repository | License | Pinned | Usage | Notes |
|---|---|---|---|---|
| [codecrafters-io/build-your-own-x](https://github.com/codecrafters-io/build-your-own-x) | README declares **CC0** (no top-level LICENSE file detected via API; treat as declared-by-readme) | `master@aa17439b62f3` | CONVENTION | Educational index of tutorials; linked tutorials carry **their own licenses** — never assume sub-linked content is CC0. Use for first-principles learning topics only. |
| [obra/superpowers](https://github.com/obra/superpowers) | MIT (LICENSE at top level) | `main@2ce8aa15d05b` | REUSE (methodology) | 15 skills incl. `brainstorming`, `writing-plans`, `test-driven-development`, `requesting-code-review`, `verification-before-completion`, `systematic-debugging`. We adapt the methodology into project-authored skills; we do **not** install its plugin hooks or run its installers. Optional visual-companion telemetry: set `SUPERPOWERS_DISABLE_TELEMETRY=1` if ever executed (verify against upstream at that time). |
| [leonxlnx/taste-skill](https://github.com/leonxlnx/taste-skill) | MIT | `main@6010f91a9935` | REUSE (design judgment) | Single skill `skills/taste-skill` + `install.sh` (never executed; we read the skill file only). Design-judgment principles are web-oriented; apply only where they do not conflict with desktop color-critical UI requirements (see `skills/desktop-ui-design/SKILL.md`). |

## B. Film & color science

| Repository | License | Pinned | Usage | Notes |
|---|---|---|---|---|
| [darktable-org/darktable](https://github.com/darktable-org/darktable) | GPL-3.0 | `master@d01c1527dcbe` | REUSE | Verified `src/iop/filmic.c`, `src/iop/negadoctor.c`, `src/iop/grain.c` exist. Primary references for display-referred→scene-referred reasoning (filmic), negative inversion math (negadoctor), grain placement. |
| [RawTherapee/RawTherapee](https://github.com/RawTherapee/RawTherapee) | GPL-3.0 | `dev@b2ee504c47e5` | REUSE | HaldCLUT film simulation path; ICC handling; `RawPedia` docs (repo `RawTherapee/RawPedia`, no license file detected → treat docs as study-only). |
| [CarVac/filmulator-gui](https://github.com/CarVac/filmulator-gui) | NOASSERTION (LICENSE + LICENSE.txt both present; file-level review required) | `master@57fbaec57555` | STUDY | Filmulation algorithm (film + developer simulation from raw exposure). Licensing ambiguity blocks copying until reviewed. |
| [andreavolpato/spektrafilm](https://github.com/andreavolpato/spektrafilm) | GPL-3.0 (code) — **LUTs under custom license: "commercial use, free share, no resale" (SPEKTRAFILM_LICENSE.txt)** | `main` (see manifest) | REUSE (code) / **STUDY (LUT assets)** | Verified via README + license file: current lineage of the original `agx-emulsion`. Spectral film simulation. Its LUT assets must never be redistributed with MELOGRAPHY. |
| [aaronmurniadi/agx-emulsion](https://github.com/aaronmurniadi/agx-emulsion) | GPL-3.0 | `main` (see manifest) | REUSE | Original emulsion-simulation project (2 stars, dormant since 2026-05); spektrafilm is the maintained continuation. |
| [colour-science/colour](https://github.com/colour-science/colour) | BSD-3-Clause | `develop@e82dd140c02d` | REUSE | Reference implementations of colorimetry: CCT, chromatic adaptation, ΔE2000, OETF/EOTF. Python → port carefully to Rust with golden tests. |
| [AcademySoftwareFoundation/OpenColorIO](https://github.com/AcademySoftwareFoundation/OpenColorIO) | BSD-3-Clause | `main@22dd9c4faa0d` | STUDY→REUSE possible | ACEScg (AP1) definitions, displays/transforms concepts. OCIO v2 config model informs our pipeline stage contracts. |
| [AcademySoftwareFoundation/OpenImageIO](https://github.com/AcademySoftwareFoundation/OpenImageIO) | Apache-2.0 | `main@fde6512a27ea` | STUDY | Color-management-aware image I/O conventions; ICC/metadata edge-case handling patterns. |
| [mm2/Little-CMS](https://github.com/mm2/Little-CMS) | MIT | `master@1112313b3784` | REUSE (via `lcms2` crate) | The actual ICC CMM we plan to embed. Rust bindings: `lcms2` crate 6.2.0 (MIT, updated 2026-08-26) + `lcms2-sys` 4.0.7 — verified on crates.io. |
| [GreycLab/gmic](https://github.com/GreycLab/gmic) | NOASSERTION (CeCILL-family expected; file-level review required before any reuse) | `master@c79f54dfb9a7` | STUDY | Film-look filters as algorithm inspiration only. |
| Published papers: **Newson, Faraj, Delon, Galerne — "A stochastic film grain model for resolution-independent rendering"** (plus IPOL demonstration) | Paper copyright respective owners; IPOL demos typically CC-BY (verify on IPOL site at M2) | n/a | STUDY | Basis of our grain model (see `docs/FILM_MODEL.md` §G and `docs/research/grain.md`). Equations from papers are re-implemented, not code-copied, unless an IPOL code drop is separately cleared. |

## C. Comparable applications

| Repository | License | Pinned | Usage | Notes |
|---|---|---|---|---|
| [cybertimon/rapidraw](https://github.com/cybertimon/rapidraw) | **AGPL-3.0** | `main@d3374da05c84` | **STUDY** | Closest comparable (Rust, high-performance editing). AGPL-3.0 is excluded from our reuse policy (ADR-0003): architecture comparison only — module split, GPU strategy, LUT handling. **Do not copy code.** |

## D. Rust ecosystem & desktop stack

| Component | License | Version @ 2026-10-24 | Usage | Notes |
|---|---|---|---|---|
| [gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) | MIT OR Apache-2.0 | crate 30.0.1 (2026-08-22) / `trunk@63e505290e82` | REUSE | GPU compute + WGSL shader validation. |
| [image-rs/image](https://github.com/image-rs/image) | MIT OR Apache-2.0 | crate 0.25.10 / `main@075e65d618a3` | REUSE | Baseline decoders (PNG/JPEG/WebP/TIFF); codec-by-codec capability matrix to be built at M0 (heif/avif via separate crates — unverified until M0 spike). |
| exif: repo `kamadak/exif` → **renamed/redirects to `kamadak/exif-rs`**; crate `kamadak-exif` | BSD-2-Clause | crate 0.6.1 (2024-11-06) | REUSE | EXIF read/write in pure Rust; staleness (12 months) noted — alternatives: `mindeng/nom-exif` (MIT, active). Decide at M0. |
| [tauri-apps/tauri](https://github.com/tauri-apps/tauri) | MIT OR Apache-2.0 | `dev@c464ef928c35` | REUSE | Chosen UI shell (ADR-0001, conditional on Spike S1). |
| [tauri-apps/plugins-workspace](https://github.com/tauri-apps/plugins-workspace) | MIT OR Apache-2.0 | `v2@c75459d8996b` | REUSE | Updater plugin (v2) — signature model verified at M0 Spike S4. |
| [tauri-apps/tauri-action](https://github.com/tauri-apps/tauri-action) | MIT | `dev@a6e90ddc4ba4` | REUSE | Release packaging action (gated; never runs without owner approval). |
| [emilk/egui](https://github.com/emilk/egui) | MIT OR Apache-2.0 | `main@bf5ac0dac137` | REUSE (not selected; fallback option B in ADR-0001) | |
| [iced-rs/iced](https://github.com/iced-rs/iced) | MIT | `master@cd04b74ca707` | REUSE (not selected) | |
| [slint-ui/slint](https://github.com/slint-ui/slint) | NOASSERTION (LICENSE.md + LICENSES/ tree; royalty-free-desktop / GPL dual scheme — complex) | `master@b1130481f31d` | STUDY | Not selected; licensing complexity noted in ADR-0001. |
| [pykeio/ort](https://github.com/pykeio/ort) | MIT OR Apache-2.0 | crate 2.0.0-rc.15 (2026-10-19) / `main@4fac49ba41dc` | REUSE | ONNX Runtime bindings; RC status tracked as a risk (ADR-0004). |

## E. AI models & research

| Resource | License (code) | Weights license | Usage | Notes |
|---|---|---|---|---|
| [HuiZeng/Image-Adaptive-3DLUT](https://github.com/HuiZeng/Image-Adaptive-3DLUT) | Apache-2.0 | **unverified — must be checked at M3** | STUDY→candidate | Image-adaptive 3D-LUT prediction concept; a derived M9-LUT ONNX export exists third-party but is not vetted. |
| [Li-Chongyi/Zero-DCE](https://github.com/Li-Chongyi/Zero-DCE) | **NONE (no license file)** | NONE | **STUDY only — cannot ship** | 1,087 stars. Code and weights have no license → no reuse, no redistribution. Zero-DCE++ paper equations may be re-implemented (paper ≠ code license; verify paper terms at M3). |
| [ChaoningZhang/MobileSAM](https://github.com/ChaoningZhang/MobileSAM) | Apache-2.0 | weights under separate terms — verify at M3 | candidate | Small subject segmentation. |
| [chongzhou96/EdgeSAM](https://github.com/chongzhou96/EdgeSAM) | NOASSERTION (LICENSE present; content review pending) | unverified | candidate | Edge-optimized SAM variant. |
| [xuebinqin/U-2-Net](https://github.com/xuebinqin/U-2-Net) | Apache-2.0 | unverified (research weights often non-commercial) | candidate | Saliency/subject matting fallback. |
| [DepthAnything/Depth-Anything-V2](https://github.com/DepthAnything/Depth-Anything-V2) | Apache-2.0 | unverified | candidate | Small depth models (deferred; optional). |
| [microsoft/onnxruntime](https://github.com/microsoft/onnxruntime) | MIT | n/a | REUSE (via `ort`) | EP support matrix per platform verified at M0 Spike S3. |
| SCI (Self-Corrective Illumination) | canonical repo not located in this session | n/a | deferred | Recorded as **not verified**; locate + vet at M3. |
| [anthropics/skills](https://github.com/anthropics/skills) | **NONE (no license file detected)** | n/a | CONVENTION | SKILL.md format reference only — do not copy content. |

## F. Verification commands used (reproducible)

- `GET /repos/{repo}` → license, default branch, archived, stars, pushed_at (script: `/home/z/my-project/scripts/audit_refs.sh` pattern, retained in session log summary)
- `GET /repos/{repo}/commits/{branch}` → pinned HEAD SHA
- `GET /repos/{owner}/{repo}/contents/...` → license-file presence, skill listings, module existence
- `GET https://crates.io/api/v1/crates/{name}` → crate version/license/freshness

## G. "Not verified / open" items (honest gaps)

1. HEIC/AVIF decoder crates for Rust — no capability/license verification performed yet (M0 task, Spike S2 scope extension).
2. Exact Tauri v2 updater signing key model (minisign-style keypair) — cited from memory, must be confirmed against current `plugins-workspace` docs at M0 Spike S4.
3. Model weights licenses (all "unverified" above) — blocking for M3 model selection; each requires the per-model manifest of ADR-0004 before any download or bundling.
4. filmulator-gui and gmic file-level license texts — read before any code reuse from them.
5. IPOL film-grain demo code license — check at M2.
