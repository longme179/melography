# MELOGRAPHY — Color Pipeline Specification

**Version:** Phase-0 spec · **Date:** 2026-10-24 · **Authority:** ADR-0002
Notation: RGB triples are column vectors; matrices are 3×3 row-major; all math in f32 (CPU oracle) unless noted.

## 1. Stage contracts (the spine of the pipeline)

| # | Stage | Input | Output | Notes |
|---|---|---|---|---|
| S0 | Decode + sanitize | file bytes | oriented, alpha-normalized, ICC-tagged pixels in source encoding | EXIF orientation; unpremultiply; bomb limits (§7) |
| S1 | Input color transform | source encoding + ICC | **AP1 linear f32, unbounded** | lcms2 for arbitrary ICC; validated fast paths for sRGB/P3 |
| S2 | Pseudo-scene expansion *(optional)* | AP1 linear | AP1 "exposure proxy" | log-shaped, reversible; §3 |
| S3 | Film chain | exposure proxy / AP1 | film output in AP1 | FILM_MODEL.md; LUT insertions per luts.md §3 |
| S4 | Display transform | AP1 | display encoding | AP1 → display ICC (lcms2); exactly once (§5) |
| S5 | Export transform | AP1 | output encoding | profile + intent + black-point-comp configurable; sRGB default |

**Hard rules:** no stage blends transfer-encoded pixels where linear compositing is required; every LUT wraps its declared encoding at its insertion point; negative/out-of-range values are preserved until a *documented* clamp boundary (LUT domain, S4/S5 inputs).

## 2. Input handling details

- **Encoding identification priority:** embedded ICC > declared container colorimetry (e.g., PNG cICP) > format default (JPEG/HEIC → sRGB assumption with UI warning when display is wide-gamut) > user override.
- **Orientation:** EXIF orientation applied at S0 (baked into decode); never re-applied later (guard flag in DecodedImage).
- **Alpha:** straight alpha internally; premultiplied only inside resampling/convolution stages (documented convert points). All-opaque fast path skips alpha entirely.
- **Animated inputs:** first frame + UI notice ("animated: first frame used") — no silent frame choice.
- **Bit depths:** 8/16-bit integer and f32 inputs accepted where the format allows; unusual depths documented per codec (M0 codec matrix).
- **Broken files:** structured `IoError`s (ARCHITECTURE.md §8); no best-effort garbage.
- **Decompression bombs:** pixel-count cap (default 300 MP, configurable) + zip-bomb ratio detection per codec; error includes the observed limit.

## 3. Pseudo-scene expansion (optional, reversible)

Display-referred input is already tone-mapped and clipped. The expansion converts it to a usable *exposure proxy*:

```
E(x) = ( max( L(x) − L_toe, 0 ) )^γ · E_scale        (log-domain pivot form)
L(x) = log2( max( Y_AP1(x), ε ) / Y_pivot )          (Y_pivot: middle-gray 0.18 by default)
```

- Parameters: pivot (default 0.18), toe, γ (0.5–1.5), E_scale — all serialized in the recipe; identity bypass available.
- **Contract:** reversible by construction (inverse mapping defined); UI label "exposure proxy — clipped data cannot be recovered"; histogram overlay marks clipped input channels.
- This is *not* RAW recovery; no reconstruction of clipped channels is attempted or implied.

## 4. Working space (AP1/ACEScg) definitions

- Primaries/white: AP1 as published by the Academy (OpenColorIO reference — verified source, `REFERENCES.md` §B).
- Chromatic adaptation: Bradford; input profiles' media white → AP1 working white (ACES ~D60) with adaptation state recorded in the recipe (reproducibility).
- Fast paths: fixed matrices for sRGB↔AP1, Display-P3↔AP1 (derived once, golden-tested against lcms2 per CI run; tolerance §8).
- Negative/out-of-gamut: preserved through S2–S3; the film model naturally maps them into dye gamut (physical behavior); S4/S5 gamut-map (perceptual intent default) with documented clip warnings.

## 5. Display path (S4) — exactly one transform

- AP1 → display profile via lcms2, retrieved from OS color management per monitor (Windows: WCS/ICMv2 profile; macOS: ColorSync; Linux: XDG portal / colord where available, sRGB fallback with UI notice).
- **Double-transform detection (S2 spike):** render a known AP1 test pattern to the preview surface; capture the surface back (test harness); the composed transform must match `lcms2(AP1→display)` within tolerance — if the webview applies (or drops) an extra transform, the spike fails and ADR-0001 fallback engages.
- HDR/P3: v1 targets correctly-managed SDR; P3 displays get correct P3 profiles (wide gamut preserved); explicit HDR (PQ/EDR) output is deferred with per-OS notes in RELEASING-era docs.
- Soft proofing (simulate output profile on display) = replace S4 target with output profile + display adaptation — same machinery, explicit UI state.

## 6. Export (S5)

- Formats: JPEG (8-bit, quality + chroma subsampling controls), PNG (8/16-bit, alpha), TIFF (8/16-bit, alpha, compression options). Bit depth/alpha support matrix is *per-format factual* — UI disables impossible combinations (never silently converts).
- Profile: embedded ICC by default (sRGB default; user-selectable working/output profiles); rendering intent configurable (default perceptual for photography, relative colorimetric available), black point compensation toggle.
- Dithering: optional ordered/blue-noise dither when quantizing to 8-bit (default on for 8-bit outputs) — eliminates banding introduced by quantization; documented as *display* convenience, not information gain.
- Metadata: EXIF (orientation rewritten correctly for any edits, date/time preserved), XMP (selective copy; strip GPS option, default ask), colorimetric tags consistent with embedded profile. **No byte-for-byte preservation promises** for unsupported/private tags.

## 7. Reliability & limits

Corrupted ICC → fall back to format-default encoding + warning (never abort the whole edit). Missing display profile → sRGB + notice. Unsupported CMM features (v4 tables, non-standard LUTs) → explicit error naming the feature. Huge files → tiled path with progress + cancellation (never frozen UI).

## 8. Golden Image Oracle (correctness gates — CI-enforced, CPU always)

| Test | Metric | Threshold |
|---|---|---|
| ICC roundtrip (sRGB→AP1→sRGB, P3↔AP1) | max abs error (8-bit scale) | ≤ 1/255 · 0.5 |
| Fast-path matrix vs lcms2 (patch set) | ΔE00 mean / max | ≤ 0.1 / ≤ 0.5 |
| Synthetic ColorChecker (own license-clean values) through S1→S4 | ΔE00 vs CPU reference (GPU run) | ≤ 0.5 mean, ≤ 1.0 max |
| Neutral 21-step ramp | chromaticity drift ΔE00 | ≤ 0.3 |
| Near-black gradients (post-quantization) | banding detector (Δ between adjacent steps) | ≤ 1 LSB with dither on |
| Premultiplied↔straight alpha roundtrip | max abs error | ≤ 1/255 · 0.25 |
| Saturated primaries + skin tones + grayscale patches | ΔE00 (GPU vs CPU per stage) | per-stage published tolerances (COLOR tests file, M1) |

**SSIM** used for structural comparisons of preview vs export at matched resolution. **ΔE00 is a pipeline-correctness metric only** — never presented as proof of film-stock authenticity (FILM_MODEL.md §9 keeps those separate).

## 9. Test asset policy

Synthetic charts generated procedurally (license-clean, `tests/assets/` with a `provenance.json` entry each); real-world reference images only owner-supplied or licensed (never scraped, never committed user photos).
