# MELOGRAPHY — Development Plan

**Version:** Phase-0 (2026-10-24) · **Execution units:** GitHub issues/milestones (linked once created)
**Effort scale:** S ≤ 2 focused days · M ≤ 1–2 weeks · L ≤ 1 month+ (solo agent+owner cadence; **no calendar promises** — relative effort + uncertainty only).

## 0. Current position

Phase 0 (this PR) delivers research, ADRs, specs, skills, CI skeleton, memory protocol. **M0–M5 require owner approval before starting** (approval sought in the Phase-0 PR; OPEN_QUESTIONS #2).

## 1. Milestone dependency graph

```mermaid
flowchart TD
  P0[P0: research+architecture ✅ draft PR] -->|owner approval| M0
  M0[M0: skeleton + 4 risk spikes] --> M1[M1: LUT workflow MVP]
  M1 --> M2[M2: Film lab core]
  M0 --> M2
  M2 --> M3[M3: AI assistance]
  M2 --> M4[M4: UX/polish/hardening]
  M3 --> M4
  M4 --> M5A[M5-A: depth & speed (owner asks)]
  M5A --> M5B[M5-B: beta v0.1]
  M5B -->|owner approval| R1[First stable release]
```

## 2. Milestones (scope · tasks · exit criteria)

### M0 — Functional skeleton + risk spikes (effort M, uncertainty medium)
**Scope:** prove/disprove the four architecture assumptions; establish the test spine.
1. Cargo workspace bootstrap (`crates/` per ARCHITECTURE §2, `rust-toolchain.toml` pinned; fmt/clippy/deny/test gates activate in CI).
2. `melography-cli` headless: decode → ICC→AP1 → (identity film chain) → display/export roundtrip on a synthetic chart.
3. **Spike S1** preview transport (Tauri webview vs native child-window; thresholds ADR-0001) → confirms/overturns ADR-0001.
4. **Spike S2** lcms2 cross-platform golden consistency + webview double-transform detection (COLOR_PIPELINE §5).
5. **Spike S3** ort CPU packaging on 3 OSes (bundle size, cold init, dummy-model latency).
6. **Spike S4** Tauri v2 updater dummy channel roundtrip in CI (throwaway key, **test channel only, no public release**); verify `/releases/latest` semantics live.
7. Benchmark hardware pinning: declare reference machines; measure decode/render baselines; recalibrate budgets (§4).
8. Codec capability matrix (JPEG/PNG/TIFF/WebP verified; HEIC/AVIF go/no-go with build evidence).

**Exit:** all 4 spike reports with evidence; CLI roundtrip golden test green on Linux/Windows/macOS CI; budget table re-pinned from measurements; ADR-0001 confirmed or superseded. **Deferred:** any product UI beyond a shell with one canvas.

### M1 — LUT workflow MVP (effort M, uncertainty low)
**Scope:** the first *user-visible* end-to-end slice (small but complete).
Tasks: `.cube` parser+validator (luts.md §1) · tetrahedral CPU + WGSL + independent oracle · intensity/mix + insertion points with encoding contracts · LUT library (content-addressed, provenance) · editor shell (S2 minimal: open/preview/compare/save recipe/export) · error UX for LUT states · sidecar v1 + fingerprint policy.
**Exit:** a user imports a real `.cube`, previews on a real photo with correct declared encodings, before/after compares, exports JPEG/PNG/TIFF — original untouched; CPU/GPU parity ≤ tolerance; fuzz corpus green; cross-platform serialization stable.

### M2 — Film lab core (effort L, uncertainty high — the differentiator) — **EXECUTED on `agent/m2-film-core` (see docs/quality/M2_FILM_QUALITY_REPORT.md)**
**Scope:** FILM_MODEL.md implemented stage-by-stage with tests-first on CPU oracle.
Tasks: stages A–I (curves+crosstalk first, then halation, grain, print/scan) · stock profile schema + 5 original artist-authored profiles · deterministic grain (statistical gates) · tiled-vs-monolithic suite · fitting/calibration harness (`tools/`) · golden film-chain cases · Film Lab panel UI (P3 depth) + stock browser (S3) · internal quality report published in-repo.
**Exit:** algorithmic gates 100% green (FILM_MODEL §L); 5 profiles loadable with provenance labels; preview p50 ≤ 50 ms budget on reference hardware (or documented shortfall + fast-path plan); export 24MP targets measured honestly.

### M3 — AI assistance (effort M, uncertainty high — licensing gate) — **EXECUTED on `agent/m3-ai-assist` (see docs/quality/M3_AI_REPORT.md)**
**Scope:** non-ML baseline first; one vetted model only if it beats baseline.
Tasks: baseline auto-lighting (AI_MODELS §2) · model manager + manifest gate + downloader · one parameter-predictor evaluation with measured benchmark vs baseline · optional segmentation mask (if gates pass) · suggestion UX (Accept/Adjust/Dismiss + provenance in sidecar) · mask-edge golden tests.
**Exit:** suggestions are editable parameters (never baked pixels); offline mode fully functional with no models installed; every shipped model has a complete manifest with verified weights license; budgets met on reference hardware.
**Outcome:** all exit criteria met — Tone stage (editable params + provenance), baseline-only v1 (no model passed the gate — the honest ADR-0004 §7 outcome), manager fully tested offline, budgets 49–61× under (60.6 ms vs 3 000 ms @ 12 MP). Masks deferred with the gate-blocked candidates; the benchmark (baseline 0.205 vs naive-ONNX 1.019) is CI-enforced as the bar.

### M4 — Production UX polish & hardening (effort L, uncertainty medium) — **IN PROGRESS on `agent/m4-ux-hardening` (sessions 06–07; issues #23 #24 #25 + #15)**
**Scope:** everything that makes it a product for P1–P5.
Tasks: full panel set per UX_SPEC · batch export queue · undo/redo + autosave + crash-safe sidecars · preset manager · EN/VI complete catalogs + pseudo-locale CI · accessibility pass (keyboard-only, SR announcements, contrast) · high-DPI · performance (cache budgets, memory ≤ 2 GiB @ 50MP) · security hardening (fuzz decoders/LUT/sidecar; secret scan; provenance tooling) · error-state inventory complete.
**Exit:** persona walkthroughs pass; crash-safety tests (kill during save/export/update) green; a11y audit checklist green; EN/VI screenshots complete.
**Progress (session 06):** session-model slice (#24 core) done — shared durable-write primitive `core::durable` (kills the 3 duplicated implementations; fixes the presets Windows no-fsync honesty gap) + real SIGKILL crash battery in CI (PLAN exit criterion "kill during save" — green); undo/redo (Ctrl+Z/Y, toolbar, panel refresh, bounded 100-step history, session-scoped) + debounced autosave (dirty-badge indicator, never modal) + open-time sidecar litter recovery.
**Progress (session 07, #23 part 1):** screens slice 1 done — settings/recents/user-preset stores (durable, corrupt-safe); **film-resume gap fixed** (FilmStock stages were saved but never re-applied on re-open — one serialization + one apply path now, missing stock = amber badge); Color panel Temp/Tint via new `color::gains_from_cct_tint` (Planckian-locus synthesis, the documented tone.rs follow-up); S1 Home (recents+thumbs+drag-drop+format chips+inline error rows), S5 Preset manager, S7 Settings (live language/theme, offline explainer); panel rail per §4 with honest not-available rows (no fake sliders); filmstrip (live per-stock thumbs); keyboard map §8 complete (incl. Z hold-100% loupe); headless `--no-default-features` check repaired (broken since M1).
**Progress (session 08, #23 part 2 — issue #23 COMPLETE, PR #28 merged):** S6 batch export queue (sequential worker thread decoupled from the editor; jobs carry their own stages so **retry keeps settings**; collision-safe outputs; per-file actionable errors — disk full → folder chooser & retry; global pause/cancel with honest "current file finishes" semantics; `batch://update` events); **ONE standalone export path** shared by single export and the queue — fixed two preview≠export inconsistencies (disabled film silently dropped the LUT stack; a disabled LUT was applied on export) and surfaced `export.lut_skipped_by_film` as an honest warning; WB eyedropper (`wb_pick` — the same `analysis_gains_for_gray` as auto-lighting, one WB path, undo-able); S6 screen + Toolbar badge + ExportDialog add-to-batch + Home multi-drop→queue; **persona walkthroughs P1–P5 documented with per-row code/test evidence** (`docs/quality/M4_PERSONA_WALKTHROUGHS.md`).
**Progress (session 10, #25 — hardening):** memory budget **met with headroom — 1265 MiB peak vs 2048 MiB @ 49.8 MP** (exact VmHWM; gate `crates/cli/tests/memory_budget.rs`, CI `memory-budget` on 3 OSes) after the budget work found and fixed **two real full-res-buffer lifetime bugs** in the export path (+600 MB each @ 50 MP); **kill-during-export battery** green on 3 OSes (all encoders publish atomically; the battery anchors kills to the write phase via a progress pipe after a naive version failed mutation testing — a direct-write encoder now fails it 4/4); **fuzz battery** live (4 targets, CI bursts, 0 crashes — decode ran 156k execs locally); **secret scan + provenance gate** executable in CI (16 patterns × 335 files; 14 binary assets sha-verified — found 2 unprovenanced icons); **error-state inventory** 38 codes × EN/VI × matrix, test-enforced (found 2 codes that could reach the UI untranslated); **cache budgets enforced** (preview slot + LUT LRU, live-configurable) with **hit-rate surfaced in the S2 status bar**; **cross-platform build scripts** (`scripts/build/` — Ubuntu/Debian/Fedora/Arch, Windows, macOS incl. universal) + `docs/BUILDING.md`, unsigned-by-design. Evidence: `docs/quality/M4_HARDENING_REPORT.md`. M4 scope complete pending owner review.
**Progress (session 11 — post-merge functional fixes after the owner's first real GUI run):** the owner reported the app "opens, UI nice, image imports — but not functional". Root-caused to TWO packaging/runtime-layer bugs invisible to every CI gate (no gate ever ran the real webview bridge or a real bundle): **(1) IPC key-case mismatch** — Tauri v2 converts command ARGUMENT keys to camelCase but RESPONSES are serde-verbatim; `StateJson` serialized `image_open` while the UI read `imageOpen` (undefined) → `$state.imageOpen` always falsy → **the editor never left the Home screen after import** (no crash, no error — silent); same mismatch on `ImageInfo`/`LutRecordJson` responses and `LutStackEntry`/`ImportLutArgs` nested request payloads (the LUT panel was doubly broken). Fixed with `#[serde(rename_all = "camelCase")]` on the app-layer DTOs (on-disk formats untouched — LutRecord/sidecars/settings keep their snake_case). **(2) Stock registry never shipped** — `tauri.conf.json` had no `bundle.resources`, and `stocks_root()` probed cwd-relative paths that only work in a checkout → installed apps had ZERO film stocks (Film Lab/Filmstrip dead, film export broken). Fixed: `profiles/*` bundled as resources, resolution order env → checkout → **resource_dir()** (resolved in `.setup()` where the AppHandle exists) → library fallback, pure-and-unit-tested (`lib.rs::resolve_stocks_root`); Film Lab now shows an honest empty state NAMING the path it looked in; viewport render failures surface in the error banner instead of console-only. **New gates so this class of bug cannot ship again:** `tests/ipc_contract.rs` parses the TS interfaces the UI consumes and asserts field-by-field equality with the Rust wire JSON (both directions, incl. nested payloads); CI **`app-bundle` job** runs a real `tauri build --bundles deb` and asserts the .deb contains ≥5 stock profiles (uploads the unsigned .deb as an artifact); `cache_stats` refactored to a pure `cache_stats_json` for contract sampling.
**Progress (session 12 — post-merge fix for the owner's second GUI run, "Expected ArrayBuffer for the first argument. [undefined]"):** after PR #32 the app opened the editor and imported, but the viewport never rendered and no adjustment had any effect. Root cause (verified against tauri 2.12.5 sources): the CSP in `tauri.conf.json` was `default-src 'self'` with no `connect-src`, so the built app's webview **blocked the `ipc://localhost` fetch** — Tauri's fast IPC transport. Tauri silently fell back to the postMessage interface, where a command returning `ipc::Response` (raw bytes — `render_preview`/`original_frame`, the only two) is serialized by `format_result` as a **plain JSON array of numbers** instead of an ArrayBuffer; `new DataView(array)` then throws JavaScriptCore's "Expected ArrayBuffer for the first argument." on WebKitGTK → viewport dead while every JSON command (import, state, stocks, tone) kept working. Fixes: **(1)** CSP now `default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; …` (restores the fast path on Linux/macOS `ipc:` and Windows `http://ipc.localhost`); **(2)** `ipc.ts::parseFrame` now normalizes every transport shape into an ArrayBuffer (direct buffer · `{message, index}` channel wrapper · JSON number array), so the postMessage fallback stays functional on any platform/webview policy instead of dying at the first DataView; **(3)** `tErrorCode` no longer appends a bogus `[undefined]` suffix for raw JS errors without a taxonomy code.
**Progress (session 10b — post-merge fix, PR #31):** the first real `tauri build` on owner hardware (Fedora 44) exposed that session 10's build scripts had never exercised the Tauri hook layer — CI builds the UI manually and debug builds with `devUrl` never embed `frontendDist`, so the misconfiguration was invisible to every gate. Root cause: `beforeBuildCommand` runs with cwd = the CLI's *auto-detected frontend dir* (`apps/melography-app/ui`, the nearest `package.json`), so `npm --prefix ui` resolved to `ui/ui` (ENOENT, exit 254), and `frontendDist: "../ui/dist"` resolved against the `tauri.conf.json` dir to the nonexistent `apps/ui/dist` (would have failed the CLI web-assets check right after the hook). Fixed both in `tauri.conf.json` (hooks `npm run build`/`npm run dev`; dist `ui/dist`), verified against tauri-cli 2.12.4 source (`app_paths.rs`/`helpers.rs`/`tauri-codegen context.rs`), a directory-resolution simulation, and a real `npm run build` from `ui/`; `docs/BUILDING.md` §"Invocation directory matters" documents the constraint.
**Progress (session 09, #15 — i18n/a11y):** zero embedded strings (FilmLab/StockBrowser/Settings swept; `tEnum` localizes backend enums with raw fallback; 17 missing error-code entries added — 28/28 covered); locale-aware numbers (`fmtNum`, VI comma decimals); **pseudo-locale CI** (`vite build --mode pseudo`, proportional ≥40% accent-padding, tree-shake-verified) + `ui_i18n_audit.mjs` (parity/diacritics/placeholders/embedded-scan/error-coverage — 323/323 keys, 95% diacritics, 0 embedded strings); **`ui_contrast_audit.mjs` computes WCAG AA for all 54 token pairs × 3 themes — found and fixed 2 real AA failures** (light primary-button label 3.65→4.84; light fg-tertiary-on-elevated 4.28→4.96) + dark fg-tertiary headroom; SR announcements (debounced render status, toast aria-live); Esc closes every dialog; Svelte a11y linter clean repo-wide; 32px touch targets (one documented exemption); reduced-motion rule. Evidence: `docs/quality/M4_A11Y_I18N_REPORT.md`. Remaining in M4: #25 hardening.

### M5 — Controlled beta v0.1 (effort L, uncertainty medium — owner-gated)

**Restructured (session 13, 2026-10-25).** The owner's verdict on the first
functional GUI run — *"works; incomplete features; small bugs; preview slow
but optimizable; Film Room not good yet"* — plus four explicit asks
(bundled famous LUTs, especially Rec709; lightcraft as reference;
Nvidia/Radeon/Intel-Arc support; plan carefully including edge cases) moved
the decision point: **a beta is a user-facing gate** — shipping it with a
Film Lab that feels dead and multi-second previews would spend the one
first impression a beta gets. M5 therefore splits into two owner-gated
phases; the original beta scope is unchanged and becomes **M5-B**.

#### M5-A — Creative depth & preview speed (code-verified diagnosis first)

**Progress (session 14, A1 — preview transport):** shipped end-to-end.
Command-boundary `resolve_max_dim` (`0` → `Settings.preview_max_dim`;
`256..=2560`; else typed `render.max_dim_out_of_range` — EN/VI catalogs +
inventory row added; `set_settings` now clamps too, closing the in-session
drift window). `max_dim` folded into `preview_key`; the slot now caches the
**bounded** preview image (no-film leg: full-res LUT output downscaled once
before caching — never a full-res slot, hit path is a blit; budget-exceeded
frames stay un-retained). Film Lab rerouted through the App-level
`viewport.refresh()` — one render path, stock select re-renders, no panel
invokes `render_preview` directly; viewport last-wins coalescing (mid-render
requests collapse into one queued re-run reading the latest params/bound,
including a failed-pass drain) + original-frame reuse keyed on
(image, bound). Regression tests: boundary validation (sentinel, bounds,
drifted fallback), key separation both legs, no cross-size stale hits, slot
holds the bounded artifact, film compose bounded + wire-frame geometry.
Known trade-off: a viewport-size change with no film leg re-renders once
(was a cache hit + per-hit downscale) — coalescing bounds the storm, and
≥35 MP sources gain caching they never had (bounded artifact fits budgets
the full-res render blew). Evidence: PR #35.

**A1 · Preview transport correctness (P0 — the "Film Lab not good yet" bug).**
Diagnosis verified against `main` @ 5f2d34f (three real defects, not guesses):
1. `FilmLab.svelte` invokes `render_preview` with `maxDim: 0`; in
   `render.rs::downscale_ap1` a `max_dim` of 0 is the **identity** → the film
   chain renders at **full source resolution** on every slider commit (the
   M2 §4 measurement was 3.4–4.4 s at 3.1 MP *preview* scale — full-res is
   ~8× that on a 24 MP source) and ships a wire frame up to ~100 MB that the
   caller then **discards** (`.catch(() => {})`).
2. The preview cache key (`compose_preview`) omits `max_dim` → a full-res
   render **poisons the slot** for every later bounded request: a Viewport
   `refresh()` at 1600 px receives a full-res image from cache (frame-size
   explosion) — or the slot is dropped by the budget and every render stays
   a miss.
3. `selectStock()` never requests a render → picking a stock shows **no
   visual change** until a slider moves; the `busy` gate silently swallows
   rapid slider commits (no last-wins coalescing).
Fixes + acceptance: command-boundary validation `256..=2560` (`0` = fall
back to `Settings.preview_max_dim`; outside → typed error
`render.max_dim_out_of_range`); `max_dim` folded into the cache key (test:
same params, different `max_dim` → different keys, no cross-size poisoning);
Film Lab routes through the App-level `viewport.refresh()` — **one render
path**, stock select included; last-wins coalescing. Edge cases: viewport
resize mid-render; 24 MP sources (wire frame hard-capped ≈ 26 MB);
`original_frame` bounded identically (compare parity); budget-exceeded
frames stay un-retained (the existing honest degradation).

**Progress (session 16, A2.1 — strip threading):** shipped.
`chain::render_threaded(img, params, workers)` splits the image into
contiguous chunks, one `std::thread::scope` worker per chunk — no new
dependencies, and chunks are just strips of their own height, so the §L
contract (byte-identical for ANY partition) extends to any worker count
(new gates: threaded≡mono across 2/3/4/8/16 + auto workers, tiny-image
worker clamping, margin-heavy configs, and the scratch-bound invariant
`h + n·2·m_total ≤ 2(h + 2·m_total)` unit-tested on `worker_cap`). The
strip body was extracted UNCHANGED into `render_strip` shared by all
three executors — one code path is what makes them byte-identical;
`render`/`render_tiled` keep their exact sequential semantics (the
existing tiled≡mono gate still passes untouched). The preview leg
(`render_pipeline`) now calls `render_threaded(_, 0)` = one chunk per
logical core; export keeps the sequential CLI path (memory-first,
R-REN-2 — the CLI/memory-budget suites are untouched). Panic
propagation: a failing worker resumes its panic in the caller
(no swallowing, no double-panic). Evidence: PR #37; perf harness gains
a `preview-threaded-2048x1536` leg (ignored, release-run — the honest
re-measure for the §4 re-pin happens on the owner's machine).

**A2 · Film-chain preview speed (the measured 3.4–4.4 s vs ≤ 50 ms / 400 ms
budget — executing the M2 §5 plan in spec order).**
1. **Strip threading** — embarrassingly parallel; the tiled≡mono gate already
   proves order-independence, so there is no determinism risk; the cheapest
   big win on multi-core owner hardware (the 2-core sandbox halves it).
2. **Grain fast path** (FILM_MODEL §G correlated-noise approximation) — the
   §L statistical gates apply **unchanged** to the fast path (RMS ±10 %,
   PSD ±15 %, ρ ±0.05): a fast approximation that misses the statistics is a
   bug, not a feature. UI-labelled "grain (fast preview)"; export always
   uses the full model.
3. **GPU spatial kernels** (grain gather + halation as WGSL compute, the
   `film_pixel` parity-harness pattern). lightcraft's public ~4 ms/24 MP
   slider re-render on wgpu compute (M4 Pro) is the existence proof that the
   budget is reachable; parity gates run on CI adapters, CPU stays the
   reference path.
Re-measure, re-pin §4 honestly. Edge cases: the zoom-consistency gate must
still pass at preview scale on the fast path; the memory-budget gate re-runs
(threading changes buffer lifetimes); no adapter → CPU identical (existing
fallback contract).

**A3 · Bundled LUT library (the "famous LUTs, especially Rec709" ask).**
- **Tier 1 — generated in-repo (GPL, no external rights needed):**
  (a) **camera-log→Rec709 technical LUTs** baked from *published* transfer
  functions (first slice: ARRI LogC3 + LogC4, Sony S-Log3, Panasonic V-Log,
  Canon CLog3, Fuji F-Log) — new camera-log `LutEncoding` variants +
  golden-vector tests in `melography-color` against the vendors' published
  reference values; a `tools/` baker; vendor LUTs are downloaded at **dev
  time only** for validation and **never bundled** (their redistribution
  terms are unverified) — validation deltas land in the tool output, not the
  repo. (b) **PFE-style print LUTs generated from our own print stage** —
  honest non-trademark names, provenance `generated`.
- **Tier 2 — externally sourced, license-gated by the existing
  manifest/provenance machinery:** G'MIC film styles (CeCILL), Pat David's
  original set (license to verify before bundling — expected CC BY-SA),
  the YahiaAngelo `.cube` conversions (MIT packaging). Every entry carries
  license + source URL + SHA-256 in `LICENSES-LUTS.md`; the existing
  `license_unclear` gate refuses anything unverifiable.
- Bundling via `bundle.resources` (the PR#32 stocks pattern) + an
  `app-bundle` CI assertion (≥ N bundled LUTs in the .deb); UI: a read-only
  "Built-in" section in the LUT panel beside user imports.
Edge cases: encoding contracts flow through the insertion-point machinery
(declared input = camera log; output `rec709-gamma2.4` — already in the
vocabulary); `rec709-gamma2.4` vs sRGB-piecewise must never be conflated;
bundled LUTs load through the existing hash-verified path (tamper-evident).

**A4 · GPU adapter support & diagnostics (the "Nvidia/Radeon/Intel Arc" ask).**
Adapter selection in Settings → "Graphics": Auto (policy: discrete >
integrated > software, documented vendor tie-break) + explicit per-adapter
pick with honest "why unavailable" rows (UX_SPEC §9 pattern); backend
preference Vulkan → DX12 → Metal → GL, software (llvmpipe) last resort and
labelled "slow (software)". Known-issue matrix from research: mixed-GPU
systems pick the wrong adapter without a selector (wgpu/Steam reports);
DX12-only VMs; Intel Arc on Linux needs recent Mesa; device-loss recovery
(eGPU unplug → re-init; GPU auto-disabled after N failures, persisted);
zero adapters → CPU path + honest message. A **local-only diagnostics view**
(adapter, driver, backend, parity status, probe timings, render telemetry)
with opt-in export for bug reports — **no telemetry** (R-PRV-1 stands).

**A5 · lightcraft reference** (research → `comparables.md` §6; Apache-2.0/MIT
dual license — GPL-compatible, reuse permitted with attribution). Borrowings:
background render worker pool (loupe/thumbnails off the UI thread); the
~4 ms/24 MP wgpu proof (calibrates the A2.3 target); preset-Amount pattern
(extend LUT intensity semantics to stocks). Confirmed differentiation:
lightcraft ships **no LUT support and no film emulation** — our bundled
library + physical film model stays the edge.

**M5-A exit:** Film Lab interactions are bounded and instant on the owner's
machine (no full-res renders, no discarded frames); preview p50 within the
re-pinned §4 budget on declared reference hardware (shortfalls documented,
never hidden); ≥ 6 camera-log→Rec709 LUTs + print LUTs bundled and
license-clean; adapter picker + diagnostics shipped; every A1 defect class
has a regression test.

#### M5-B — Beta engineering v0.1 (the original M5 scope, unchanged)
Tasks: packaging matrix (ADR-0005) · updater end-to-end on dummy→beta channel with owner-held keys (owner generates; CI uses Secrets) · release workflow + protected Environment `release` · recovery docs · beta opt-in flow (Settings) · CHANGELOG + install/recovery guides.
**Exit:** reproducible signed builds from tagged commit; updater roundtrip verified on all 3 OSes by owner (or documented device gaps); **release published only after explicit owner approval** (OPEN_QUESTIONS #2/#5).

## 3. Requirements traceability matrix (seed — rows grow as issues are created)

| Req ID | Requirement (summary) | Module | Milestone | Verification | ADR/Spec |
|---|---|---|---|---|---|
| R-OS-1 | Win10/11 x64, macOS 12+ (arm64+x64), Linux x64 AppImage | app/packaging | M0/M5 | CI matrix + owner smoke | ADR-0001/0005 |
| R-IN-1 | JPEG/PNG/TIFF/WebP decode, oriented, ICC-honoring | io/color | M0 | codec matrix + golden | ARCH §8, CP §2 |
| R-IN-2 | HEIC/AVIF only via proven distributable backend | io | M0 | spike S2 ext + build evidence | CP §2 |
| R-IN-3 | No RAW | — | — | (scope guard) | master §02 |
| R-OUT-1 | JPEG/PNG/TIFF export 8/16-bit, profile+intent, sRGB default | io/color | M1 | golden + UI matrix | CP §6 |
| R-ND-1 | Non-destructive: immutable original, versioned sidecar, fingerprint guard | presets | M1 | sidecar tests (migrate/unknown-field) | ARCH §7 |
| R-REN-1 | GPU where supported + CPU fallback, shared spec, per-stage tolerances | gpu/core | M0–M2 | parity harness in CI | ARCH §5, CP §8 |
| R-REN-2 | Tiled export, halo margins, ≤ 2 GiB @ 50MP | core | M2 | tiled≡mono suite + memory bench | ARCH §5 |
| R-REN-3 | Deterministic grain/halation in film coords, zoom-stable | film | M2 | determinism + statistics gates | FM §G/L |
| R-REL-1 | Graceful no-GPU/low-disk/cancel/corrupt-file/interrupted-update | all | M4 | failure-mode test battery | CP §7, REL §5 |
| R-PRV-1 | No upload/telemetry; opt-in update checks & model downloads only | app | M3/M5 | network-allowlist tests + code review | ADR-0004/0005 |
| R-LUT-1 | `.cube` v1: tetrahedral, encodings, intensity, provenance | lut | M1 | luts.md §5 test matrix | LUT research |
| R-FILM-1 | Negative/slide curves, crosstalk, print/scan second stage | film | M2 | FM §L gates | FM §D/F |
| R-FILM-2 | Stock profiles are versioned data + provenance labels | presets | M2 | schema tests + UI badges | FM §J |
| R-AI-1 | Local-only, CPU-guaranteed, parameter-output AI; manifest gate | ai | M3 | ADR-0004 §4 checklist | AI doc |
| R-UX-1 | 7-screen UX, EN+VI, a11y, high-DPI, keyboard map | app | M4 | walkthroughs + pseudo-locale CI | UX_SPEC |
| R-UPD-1 | Signed updater, channels, consent, supersede-rollback | app/rel | M5-B | S4 + M5-B roundtrip evidence | ADR-0005 |
| R-LUT-2 | Bundled LUT sets incl. camera-log→Rec709; license-clean, tamper-checked | lut/presets | M5-A | manifest gate + app-bundle CI assertion | PLAN M5-A A3 |
| R-GPU-1 | Adapter selection (auto+manual), vendor matrix, device-loss recovery, local-only diagnostics | gpu/app | M5-A | unit tests + owner-device probes | PLAN M5-A A4 |

## 4. Performance budgets (targets; re-pinned by M0 measurements on declared hardware)

Preview (12MP source, fit-to-window on reference integrated GPU): cached/proxy interactions p50 ≤ 50 ms, p95 ≤ 120 ms, cold full re-render ≤ 400 ms. Export: 24MP ≤ 5 s GPU / ≤ 20 s CPU. Memory ≤ 2 GiB @ 50MP incl. caches. AI: per ADR-0004 §5. **These are stretch targets subject to measurement** — every CI perf test prints the hardware used.

### M0 measurements (2026-10-24) — first re-pin

Declared hardware so far: **GitHub-hosted CI runners** (ubuntu-latest /
windows-latest / macos-latest, the only 3-OS fleet available) and the
**dev sandbox** (linux x86_64, 2 cores). Owner-owned reference machines
(integrated-GPU laptops) are still to be declared — the GPU-dependent
budgets stay provisional until those devices run the S1 harness.

| Budget line | M0 evidence | Re-pin |
|---|---|---|
| ORT cold init | 14.1 ms (S3, 2-core sandbox) | target stays ≤ 300 ms — 21× margin observed |
| ORT warm invocation | 0.011 ms median (S3) | ~200 ms analysis budget: cleared |
| Frame encode 1080p (CPU, preview transport) | raw 2.0 ms / PNG 12.3 ms / WebP-LL 6.4 ms (S1) | p50 50 ms interaction: CPU legs fit with > 25 ms headroom worst-case |
| HEIC/AVIF decode (tiny fixtures) | 3.3 ms / 1.0 ms (HEIF probe) | per-image decode is not a budget risk; codec matrix owns it |
| Updater check+download+verify | 27 + 28 ms @128 KiB (S4) | UX-invisible; not a user-facing budget line |
| GPU readback throughput | **unmeasured** (no adapter in sandbox; CI macos Metal + owner devices pending) | preview p50/p95 and export 24MP ≤ 5 s stay **provisional** |
| Webview IPC + blit | **unmeasured** (M1 app shell) | same — provisional; egui fallback armed (ADR-0001) |

See `docs/research/spikes/{S1,S2,S3,S4}_REPORT.md` + `HEIF_PROBE.md`
for full context; every number above is reproducible from the repo.

### M2 measurements (2026-10-24, session 04) — film chain

Dev sandbox (linux x86_64, **2 cores**, release build; CI re-runs the
gates on all 3 OSes; owner reference machines still pending).

| Line | M2 evidence (worst/best stock) | Status |
|---|---|---|
| Film resolve (per param change, incl. grain self-calibration) | 1.3–2.9 ms | fine — cached per param set |
| Preview re-render p50 (2048×1536, full film chain) | **3.4–4.4 s** vs ≤ 50 ms interactions / ≤ 400 ms cold | **shortfall — documented cause + fast-path plan** (quality report §5: grain gather at pixel-area supersampling ≈ 225 cell-visits/px; the spec's correlated-noise fast path + threading + GPU spatial kernels are the plan) |
| Export 24 MP (full film chain, CPU, tiled strips) | **8.3–18.4 s** vs ≤ 20 s CPU | **within budget** (2-core worst case) |
| §L statistical gates (grain RMS/PSD/ρ, halation edge-spread, zoom, determinism, tiled≡mono) | all green; 2 real seam bugs caught + fixed by the tiled gate | **M2 exit criterion met** |
| GPU film pixel-kernel parity (5 stocks × 3 paths × 2 polarities × 2 layouts) | gate ≤ 1e-3 on CI adapters (macos/windows); spatial-stage kernels = M3/M4 follow-up | pixel scope green; spatial pending (documented) |

Full evidence: `docs/quality/M2_FILM_QUALITY_REPORT.md` (reproducible via
`cargo test --release -p melography-film --test gates -- --ignored`).

### M3 measurements (2026-10-24, session 05) — AI assistance

Dev sandbox (linux x86_64, 2 cores, release build); CI job `ai-onnx`
re-runs the benchmark + budget on every push.

| Line | M3 evidence | Status |
|---|---|---|
| Baseline analysis (12 MP → 512 px proxy) | 60.6 ms vs ≤ 3 000 ms budget | **green — 49× margin** |
| ONNX predictor inference (12 MP → 64 px, ort CPU) | 49.3 ms vs ≤ 3 000 ms | **green — 61× margin** |
| Benchmark aggregate (ev + 5·wb + clip, lower better) | baseline **0.205** vs naive-ONNX predictor 1.019 | **the bar** — CI-asserted; any model must beat 0.205 |
| Vetted models shipped | 0 (all candidates gate-blocked on unverified weights licenses) | baseline-only v1 — the honest ADR-0004 §7 outcome |
| Tone apply (full film-chain export path) | 19.2 ms @ 0.6 MP (CLI report) | not a budget line; GPU mirror = documented follow-up |

Full evidence: `docs/quality/M3_AI_REPORT.md`.

### M4 measurement (2026-10-25, session 10) — memory budget gate

Dev sandbox (linux x86_64, 2 cores, release; CI job `memory-budget`
re-runs the gate on all 3 OSes; macOS reports a sampled lower bound,
never claimed as a peak).

| Line | M4 evidence | Status |
|---|---|---|
| Peak RSS, 49.8 MP end-to-end (decode → tone → full film chain → PNG16 + sidecar) | **1260–1267 MiB** vs ≤ 2048 MiB on CI linux+windows (exact); 1265 MiB dev sandbox; **1997–2006 MiB on the macOS runner** (sampled lower bound — green but tight; allocator retains freed pages; the platform to watch) | **green on all 3 OSes** |
| Root causes fixed en route | pre-tone f32 buffer survived its own replacement (+600 MB); `decoded`/`ap1`/`working`/output-file-slab held to end-of-report (+600–900 MB) | two real bugs, regression-gated |
| Kill-during-export (3 OSes) | 9 anchored SIGKILL rounds × PNG/TIFF/JPEG: destination absent-or-complete, litter cleaned, retry succeeds; mutation-validated (direct-write encoder fails 4/4) | PLAN M4 exit criterion **met** |

Full evidence + method notes: `docs/quality/M4_HARDENING_REPORT.md`.

### M1 additions (2026-10-24, session 03)

| Line | M1 evidence | Status |
|---|---|---|
| CPU tetrahedral correctness | 656 oracle samples, worst \|Δ\| = 1.19e-7 vs the independent Python implementation | correctness gate (not a perf line) — green |
| GPU tetrahedral parity | CI macos/windows adapters run the ≤1e-6 gate on every push (branch first run pending) | gate exists; numbers land in CI logs |
| ICC export determinism | lcms2 profile dateTime pinned (`icc_bytes_deterministic`) after the E2E test caught byte-drift between identical exports | bug fixed, regression-tested |
| Preview composition (app `render_preview`) | implemented with per-param full-res cache + fit-downscale; render-ms telemetry in every wire frame | **unmeasured at scale** — needs the live webview legs (issue #4) / owner devices; budget lines stay provisional |

The webview-IPC + double-transform legs (S1/S2 residuals, issue #4) need a
running GUI session — the M1 shell ships the transport and instrumentation
for exactly that measurement.

## 5. Risk register (top 10)

| # | Risk | Likelihood | Impact | Mitigation / owner of trigger |
|---|---|---|---|---|
| 1 | Webview preview transport fails color/latency gates (S1) | Medium | High (stack change) | egui fallback fully designed (ADR-0001); decision by end of M0 |
| 2 | Film model quality below "convincing" without calibration data | Medium | High (core value) | artist-authored profiles + honest labels first; calibration upgrade path; A/B protocol |
| 3 | `ort` RC instability / ONNX packaging pain on 3 OSes | Medium | Medium | pin version; CPU-first; S3 spike; AI is optional by design |
| 4 | HEIC/AVIF licensing/build complexity blocks formats | Medium | Medium | optional feature gate; explicit go/no-go at M0; docs state support precisely |
| 5 | Solo-maintained scope creep (comparables lesson) | High | Medium | milestone gates; V1 non-goals enforced in review checklist |
| 6 | macOS/Windows runtime gaps without test devices | High | Medium | CI smoke + owner devices when available; honest gap docs (OPEN_Q #8) |
| 7 | Model weights licensing dead-ends M3 candidates | Medium | Low-Med | non-ML baseline ships regardless; vetting gate decides early |
| 8 | Updater signing mistakes break trust | Low | High | owner-held keys; S4 dry-runs; supersede-rollback runbook |
| 9 | Performance budgets missed on integrated GPUs | Medium | Medium | budget tests from M0; **M5-A A2 executes the fast path** (threading + grain fast mode + GPU spatial kernels) |
| 10 | License/attribution mistakes when reusing GPL code | Low | High | provenance.json + THIRD_PARTY_NOTICES CI check + skill `license-and-asset-vetting` |
| 11 | Bundled-LUT licensing/provenance mistakes | Medium | High (legal) | Tier-1 self-generated first (published formulas + own print stage); Tier-2 only after license verification (CeCILL / CC BY-SA / MIT candidates) via the existing manifest gate; `LICENSES-LUTS.md` + provenance CI |

## 6. Definition of Done (every milestone, non-negotiable)

CI green on 3 OSes (with honest GPU-test absence notes) · tests-first for deterministic core math · provenance for any reused code/asset · STATE.md + session log updated · docs updated with real measurements · draft PR checklist completed · **owner approval before any merge to `main` or release action**.
