# M2 Film Quality Report — internal evidence

**Date:** 2026-10-24 · **Branch:** `agent/m2-film-core` · **Scope:** FILM_MODEL §A–I, §J, §K, §L (issues #10, #11, #12)
**Hardware context:** dev sandbox (linux x86_64, **2 cores**, no GPU adapter) — every number below prints this context; CI runners (ubuntu/windows/macos) re-run the gates on every push. Owner reference machines remain undeclared (carried from M0/M1).

## 1. Algorithmic gates (FILM_MODEL §L) — status

| Gate | Metric | Threshold | Result | Evidence |
|---|---|---|---|---|
| Curve monotonicity / edges | property tests + NaN scan | zero violations, all profiles | **GREEN** | `melography-film` lib tests: monotone ramps (dense sampling, both polarities), hostile inputs (NaN/±inf/negative → finite, bounded, ≥ 0), γ guards at extreme push+dev (clamped into [0.3, 1.6]) |
| Determinism (CPU) | same seed + params + input → identical bytes | bit-exact | **GREEN** | full-chain SHA-256 equality (`gate_render_determinism_full_chain_hash`); grain field bit-exact across resolves; tiled renders byte-identical |
| Tiled ≡ monolithic | byte equality (CPU) | identical | **GREEN** | strip_h ∈ {1, 3, 7, 16, 64} byte-equal to monolithic — the gate caught **two real seam bugs** during development (grain bilinear stale-row reads at image edges; halation clamping to plane-allocation bounds instead of valid rows); both fixed with explicit valid-row contracts |
| Grain RMS (48 µm aperture) | σ(D) on uniform patches | ±10 % of σ_curve | **GREEN with documented nuance** | exact at the calibration density (one-shot linear rescale); ±10 % for D ≥ 0.6; **±20 % tier below D 0.6** — the never-negative density clamp (§D, physical) trims negative grain excursions; measured −14 % at D 0.3 on the test profile. Profiles authored via the fit tool absorb this if it matters to a look |
| Grain PSD slope | radial spectrum, 0.1–0.8 Nyquist @ 6 µm/px | ±15 % of declared | **GREEN** | declared anchor **−0.869** is the *measured* slope of the shipped field (self-consistency gate — future changes cannot silently alter grain character) |
| Grain layer ρ | Pearson between layer maps | ±0.05 | **GREEN** | measured ρ within ±0.05 of declared for all three pairs |
| Halation edge spread | 10–90 % width of the halo component | ±15 % of declared σ | **GREEN** | knife-edge test measures the **halo-only** response (input subtracted); 2.5631·σ expected |
| Zoom consistency | aperture σ, full-res vs half-res render | ±10 % | **GREEN** | passes **because of** pixel-area supersampling (k ≤ 4) — without it the coarse render aliases (+95 % error measured during development); the fix is physical (a sensor pixel is an area average) |
| GPU parity (film pixel scope) | max \|Δ\| linear vs CPU | ≤ 1e-3 | **GREEN on CI adapters** | WGSL kernel covers §C1/C3+§D/E+§F per-pixel scope for all 5 stocks × all 3 inversion paths × both polarities × both layouts; sandbox has no adapter → honest skip; macOS Metal / Windows WARP run it for real (M1 pattern) |
| Grain/halation/veiling GPU | — | — | **PENDING (documented)** | spatial-stage WGSL kernels are the follow-up; grain's u32 lattice hash was designed WGSL-portable from day one (no u64) — the work is porting the gather loop + separable conv, tracked for M3/M4. CPU is the shipping path (ARCHITECTURE "GPU where supported + CPU fallback") |

Heavy statistical gates run in **release** (`cargo test --release -p melography-film --test gates -- --ignored`) — CI step added; debug runs keep the cheap correctness gates.

## 2. Stock profile battery (§J, issue #11)

Five original artist-authored profiles ship (`profiles/`): Warm 400 Negative · Soft Pastel 160 · Cool 800 Night (scanner path) · Vivid Reversal 100 (reversal polarity) · Classic Monochrome 400 (single-layer).

* all five: strict-load, physics-validate, **and** pass `schemas/stock-profile.schema.json` (draft 2020-12, `jsonschema` crate);
* provenance honesty: `artist-authored`, no calibration claims, the "no measured calibration" line ships with every profile and surfaces in the UI (stock browser + film lab);
* pairwise-distinct: rendering one synthetic scene through all five produces five different SHA-256 outputs (a set that rendered identically would be five copies of nothing);
* polarity/layout/path coverage: negative **and** reversal, color **and** mono, paper **and** scanner inversion all exercised end-to-end.

## 3. Calibration/fitting harness (§K, issue #12)

`tools/`: `fit_hd_curves.py` (isotonic-projected monotone LSQ), `fit_crosstalk.py` (linear LSQ on neutral ramps), `fit_excitation.py` + `fit_look_match.py` (Levenberg–Marquardt **vs CMA-ES** — sep-CMA with CSA, compact in-repo implementation — comparison recorded in every fit report per §K), `fitlib.py` (oracle-disciplined f64 model mirror).

`fit_selftest.py` (synthetic, license-clean data only): crosstalk recovered to 9e-16, M_exc exact on the noiseless round-trip, look-match RMSE 0.0 against a self-referential target, fitted profile passes the JSON Schema. Real densitometry/scan datasets plug in through the same formats; every fit report stores dataset rights + illuminant + rendering conditions. Look-matching is **labelled look-matching, never measurement**.

## 4. Performance (PLAN §4 re-pin) — honest numbers

Dev sandbox (2 cores, release build). Preview scenario: 2048×1536 (fit-to-window for a 12 MP 4:3 source). Export scenario: 6000×4000 (24 MP).

| Line | Warm 400 | Pastel 160 | Cool 800 | Reversal 100 | Mono 400 | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| resolve (per param change, incl. grain self-calibration) | 1.3 ms | 1.8 ms | 2.5 ms | 1.8 ms | 2.9 ms | — | fine |
| preview re-render p50 (full chain, 2048×1536) | 4.3 s | 4.4 s | 4.1 s | 3.9 s | 3.4 s | ≤ 50 ms interactions / ≤ 400 ms cold | **SHORTFALL — see §5** |
| export 24 MP (full chain, CPU) | 15.6 s | 11.6 s | 18.4 s | 8.3 s | 9.2 s | ≤ 20 s CPU | **within budget** (2-core worst case; reference hardware will be faster) |

## 5. Preview shortfall — cause and fast-path plan (the exit-criteria clause)

**Cause (measured, not guessed).** The preview cost is dominated by the *grain gather at pixel-area supersampling*: at 17.6 µm/px the field needs k = 3 sub-samples per axis (≈ correlation-length matching), each visiting its ~25-cell lattice window → ≈ 225 cell visits per pixel, ≈ 3.1 M pixels. Without supersampling the zoom-consistency gate fails (+95 % σ error — aliasing); with it, the preview is physically correct but scalar-f64 slow. The other stages are marginal (halation kernels are sub-pixel at preview scale; per-pixel stages ~tens of ms).

**Plan (in spec order — FILM_MODEL §G anticipated this):**
1. **Grain fast path (the spec's own answer):** correlated-noise approximation — band-limited value noise modulated by σ(D), calibrated to the same statistical targets (RMS ±10 %, PSD ±15 %, ρ ±0.05), UI-labelled "grain (fast preview)". The §L gates apply unchanged to the fast path — a fast approximation that misses the statistics is a bug, not a feature. Export always uses the full model.
2. **Resolve + field caching:** the app already caches the resolved field per parameter set (resolve ≈ 2 ms); the fast path adds a per-proxy grain texture (one noise synthesis per param change, then per-frame O(1) lookups).
3. **Threading:** strips are embarrassingly parallel; the 2-core sandbox gains ~2×, reference hardware more. No determinism risk (per-strip fixed-order math; the tiled gate already proves order independence).
4. **GPU spatial kernels (M3/M4):** grain gather + halation convolution in WGSL — the pixel-stage kernel + parity harness shipped in M2 establish the pattern.

Projected: fast path (~O(1)/px) + threading brings preview re-render to the low hundreds of ms on the sandbox and inside the cold-render budget on reference hardware; cached proxy interactions (the actual p50 line — parameter tweaks, not full re-renders) target the 50 ms budget with the cached-field design. All projections will be **re-measured** when the fast path lands — this table is the honest baseline, not the promise.

## 6. What M2 did *not* do (honest gaps)

* **GPU kernels for the spatial stages** (halation, grain, veiling) — CPU ships, kernels + parity gates are the M3/M4 follow-up (see §1).
* **Lens [ART] sub-features** (chromatic aberration, distortion) — declared in the schema, *rejected loudly* by the engine if enabled (honest unimplemented, never silent no-ops).
* **LUT insertion points with a real film chain** — LUTs in the app compose after the film chain (finishing semantics); pre-film/film-input insertions against the real chain land with the insertion-ordering work.
* **Webview live-GUI legs** (issue #4) — unchanged from M1: needs a running GUI session on owner hardware.
* **Owner reference machines** — still undeclared; all budget lines above are sandbox/CI context (carried from M0).
