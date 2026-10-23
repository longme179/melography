# Research Notes — Film Grain

**Distilled 2026-10-24. Primary reference: Newson, Faraj, Delon, Galerne — "A stochastic film grain model for resolution-independent rendering" (IEEE TIP 2017; IPOL demonstration exists). Secondary: darktable `src/iop/grain.c` (GPL-3.0).**

## 1. Why per-pixel noise is wrong

Physical film grain is a *spatial clustering of developed silver-halide crystals* (or dye clouds), which means: (a) grain exists on the **film plane** in physical units (μm), not output pixels — zooming in must reveal *more structure*, not bigger pixels; (b) grain amplitude scales with **local density** (more exposure → more crystals → more developed density fluctuation, with stock-dependent behavior for shadows vs highlights); (c) the three color layers' grains are **correlated** through scattering (one crystal event can influence multiple layers) — RGB-independent white noise produces a digital look immediately.

## 2. Newson et al. model (concept → our implementation plan)

The paper models grain as a random point field of grain centers with normally-distributed sizes; the observed density fluctuation is the sum of contributions of grains covering the sample point:

- Grain centers: Poisson (or dead-leaves-like) point process in continuous film coordinates, intensity λ(p) modulated by local exposure.
- Grain size distribution: Gaussian with stock-dependent mean/variance (in μm).
- Per-grain contribution to density: positive blob with a chosen radial profile; luminance-dependent opacity.
- Resulting granularity σ(D) matches measured RMS-granularity when parameters are fit to published RMS/SELGRAIN numbers.

**Our v1 plan (M2):** implement the continuous-field formulation with a deterministic hash-based evaluation in film-plane coordinates (see below), keeping the paper's parameter meanings (λ, size distribution mean/σ, per-layer correlation ρ) as the stock-profile grain fields. If per-sample performance is too high for interactive preview, an approximate correlated-noise path (band-limited noise warped by density field, calibrated to the same σ(D) and power spectrum) provides the fast path — both must match statistical targets (§4).

## 3. Determinism contract (critical, from master prompt §09)

- The grain field is a function **G(film_x, film_y, layer, seed, stock_params)** — continuous coordinates in a normalized film frame (36×24 mm full-frame equivalent; explicit crop rule for other aspect ratios: grain frame anchored to image *content* rectangle, defined in the stock profile).
- Deterministic hashing: integer lattice of grain centers derived from `hash(floor(fx·s), floor(fy·s), layer, seed)`; interpolation of amplitude within cells keeps zoom-invariance behavior stable. Exact scheme specified in `docs/FILM_MODEL.md` §G.
- **Tile-independence:** any tiling of the render must evaluate the same field; tile boundaries carry overlap margins derived from the maximum grain radius (halo margin, same mechanism as halation kernel margins).
- **Zoom-consistency:** preview at 25% and export at 100% sample the same continuous field at different densities — the *statistics* (σ(D), power spectrum) must match within tolerance; the *exact pixels* differ by design (this is physically true of film too — a print and a scan of the same negative show different grain realizations only in the sense of sampling; we document this nuance and pin preview/export behavior precisely).

## 4. Quality targets (algorithmic, independent of "sounds like film")

| Metric | Definition | Target (v1) |
|---|---|---|
| RMS granularity | σ of density fluctuation on a uniform 18% gray patch, measured in a fixed 48 μm aperture equivalent | within ±10% of profile-declared σ |
| Radial power spectrum | 2D FFT power of grain on uniform patch | slope within ±15% of profile-declared slope between 0.1–0.8 Nyquist |
| Layer correlation | Pearson ρ between layer fluctuation maps | within ±0.05 of profile-declared ρ |
| Determinism | same seed+params+coords → identical bytes on CPU; GPU within float tolerance | bit-exact CPU; GPU ΔE-max ≤ 1e-3 (linear) vs CPU |
| Tile invariance | tiled render vs monolithic render | identical (CPU bit-exact; GPU ≤ tolerance) |

## 5. darktable grain lessons (GPL-3.0, studied)

darktable's grain applies noise *in a mid-pipeline space* scaled by image size proxy ("image scale" assumes a virtual print size) — the concept of **film-plane anchoring through a print-size interpretation** is exactly right; their implementation is resolution-dependent (noise generated on the pixel grid). We keep their *parametrization intent* (coarseness via physical scale) but not the pixel-grid generation.

## 6. Open items

- IPOL demonstration code license (for optional reference cross-check of our σ(D) math) — verify at M2; equations from the paper itself are re-implemented regardless.
- Real scanned grain reference datasets (for spectrum validation): must be own scans or licensed; none assumed.
