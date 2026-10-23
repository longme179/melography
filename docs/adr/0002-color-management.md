# ADR-0002: Color Management & Working Space

**Status:** Accepted · **Date:** 2026-10-24
**Related:** `docs/COLOR_PIPELINE.md` (full spec), ADR-0001 (S2 spike)

## Context

The pipeline must: decode arbitrary ICC-tagged inputs (JPEG/PNG/TIFF/WebP, optionally HEIC/AVIF), process in a mathematically-defined space suitable for *physical film emulation* (linear-light compositing, exposure math, density formation), render previews through the OS display profile, and export with configurable profile/intent — identically on Linux/Windows/macOS, on CPU (oracle) and GPU (wgpu), with documented tolerances.

## Options considered (working representation)

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| Linear Rec.2020 | Wide, standardized, no imaginary primaries | Broadcast/HDR baggage; no compositing ecosystem; D65-only framing | Rejected |
| Linear ProPhoto (ROMM) | Wide, photography heritage | Imaginary green primary → negative XYZ excursions; D50 working white mixed into common practice | Rejected |
| **ACEScg (AP1, linear, f32)** | Purpose-built for CG/compositing; published stable primaries (OpenColorIO reference); all-positive-ish coverage adequate for camera-originated content; industry precedent; clean docs for negatives handling | Slightly smaller gamut than ProPhoto in far greens/blues (irrelevant for camera images); white point ACES ~D60 requires explicit chromatic adaptation bookkeeping | **Selected** |
| Linear Rec.709 | Simple | Too narrow — clips P3/2020 camera originals before film stage even runs | Rejected |

## Decisions

1. **Working space: ACEScg (AP1 primaries, linear, `f32` per channel, unbounded).** White point handling: AP1 is defined with the ACES white (~D60); all input conversions go input-ICC → XYZ(D50) → Bradford CAT → AP1. Negative/out-of-range values are **preserved** through the pipeline (clamped only at documented boundaries: LUT domain, display transform input, export encode), enabling gamut mapping rather than silent clipping.
2. **ICC machinery: LittleCMS (`lcms2` crate, MIT)** for (a) any embedded input profile → AP1, (b) AP1 → display/soft-proof profile, (c) export transforms. **Never** approximate an arbitrary ICC with a 3×3. Built-in fast paths (sRGB↔AP1, Display-P3↔AP1) are hand-written matrix implementations *validated against lcms2* in golden tests (they are optimizations, not replacements).
3. **Decode/sanitize stage:** apply EXIF orientation; unpremultiply alpha (with all-opaque fast path); identify actual encoding (assume sRGB **only** when metadata is absent; UI warns when ambiguity is consequential — e.g., untagged JPEG + wide-gamut monitor).
4. **Pseudo-scene expansion is optional & reversible:** a log-shaped expansion (filmic-inverse family) converts display-referred input to an exposure proxy; it can be bypassed per-image/recipe; UI labels it "proxy — clipped data cannot be recovered". It is **not** RAW recovery and never marketed as such.
5. **Display path:** working → display ICC via lcms2 (OS-retrieved profile; HDR/P3 behavior documented per-OS with limits — v1 targets correctly-managed SDR; explicit HDR output is deferred). Spike S2 verifies the webview canvas performs **no additional** hidden transform (double-transform detection test included).
6. **Stage I/O contracts:** every pipeline stage declares input/output space + encoding (see COLOR_PIPELINE.md §contracts). LUT stages wrap themselves in their declared encoding (ADR in `docs/research/luts.md` §3). Light/transfer-encoded pixels are never blended where linear compositing is required.
7. **Precision:** CPU oracle f32 (f64 for curve fitting offline); GPU f32 compute (f16 only where measured error is within tolerance, and only for stages proven safe — e.g. final dither); premultiplied alpha only across resampling boundaries, straight alpha in compositing-adjacent stages, converted at documented points.
8. **Golden Image Oracle (correctness, not film authenticity):** synthetic ColorChecker-class patch sets (our own values, license-clean) + real charts later; metrics ΔE00 (patch mean/max), SSIM (grayscale ramps), ICC roundtrip identity (max |Δ| ≤ 1/8-bit-LSB), CPU↔GPU per-stage tolerances (published per stage; revision requires ADR-level note in the test file). ΔE00 is never presented as proof of "film authenticity" — it validates *pipeline correctness* only.

## Consequences

- Deterministic, documented color math that a reviewer can check against published ACES/ICC references.
- lcms2 (C library) enters the dependency tree — vendored via crate `lcms2-sys` (MIT), supply-chain-checked (cargo-deny) like everything else; spike S2 proves cross-OS build sanity.
- Wide-gamut inputs survive to the film stage, where the emulsion model deliberately maps them into dye-gamut (that's physically what film does — feature, not bug).
