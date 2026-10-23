# MELOGRAPHY — Film Emulation Model (Mathematical Specification)

**Version:** Phase-0 spec · **Date:** 2026-10-24
**Scope:** implementable math, not slider poetry. Physical/sensitometric approximation is explicitly separated from artistic additions (`[ART]` tags); users are told which parameters lack measured calibration.

**Position statement:** we target *convincing negative, reversal, print and scan behavior* for generic film classes. We do **not** claim exact recreation of branded stocks without calibration data; profiles carry provenance labels (`measured` / `reference-fitted` / `artist-authored` / `experimental`) that the UI surfaces (UX_SPEC §5).

## 0. Signal domains (defined once, used everywhere)

- `E` — exposure proxy, AP1-linear, scene-ish (COLOR_PIPELINE §3).
- `X = (X_r, X_g, X_b)` — emulsion layer excitations (density-space input).
- `D = (D_r, D_g, D_b)` — developed dye densities (status-M-like proxy, normalized units).
- All kernels operate in **film-plane coordinates** `f = (u, v) ∈ [0,1]²` mapped to a 36×24 mm full-frame equivalent rectangle; aspect ratios ≠ 3:2 use an explicit crop rule recorded in the profile (`fit-width`, `fit-height`, `anchor-center` — default `fit-width`).

## A. Camera/input reconstruction

1. `E` from COLOR_PIPELINE S1–S2 (ICC-corrected, optionally pseudo-expanded). EV slider: `E ← E · 2^EV`.
2. WB: chromatic adaptation on `E` — Bradford CAT from adjustable source illuminant (cct/tint parametrized; defaults estimated by UI baseline or AI suggestion) to working white.
3. Tone-mapping inversion `[optional, reversible]` — the pseudo-scene expansion (COLOR_PIPELINE §3).
4. Clip policy: clipped input channels are flagged (histogram overlay); nothing reconstructs them — the emulsion model simply exposes what remains (physically honest).
5. Input class weighting: profiles declare `input_assumption ∈ {phone-heavily-processed, neutral-camera-jpeg, unknown}`; phone images get a pre-desaturation/de-sharpen option `[ART]` because baked-in local tone mapping fights the film response.

## B. Lens & capture optics (order: before emulsion exposure — light passes the lens first)

- Vignetting: `E ← E · cos^k(θ(u,v)·f_number_factor)` with cat-eye ellipse softening near corners; params: strength k, radius falloff `[measured-shape, artistic-range]`.
- Veiling glare / diffusion: `E ← (1−s)·E + s·(G * E)` where `G` = wide gaussian (bloom proxy) — **distinct from film halation** (§C) in shape, color, and stage; veiling is broadband and precedes the film; halation is red-biased and occurs in the film. UI keeps them separate with a tooltip.
- Low-order chromatic aberration `[ART, default off]`: per-channel radial scale ≤ 0.3%; MTF/sharpness proxy: configurable mild unsharp-on-input is explicitly `[ART]` (a "lens softness" downscale-and-resample is the default physical direction).
- Distortion `[ART, default off]`: cubic barrel/pincushion on film coords.

## C. Film exposure & halation (in the emulsion, at exposure time)

1. **Layer excitation** (3-channel approximation of spectral sensitivities):
   `X = M_exc · E`, with `M_exc` a fixed 3×3 (profile-declared, rows = layers, non-negative, diagonal-dominant; spectral-basis note recorded). Full spectral integration (spektrafilm-style) is an upgrade path, not v1.
2. **Halation** (red-biased back-reflection, energy-accounted):
   ```
   X_halo = X + H(X)
   H_r = α_r · (K_r * X_r),  H_g = α_g·(K_g * X_g),  H_b = α_b · (K_b * X_b)
   α_r ≫ α_g ≥ α_b  (α_b typically 0)
   ```
   `K_c` — separable approx of a physically-sized 2D kernel (radius in mm on film plane; gaussian-mixture with narrow core + long tail); halo margins (ARCHITECTURE §5) derive from `max_c radius(K_c)`.
   **Why red:** the film base/anti-halation layer reflects long wavelengths least-absorbed; red exposure spreads and re-exposes the red-sensitive layer around bright sources — hence *exposure-stage*, red-weighted, energy-conserving (`α` scaled so total added exposure ≤ ε·‖X‖, default ε ≤ 0.05). Lens bloom (§B) must not be used as a halation substitute.
3. Exposure reference normalization: `X` scaled so 18% gray maps to profile's mid-density reference before curves (§D).

## D. Emulsion characteristic response (H&D curves)

Per layer `c ∈ {r,g,b}`:

```
D_c = D_min,c + f_c( log10( max(X_c, X_floor) ) )        (negative stocks)
f_c: monotonic C¹ cubic-Hermite spline through control points
     { (logE_i, D_i) } : toe → straight-line (gradient γ_c) → shoulder (D_max,c)
```

- Constraints (schema-enforced): monotone non-decreasing; `D_min,c ≥ 0`; `γ_c ∈ [0.3, 1.6]`; shoulder saturates asymptotically; `X_floor` = reciprocity-floor clamp (reciprocity failure modeled only as exposure-time-dependent sensitivity shift if the profile declares `speed/latitude` data — otherwise omitted, honestly).
- **Inter-layer crosstalk:** `D ← D + C · D_off` where `C` = 3×3 coupling matrix (off-diagonal dye contamination, e.g., cyan dye absorbing in magenta layer band) — this is the "not three independent curves" correction; profile-declared, defaults mild.
- **Polarity:** negative stocks produce `D` (dense where exposed); reversal stocks use inverted characteristic `f_c` (positive slide: density *decreases* with exposure; toe/shoulder roles swap on the exposure axis). Direct-positive path skips §F's negative inversion and goes straight to §G/§H.
- Monochochrome: single-layer `D` with optional spectral weighting (`M_exc` collapses to luminance-like row) — grain remains single-layer with σ scaled to silver granularity.
- Numerical edges: splines extrapolate flat (`D_min`, `D_max`) outside control range — never negative densities, never NaN.

## E. Processing chemistry `[process-inspired, ranges physically bounded]`

- Push/pull: shifts `X` scale by `2^(±stops·0.5)` **and** multiplies `γ_c` (contrast couples to development) — both effects together, like reality.
- Development contrast: global `γ_c` multiplier ∈ [0.6, 1.6]; saturation coupling via `C` scale.
- Cross-process `[ART]`: swaps/mixes layer curve sets (e.g., E6-in-C41: increased `γ_r`, shifted `D_min`) — labeled "process-inspired" in UI, no claim of chemical equivalence.
- Bleach bypass `[ART]`: blends silver-density (luminance-weighted, high contrast) into dye densities.
- Physical plausibility guards: parameter extremes that would imply non-monotonic response or impossible densities are clamped by schema ranges with UI notice — sliders cannot create physically undefined states.

## F. Negative → positive (print/scan stage — the second nonlinear reproduction)

For negative stocks (the default chain):

1. **Orange mask:** subtract mask densities `D_mask = (m_r, m_g, m_b)` from `D` (profile-declared Dmin vector).
2. **Illumination + print/scan exposure:** `T = 10^(−(D − D_mask))` (transmission); print exposure `X_p = L_p · T` with paper/scanner light `L_p` (UI: exposure of the print stage).
3. **Paper response:** `D_p = f_p, c( log10(X_p) )` — same monotonic-spline family as §D with *paper* control points (shorter scale, harder shoulder); print contrast + black/white point controls map to `f_p` parameters.
4. **Scanner path (alternative to 3):** `log`-response + 3×3 scanner matrix + gamma — a distinct, selectable inversion mode (some stocks render better through the "scanner" model; profiles declare which path is default).
5. **Scanner glare/flare:** small additive broadband lift pre-inversion (physically: stray light in the scanner box).
6. Output: print-density-space image → linearized via paper white → AP1 via profile-declared output transform.

This two-stage nonlinearity (film curve → print curve) is what "looks like a print" instead of "looks like a filter" — it stays mandatory in the default chain (bypass allowed only as explicit `[ART]` "direct film scan" mode with a UI note).

## G. Grain (deterministic, resolution-independent — from `docs/research/grain.md`)

```
G(u, v, layer, seed, params) =
   Σ_i  w_i · ρ( ‖(u,v) − c_i‖ / r_i ) · A(D(u,v), layer)
   over grain centers c_i in a jittered lattice cell set around (u,v)
   c_i = h( floor(u·s), floor(v·s), layer, seed ) → jitter + presence (Poisson λ(D))
   r_i ~ N(μ_r, σ_r)  (film-plane μm; profile-declared per stock)
   A(D) = σ(D) amplitude curve: granularity vs density (Newson et al. formulation)
   layer correlation: shared 3×3 covariance on per-grain contribution (ρ_rg, ρ_rb, ρ_gb)
```

- Applied **after density formation, before print exposure** (physically: grain is in the negative; the print stage re-photographs it) — placement is a spec constant, tested by golden cases.
- Determinism contract: function of continuous film coords + seed only — independent of output pixel grid, tile boundaries, zoom level, backend (CPU bit-exact; GPU within COLOR_PIPELINE §8 tolerances).
- Preview at reduced resolution samples the same field with density-correct statistics (statistical-equivalence tests: RMS σ(D) ±10%, power-spectrum slope ±15%, layer ρ ±0.05 — grain.md §4).
- Interactive fast path (if needed): correlated-noise approximation calibrated to the same statistical targets; label "grain (fast preview)" in UI; export always uses the full model unless user opts out.

## H. Finish / optional wear `[ART — default pristine]`

Scanner MTF (mild deconvolution-shaped sharpening modeled as scanner transfer), tasteful soft-focus `[ART]`, borders/rebates (vector-defined with layer/coordinate semantics — drawn *under* the image frame, never resampling it), light leaks `[ART]` (parametric streaks composited at exposure stage), dust/scratches `[ART, default off]` (procedural, seeded, sparse), gate crop `[ART]`, date stamp `[ART]` (text rendered with chromatic-aberration + grain applied — it is "in the film", not overlaid after). Every spatial transform declares margins; no tile seams (tiled-vs-monolithic suite covers §H).

## I. Export color management

Final: film-chain output → (profile output transform) → COLOR_PIPELINE S5 (output ICC/intent/dither). Never a second undocumented boundary.

## J. Stock profile system (data, not code)

Versioned JSON + JSON Schema (`schemas/stock-profile.schema.json`, M2): `id` · `display_name` · `profile_version` · `provenance ∈ {measured, reference-fitted, artist-authored, experimental}` · `confidence ∈ {high, medium, low}` · `input_assumption` · `polarity` · `M_exc`, `C` · curve control points (per layer; paper curves) · `D_min`, `D_mask` · halation `{α, kernel params}` · grain `{λ, μ_r, σ_r, σ(D) curve, ρ matrix, seed default}` · print/scan path selection + params · physical references (citations, not data) · calibration data (if `measured`/`reference-fitted`: dataset pointer + fit report) · license + provenance text. Strict schema validation; explicit migration chain; parameter ranges in schema (§E guards).

**Initial profiles (all original, generic):** `Warm 400 Negative`, `Soft Pastel 160`, `Cool 800 Night`, `Vivid Reversal 100`, `Classic Monochrome 400` — artist-authored at first; upgrade path to `reference-fitted` per OPEN_QUESTIONS #6. No third-party branded names/scans/LUTs ship without permission (ADR-0003 asset rule).

## K. Fitting/calibration tooling (offline, `tools/` — M2)

Fit targets: densitometric H&D sets → §D splines (monotonic least-squares); neutral ramps → crosstalk `C`; chart colors → `M_exc`/output transform; paired images → optional end-to-end reference-LUT fit (**labeled look-matching, not stock measurement**). Optimizer comparison (Levenberg–Marquardt vs CMA-ES on the same objectives) recorded in the fit report; every fit stores dataset rights + illuminant + scanner settings. Output: profile JSON + report (residuals, per-target errors).

## L. Quality evidence (algorithmic gates vs stock fidelity — kept separate)

| Gate | Metric | Threshold (v1) |
|---|---|---|
| Algorithmic: curve monotonicity/edges | property tests + NaN scan | zero violations, all profiles |
| Algorithmic: determinism (G) | same seed → identical render | bit-exact CPU |
| Algorithmic: tiled≡monolithic | byte/hash equality (CPU) | identical |
| Film-statistical: grain | RMS σ(D), PSD slope, layer ρ (grain.md §4) | ±10% / ±15% / ±0.05 vs profile declaration |
| Film-statistical: halation | edge-spread radius on test impulse (mm film-plane) | within ±15% of declared kernel radius |
| Stock-fidelity (only for `measured`/`reference-fitted`) | normalized H&D curve error; ΔE00 on chart patches under stated rendering conditions; human A/B protocol with licensed reference scans | declared per calibration report — **no arbitrary marketing thresholds** |

Stock-fidelity numbers are reported with dataset, illuminant, and rendering conditions — never as context-free "% accurate".
