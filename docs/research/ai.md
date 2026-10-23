# Research Notes — Local AI

**Distilled 2026-10-24. Sources: pinned entries in `REFERENCES.md` §E. Full policy: `docs/adr/0004-local-ai.md`, `docs/AI_MODELS.md`.**

## 1. License findings that shape M3 candidate ordering (verified 2026-10-24)

| Candidate | Code license | Weights | Consequence |
|---|---|---|---|
| Image-Adaptive-3DLUT (AdaInt) | Apache-2.0 | **unverified** | Concept approved for study; shipping requires weights vetting (checkpoint files in research repos are often "for research only"). |
| Zero-DCE / Zero-DCE++ (Li et al.) | **NONE** (no LICENSE in repo) | NONE | **Cannot ship code or weights.** Paper equations may be re-implemented after checking the paper's own terms (papers ≠ repo code). This is the canonical trap in this space. |
| SCI (self-corrective illumination) | canonical repo not located this session | — | Deferred; find + vet at M3. |
| MobileSAM / EdgeSAM / U²-Net / Depth-Anything-V2 small | Apache-2.0 / NOASSERTION / Apache-2.0 / Apache-2.0 (code) | all **unverified** | Segmentation/depth candidates only after per-model manifest (ADR-0004 §4) passes. |

**Conclusion:** the M3 model list is *not* pre-committed. The vetting gate (ADR-0004) decides. Non-ML auto-lighting baseline ships first regardless — it needs no license at all and is fully deterministic.

## 2. Architecture pattern decision (from studying the candidates)

Pixel-generation models (Zero-DCE-style curve/illumination maps that *overwrite* pixels) conflict with our editability contract: suggestions must land as **editable parameters** in the sidecar, never as baked pixels. Therefore:

- **Preferred family:** *parameter predictors* — small CNN/transformer that ingests a downsampled image (≤ 512 px long edge) and outputs **scalar/vector edit parameters** (exposure EV, highlight/shadow recovery, WB cct/tint, optional 3×3×N tiny LUT basis coefficients in the AdaInt sense). User sees a before/after preview of *our own pipeline applying those parameters* — always revertible, always reproducible (parameters recorded in sidecar with model name/version/hash).
- **Mask family (optional, second):** tiny segmentation (subject/sky) producing a raster mask stored as a referenced asset in the recipe with feather controls; mask raster is regenerated deterministically at export resolution and aligned exactly (verified by mask-edge golden tests; halos are a hard fail).
- **Excluded for v1:** any model that outputs full-res pixels (relighting/denoise generation) — violates editability, blows CPU latency budget, and adds model-trust risk.

## 3. Runtime findings

- `ort` (ONNX Runtime bindings, Rust): 2.0.0-rc.15, MIT/Apache, actively maintained (2026-10-19). RC status → risk entry; pin exact version, CPU EP mandatory everywhere, CoreML/DirectML/CUDA EPs **opt-in per-platform after verification** (Spike S3 scope).
- Model size/latency budget (measured targets, hardware defined at M0): ≤ 50 MB compressed on disk, ≤ 3 s for reduced-resolution analysis on reference mid-tier CPU. Models violating the budget are rejected even if quality is good — the AI must never be the reason the app feels heavy.
- Downloads: opt-in, SHA-256 pinned, atomic install into user app-data, deletable from Settings; no model ever bundles with installers by default.

## 4. Baseline non-ML auto-lighting (ships in M3 before any model)

Deterministic, explainable, testable: percentile-based exposure targeting (map p50 luminance to target EV with highlight p99 protection), gray-world WB suggestion with skin-tone guard, black-point/contrast from histogram shape. This is the fallback path when no model is installed and the reference the AI must *beat measurably* (paired A/B on a fixed 30-image internal benchmark set, licensed-clean) to justify its download size.
