# Research Notes — Film & Color Science

**Distilled 2026-10-24. Sources: pinned entries in `REFERENCES.md` §B. All claims below are design conclusions from studying the cited works; no code was copied in Phase 0.**

## 1. What "film look" actually consists of (decomposition for our pipeline)

Studying darktable (filmic + negadoctor + grain), Filmulator, spektrafilm/agx-emulsion, RawTherapee HaldCLUT, and G'MIC film filters, the reproducible physical chain reduces to these independently-parameterized stages (full math spec: `docs/FILM_MODEL.md`):

1. **Exposure reconstruction** — mapping already-tone-mapped, clipped sRGB input to a pseudo-scene linear exposure proxy. darktable's filmic teaches the *display-referred → scene-referred inversion* shape (log encoding with pivot latitude); Filmulator starts from *true* linear raw data, which we do not have — hence our proxy must be optional, reversible, and honest about unrecoverable clipped data.
2. **Layer-wise density response** — per-channel H&D (Hurter–Driffield) characteristic curves: toe / straight-line / shoulder with base+fog. darktable negadoctor models this for negative inversion (paper dark, film orange mask); spektrafilm goes further with spectral layer coupling.
3. **Inter-channel crosstalk** — dye impurity: each layer's density contaminates other channels (spectral overlap / inter-image effects). This is what separates "per-channel curve stacking" (cheap, wrong) from a plausible emulsion model. Modeled as a coupling matrix + optional non-linear terms.
4. **Halation** — red-biased back-reflection off the film base, proportional to *exposure* (happens at exposure time, before development), wavelength-dependent (red ≫ green ≫ blue), spatially spread by a physically-sized kernel in film-plane coordinates.
5. **Grain** — stochastic, density-dependent, spectrally-correlated (silver clouds scatter into multiple layers) — **not** independent RGB noise. See `grain.md`.
6. **Print/scan second stage** — negative → paper characteristic (second tone curve family) or scanner inversion (log/3×3/gamma). Ignoring this stage is the single most common reason naive film filters look like Instagram rather than film.
7. **Optics & finish** — vignetting, veiling glare, scanner MTF, optional wear (default off).

## 2. Key algorithmic findings from sources

- **darktable filmic** (GPL-3.0, `src/iop/filmic.c`): the useful concept is the *fourth-degree rational sigmoid* in log space with defined latitude/contrast/pivot; its display↔scene conversions are well-tested. Our plan: re-derive the same *shape family* in our own parameterization, validated by golden tests, rather than transcribing formulas blindly.
- **darktable negadoctor** (`src/iop/negadoctor.c`): models D_min offset (paper black), film orange mask as a Dmin density vector, and print exposure through a paper curve — directly informs our stage F.
- **spektrafilm / agx-emulsion** (GPL-3.0): spectral approach — convert exposure to *layer excitations* via spectral sensitivity functions, develop to dye densities via spectral couplers, integrate back to tristimulus. Full spectral integration is expensive; we adopt a **3-channel approximation with an explicit spectral-basis note** (documented simplification, upgradeable): layer excitation = M_exc · E, dye density response per layer, then M_dye · D back to density space. Their LUT assets are custom-licensed — never shipped.
- **Filmulator** (license ambiguous — study only): the film-development ODE (richardson-like diffusion + development rate) inspires our optional "development character" controls; we will not port it verbatim.
- **RawTherapee HaldCLUT**: proof that LUT-based film looks are practical, and a caution: Hald CLUTs bake the *entire* chain into one transform, losing editability and HDR robustness — our LUT engine (`luts.md`) treats LUTs as one composable stage, never the whole chain.

## 3. Color pipeline decisions feeding ADR-0002

- Working representation: **scene-linear ACEScg (AP1) f32** — primaries/white point published and stable (OpenColorIO reference), wide enough for wide-gamut inputs, industry precedent for compositing-style pipelines; ProPhoto rejected (imaginary green primary, D50 mixing), Rec.2020 rejected (HDR broadcasting baggage, no compositing ecosystem), Rec.709 rejected (too narrow — clips P3/2020 camera inputs).
- Actual ICC handling (embedded profiles, display profiles): **LittleCMS via `lcms2` crate** (MIT, 6.2.0). We never approximate arbitrary ICC with a 3×3; the CMM handles input→AP1 and AP1→display.
- Pseudo-scene expansion: optional `filmic-inverse`-shaped log→linear expansion, documented as a *proxy* with clip warnings in UI; never advertised as RAW recovery.

## 4. Calibration evidence plan (what we will measure, not assert)

- H&D curves: published densitometric data for *generic* film classes (public-domain charts, e.g. Kodak data sheets are copyrighted but *citable*; we extract curve shape parameters, not redistribute data) → fit monotonic splines; error target: max |ΔD| ≤ 0.02 over fitted range on normalized axis.
- Neutral ramps (gray ladder) through full chain: output must stay neutral (ΔE00 ≤ 1.0 vs. intended paper-gray trajectory) unless the stock profile deliberately shifts neutrals.
- ColorChecker-style charts under D50/D65 render conditions: ΔE00 reported per profile against our own reference implementation, never against "the film itself" without licensed reference scans.
- Everything labeled `measured` requires a recorded dataset + fit report in `tools/provenance.json`; otherwise profiles ship as `artist-authored`.

## 5. Open items (honest gaps)

- Exact spectral sensitivity data licensing for generic emulsion classes — find license-clean sources at M2 or stay with curve-shape approximations.
- HDR/P3 display verification matrix per OS (Windows Advanced Color, macOS EDR, Linux HDR protocol state) — deferred to M0 Spike S2 report; v1 targets SDR-managed displays with correct ICC.
