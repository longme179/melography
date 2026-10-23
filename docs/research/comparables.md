# Research Notes — Comparable Applications

**Distilled 2026-10-24. Policy: architecture comparison only; no code copied (RapidRAW is AGPL-3.0 — excluded from reuse by ADR-0003).**

## 1. RapidRAW (`cybertimon/rapidraw`, AGPL-3.0, ~10.6k stars, active 2026)

What we studied from public repository structure and documentation (not code):

- **Rust-native performance positioning:** high-throughput editing with lazy loading and cached pipelines; confirms our belief that a Rust core can serve interactive previews comfortably on integrated GPUs.
- **Lesson adopted:** aggressive *stage-level caching* keyed by (stage, params-hash, input-hash) — invalidation at stage granularity, not whole-pipeline. Our `RenderContext`/cache trait contracts (ARCHITECTURE.md §5) encode this.
- **Lesson adopted:** LUT pipeline as first-class path with explicit color-space declarations (they expose this in UI) — reinforces our `.cube` encoding-contract decision.
- **Caution observed:** breadth-first feature growth (RAW support, many modules) stretches a solo-maintained project; we consciously sequence film-emulation depth first (master prompt MVP) and defer RAW permanently (non-goal).

## 2. darktable / RawTherapee (as *products*)

- Both are RAW-first; their JPEG-editing paths are secondary and their UX (module stacks, history stacks) targets enthusiasts. MELOGRAPHY's persona 1 ("one-click film looks") needs a fundamentally simpler default surface — persona 3 depth lives in a "Film Lab" advanced panel, not the main toolbar.
- darktable's XMP sidecar discipline (versioned, unknown-field-preserving) is the model for our recipe format (`docs/ARCHITECTURE.md` §7).
- RawTherapee's HaldCLUT browser (thumbnail swatches of looks applied to a fixed sample) is the direct UX inspiration for our Film Stock Browser — but applied to *the user's own photo thumbnail*, not a stock image (cheap because our preview pipeline is already incremental).

## 3. Filmulator

- Single-purpose (film simulation from RAW) with a *physics-first* parameter surface (developer dilution, time…). Validates our bet that "film" as a *model* (curves + development + grain) resonates with enthusiasts more than slider-soup presets; its dormancy (GPL-ambiguous, slow maintenance) shows the cost of physics-first *without* an interactive-first renderer — our preview-performance budget is a hard requirement, not a nice-to-have.

## 4. spektrafilm (GPL-3.0, 1.1k stars, active)

- The closest *spiritual* comparable: spectral film emulation as the core product, LUTs as distribution vehicle. Their architecture (spectral simulation → LUT bake) suggests a future option for MELOGRAPHY: bake stock profiles to fast preview LUTs *at runtime* (our own pipeline, our own profiles) while keeping the full model for export — worth a performance spike in M2 if preview budget slips.

## 5. Commercial apps (VSCO, Dehancer, RNI Films — studied from public marketing only, no code access)

- Dehancer (film emulation plugin ecosystem) proves paying demand for *physical-model* film emulation with grain/halation/bloom controls; their feature list (film profiles, grain, halation, bloom, gates, licenses per-host) maps almost 1:1 onto our stage list — a validation of scope, and a reminder to differentiate on: offline privacy, price (free/open), EN+VI, and open profile format.

## 6. Differentiation summary (what MELOGRAPHY must not lose)

1. Offline-first, local-only, no accounts (vs. cloud-tethered competitors).
2. Editable, *explainable* film model (physical stages user can reorder within guardrails) vs. black-box presets.
3. Honest provenance labels (`measured` / `reference-fitted` / `artist-authored` / `experimental`) on every stock profile.
4. User LUTs as composable first-class citizens, not an afterthought import.
