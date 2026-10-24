# Research Notes — LUT / Filter Engine

**Distilled 2026-10-24. Sources: OpenColorIO (BSD-3) & OpenImageIO (Apache-2.0) `.cube` handling, RawTherapee HaldCLUT (GPL-3.0) loader behavior, darktable LUT module.**

## 1. Format inventory and parsing pitfalls (v1 = `.cube` first; `.3dl` + Hald later)

### `.cube` (Adobe)
- `LUT_1D_SIZE N` (2–65536) or `LUT_3D_SIZE N` (2–256); exactly one of the two.
- `DOMAIN_MIN`/`DOMAIN_MAX` (2 or 6 floats; default [0,1] per component).
- `TITLE`, `LUT_1D_INPUT_DURATION` (rare, reject-with-error), `# comments`, CRLF/LF tolerance, arbitrary whitespace between tokens.
- **Pitfalls:** tables split across lines (row-major, N³ or N entries, any number per line — must stream-parse tokens, not per-line); >N rows = error, <N = error; DOMAIN ranges where min ≥ max = error; NaN/inf tokens = error; mixed 1D+3D in one file = error.
- **Semantics decision:** values are sampled in the LUT's *declared domain*, which is a **color-encoding contract, not just numbers** — see §3.

### `.3dl`
- Multiple dialects exist (Autodesk/Lustre, Red, Iridas-adjacent): grid sizes 17/33/64 common; input/output bit-depth scaling conventions vary (Lustre 10/12-bit shaper tables; some dialects assume 10-bit input). **Decision:** v1 ships `.cube` only; `.3dl` arrives in M1+ **only** with an explicit dialect selector in the import dialog and a per-file "detected dialect" recorded in the library; ambiguity → refuse with an explanatory error, never guess.

### Hald CLUT (PNG)
- Valid sizes: NxN PNG where N = 8·2³…; image is a reshaped 3D lattice of a fixed identity-derived order. **Validation at import:** decode size ∈ {512², 1024²(some), 4096²…} with exact expected dimension/order check; 8-bit vs 16-bit; sRGB-identity sanity check (lattice cell centers within tolerance of theoretical identity Hald values — catches wrong color space / wrong order). Wrong sizes → error with explanation.

## 2. Interpolation contract

- **Tetrahedral** 3D interpolation (the only acceptable default; trilinear is visibly wrong on saturated gradients). CPU implementation in `melography-lut` + WGSL implementation in `melography-gpu`, both validated against an **independent reference implementation** (pure-Python/NumPy-style oracle in `tools/`, not a copy of the CPU code).
- 1D shaper: monotonic piecewise-linear / PCHIP when a 1D+3D chain is declared; interpolation in declared domain.
- **Identity invariant:** LUT built from identity samples must reproduce input within 1e-6 (f32) — property-tested.
- Boundary policy: clamp inputs to DOMAIN range *before* interpolation; out-of-range inputs are clamped, and the clamp is **logged once per render, surfaced in UI** ("LUT domain clipping active").
- Precision: f32 internals; document that 8-bit LUT files (Hald) quantize to 1/255 steps — upsampling cannot restore precision; UI shows effective resolution.

## 3. Color-encoding contract (the big one most apps get wrong)

A LUT is authored with an assumed input encoding (e.g., Rec.709 display-linear, sRGB-encoded, camera-log) and output encoding. Applying a Rec.709-log LUT to linear-AP1 data produces garbage that often *looks plausible* on mid-tones — the failure is silent.

- Import dialog requires declaring input/output encodings from a fixed list (`srgb-display`, `rec709-display-linear`, `rec709-gamma2.4`, `display-p3-display`, `acescct`, `acescc`, `agx-log`, `custom-1d-shaper`…), plus optional declared domain note.
- The engine inserts the declared transforms around the LUT at its **insertion point** (`pre-film`, `film-input`, `print-output`, `finishing`) so the LUT evaluates in its authored space, then returns to working space. If the declared space cannot be verified, UI shows an amber warning badge "encoding assumed — verify".
- **We never guess proprietary camera-log metadata.**

## 4. Provenance & storage

Per-LUT record (JSON in user library + checksum in sidecars referencing it): original filename, SHA-256, source URL (optional), license/provenance note, declared encodings, size, inserted checksum verification at load; **content-addressed storage** (hash-named file in app-data `luts/`) with human-readable names as metadata; sidecars reference the hash — missing/corrupted file → explicit error state in UI + option to locate the file, never silent skip.

## 5. Test matrix (drives M1 DoD)

identity invariants · neutral ramps · domain transforms · 1D+3D chains · malformed/oversized/truncated files (fuzz) · tetrahedral boundary cases (grid corners, exact lattice points) · 8/16-bit round trips · CPU↔GPU parity (max |Δ| ≤ 1e-6 f32 after same rounding) · tiled-vs-monolithic equality · batch export determinism · cross-platform serialization (sidecar bytes stable).

## 6. What we take from which source

- OCIO: file-format corner cases + the discipline of declaring transforms around look LUTs (their `CDLTransform`/`FileTransform` composition semantics).
- RawTherapee: Hald CLUT identity-check heuristic + the lesson to *not* treat a LUT as the whole film chain.
- darktable: UI pattern for LUT intensity/mix + clipping warnings.
- No code copied in Phase 0; M1 implementations are ours under GPL-3.0 with attribution where a specific routine's logic was studied.

## 7. Implementation status (M1, 2026-10-24)

Implemented on `agent/m1-lut-workflow` (see crates/lut + tools/lut_oracle.py):

- §1 `.cube` parser: **done** (token-stream, strict, line-attributed; fuzz
  corpus 30 files + 3000 seeded mutations). `.3dl`/Hald remain future work
  exactly per §1's dialect-selector decision.
- §2 tetrahedral + 1D shaper: **done** (CPU oracle + WGSL mirror, parity
  ≤1e-6, identity invariant 1e-6 property-tested; boundary clamp reported).
- §3 encoding contract: **done** for the vocabulary subset
  {srgb-display, rec709-display-linear, rec709-gamma2.4, display-p3-display,
  acescct, acescc}; `agx-log` + `custom-1d-shaper` parse/serialize but
  refuse to apply (explicit error, UI shows "coming soon"). ACEScc/ACEScct
  applied on AP1 values (documented convention — encoding.rs header).
- §4 library: **done** (SHA-256 content-addressed blobs, provenance index,
  atomic index writes, checksum re-verified at load, Missing/Checksum/
  Corrupt explicit states; sidecars reference by hash — sidecar v1).
- §5 test matrix: all rows green locally; GPU parity + 3-OS legs run in CI
  on the branch (first run pending at time of writing).
