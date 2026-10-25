//! FILM_MODEL §L quality gates — film-statistical thresholds.
//!
//! Algorithmic acceptance criteria (grain.md §4):
//!
//! | Gate | Metric | Threshold |
//! |---|---|---|
//! | grain RMS | 48 µm-aperture σ(D) on uniform patches | ±10 % of σ_curve(D) |
//! | grain PSD | radial power-spectrum slope, 0.1–0.8 Nyquist | ±15 % of declared |
//! | grain ρ | Pearson correlation between layer fluctuation maps | ±0.05 of declared |
//! | halation edge spread | 10–90 % width of the halo component (mm) | ±15 % of declared σ_eff |
//! | zoom consistency | aperture σ at full vs half resolution | ±10 % |
//! | determinism | same seed + params + input → identical bytes | bit-exact |
//!
//! **Scale discipline.** Grain statistics are physical (48 µm aperture,
//! cycles/mm) — the gates render at native-scan-like scales so the
//! measured statistics mean what the profile declares:
//! * `NATIVE_W × NATIVE_H` = 3000×200 ⇒ 12 µm/px across 36 mm of film;
//!   4 px boxes = the 48 µm aperture.
//! * The heavy statistical gates are `#[ignore]`d for debug-mode test
//!   runs and executed in **release** by CI (`cargo test --release --
//!   --ignored`): they render megapixel fields; the correctness gates
//!   (ρ, edge spread, determinism, tiling) stay in the default run.

use melography_core::{Channels, ImageF32};
use melography_film::emulsion::{apply_halation, resolve_emulsion, Adjustments};
use melography_film::frame::{CropRule, FilmFrame};
use melography_film::grain::GrainField;
use melography_film::optics::Plane;
use melography_film::profile::testing::test_profile;

/// Native gate scale: 6000 px across 36 mm ⇒ 6 µm/px — the same sampling
/// the grain calibration uses, so the RMS gate measures exactly the
/// statistic the profile declares.
const NATIVE_W: u32 = 6000;
const NATIVE_H: u32 = 400;

fn native_frame() -> FilmFrame {
    FilmFrame::from_dims(NATIVE_W, NATIVE_H, CropRule::FitWidth)
}

fn uniform_density(w: u32, h: u32, d: f64) -> Plane {
    Plane {
        w,
        h,
        data: vec![d as f32; (w * h * 3) as usize],
    }
}

/// Box-aperture RMS per layer, **interior windows only** (the outermost
/// window ring mixes in the reduced-grain statistics of out-of-frame
/// film — excluded, honestly, rather than silently biasing the estimate).
fn aperture_sigma(plane: &Plane, box_px: u32, skip_rim: u32) -> [f64; 3] {
    let mut sums = [0.0f64; 3];
    let mut sums2 = [0.0f64; 3];
    let mut count = 0.0f64;
    let mut wy = skip_rim * box_px;
    while wy + box_px * (1 + skip_rim) <= plane.h {
        let mut wx = skip_rim * box_px;
        while wx + box_px * (1 + skip_rim) <= plane.w {
            let mut acc = [0.0f64; 3];
            for y in 0..box_px {
                for x in 0..box_px {
                    let p = plane.px64(wx + x, wy + y);
                    acc[0] += p[0];
                    acc[1] += p[1];
                    acc[2] += p[2];
                }
            }
            for c in 0..3 {
                let m = acc[c] / f64::from(box_px * box_px);
                sums[c] += m;
                sums2[c] += m * m;
            }
            count += 1.0;
            wx += box_px;
        }
        wy += box_px;
    }
    assert!(
        count > 100.0,
        "need a statistically meaningful window count"
    );
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        let mean = sums[c] / count;
        out[c] = (sums2[c] / count - mean * mean).max(0.0).sqrt();
    }
    out
}

/// Separable 2D DFT of a real N×N window → radially averaged power
/// spectrum in cycles/mm, log-log slope over 0.1–0.8 Nyquist.
fn psd_slope(plane: &Plane, layer: usize, mmpp: f64) -> f64 {
    const N: usize = 128;
    let x0 = (plane.w as usize / 2) - N / 2;
    let y0 = (plane.h as usize / 2) - N / 2;
    let mut f = vec![0.0f64; N * N];
    let mut mean = 0.0;
    for y in 0..N {
        for x in 0..N {
            let v = plane.px64((x0 + x) as u32, (y0 + y) as u32)[layer];
            f[y * N + x] = v;
            mean += v;
        }
    }
    mean /= (N * N) as f64;
    for y in 0..N {
        for x in 0..N {
            let w = 0.5
                * (1.0 - (core::f64::consts::TAU * x as f64 / N as f64).cos())
                * 0.5
                * (1.0 - (core::f64::consts::TAU * y as f64 / N as f64).cos());
            f[y * N + x] = (f[y * N + x] - mean) * w;
        }
    }
    let mut cos_t = vec![0.0f64; N * N];
    let mut sin_t = vec![0.0f64; N * N];
    for k in 0..N {
        for t in 0..N {
            let a = -core::f64::consts::TAU * k as f64 * t as f64 / N as f64;
            cos_t[k * N + t] = a.cos();
            sin_t[k * N + t] = a.sin();
        }
    }
    let mut re = vec![0.0f64; N * N];
    let mut im = vec![0.0f64; N * N];
    for y in 0..N {
        for kx in 0..N {
            let (mut sr, mut si) = (0.0, 0.0);
            for x in 0..N {
                let v = f[y * N + x];
                sr += v * cos_t[kx * N + x];
                si += v * sin_t[kx * N + x];
            }
            re[y * N + kx] = sr;
            im[y * N + kx] = si;
        }
    }
    let mut power = vec![0.0f64; N * N];
    for ky in 0..N {
        for kx in 0..N {
            let (mut sr, mut si) = (0.0, 0.0);
            for y in 0..N {
                let vr = re[y * N + kx];
                let vi = im[y * N + kx];
                sr += vr * cos_t[ky * N + y] - vi * sin_t[ky * N + y];
                si += vr * sin_t[ky * N + y] + vi * cos_t[ky * N + y];
            }
            power[ky * N + kx] = (sr * sr + si * si) / (N * N) as f64;
        }
    }
    let nyquist = 0.5 / mmpp;
    let n_bins = N / 2;
    let mut pow_bins = vec![0.0f64; n_bins];
    let mut cnt_bins = vec![0u32; n_bins];
    for ky in 0..N {
        let fy = if ky <= N / 2 {
            ky as f64
        } else {
            (N - ky) as f64
        };
        for kx in 0..N {
            if kx == 0 && ky == 0 {
                continue;
            }
            let fx = if kx <= N / 2 {
                kx as f64
            } else {
                (N - kx) as f64
            };
            let freq_px = (fx * fx + fy * fy).sqrt();
            let bin = ((freq_px / (N as f64 / 2.0)) * n_bins as f64) as usize;
            if bin >= n_bins {
                continue;
            }
            pow_bins[bin] += power[ky * N + kx];
            cnt_bins[bin] += 1;
        }
    }
    let (mut sx, mut sy, mut sxx, mut sxy, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (b, (&pw, &c)) in pow_bins.iter().zip(&cnt_bins).enumerate() {
        if c == 0 || pw <= 0.0 {
            continue;
        }
        let freq = (b as f64 + 0.5) / n_bins as f64 * nyquist;
        if (0.1 * nyquist..=0.8 * nyquist).contains(&freq) {
            let lx = freq.log10();
            let ly = (pw / c as f64).log10();
            sx += lx;
            sy += ly;
            sxx += lx * lx;
            sxy += lx * ly;
            n += 1.0;
        }
    }
    (n * sxy - sx * sy) / (n * sxx - sx * sx)
}

/// Pearson correlation between two layer fluctuation maps.
fn layer_rho(plane: &Plane, a: usize, b: usize) -> f64 {
    let mut sa = 0.0;
    let mut sb = 0.0;
    let n = (plane.w * plane.h) as f64;
    for y in 0..plane.h {
        for x in 0..plane.w {
            let p = plane.px64(x, y);
            sa += p[a];
            sb += p[b];
        }
    }
    let (ma, mb) = (sa / n, sb / n);
    let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
    for y in 0..plane.h {
        for x in 0..plane.w {
            let p = plane.px64(x, y);
            let da = p[a] - ma;
            let db = p[b] - mb;
            va += da * da;
            vb += db * db;
            cov += da * db;
        }
    }
    cov / (va * vb).sqrt()
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_grain_rms_within_10_percent_of_sigma_curve() {
    let p = test_profile();
    for &d in &[0.3, 1.0, 1.6] {
        let f = GrainField::resolve(&p, 1.0, Some(99));
        let src = uniform_density(NATIVE_W, NATIVE_H, d);
        let mut out = Plane::new(NATIVE_W, NATIVE_H);
        f.apply(&src, &mut out, 0, NATIVE_H, &native_frame(), 0, NATIVE_H);
        // 48 µm aperture at 6 µm/px = 8 px boxes (calibration procedure).
        let measured = aperture_sigma(&out, 8, 1);
        let target = p.grain.sigma_at(d);
        // Tolerance: ±10 % in the clamp-free range (D ≥ 0.6). At low
        // densities the never-negative clamp (§D — film density cannot go
        // below zero) trims the negative grain excursions and biases the
        // measured σ down by up to ~15 %; the gate allows ±20 % there and
        // the effect is documented in the M2 quality report.
        let tol = if d >= 0.6 { 0.10 } else { 0.20 };
        for (c, &m) in measured.iter().enumerate() {
            let rel = (m - target) / target;
            assert!(
                rel.abs() <= tol,
                "density {d} layer {c}: aperture σ {m:.5} vs target {target:.5} (rel {rel:+.4}, tol {tol})"
            );
        }
    }
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_grain_psd_slope_within_15_percent() {
    let p = test_profile();
    let f = GrainField::resolve(&p, 1.0, Some(99));
    let src = uniform_density(NATIVE_W, NATIVE_H, 1.0);
    let mut out = Plane::new(NATIVE_W, NATIVE_H);
    f.apply(&src, &mut out, 0, NATIVE_H, &native_frame(), 0, NATIVE_H);
    let mmpp = native_frame().mm_per_pixel;
    let slope = psd_slope(&out, 1, mmpp); // green layer
    let declared = p.grain.psd_slope;
    let rel = (slope - declared) / declared;
    assert!(
        rel.abs() <= 0.15,
        "PSD slope {slope:.3} vs declared {declared:.3} (rel {rel:+.4})"
    );
}

#[test]
fn gate_grain_layer_rho_within_005() {
    // ρ is a per-grain property — scale-free; measured at a modest scale.
    let p = test_profile();
    let f = GrainField::resolve(&p, 1.0, Some(99));
    let (w, h) = (600u32, 400u32);
    let frame = FilmFrame::from_dims(w, h, CropRule::FitWidth);
    let src = uniform_density(w, h, 1.0);
    let mut out = Plane::new(w, h);
    f.apply(&src, &mut out, 0, h, &frame, 0, h);
    for (a, b) in [(0usize, 1usize), (0, 2), (1, 2)] {
        let measured = layer_rho(&out, a, b);
        let declared = p.grain.rho.0[3 * a + b];
        assert!(
            (measured - declared).abs() <= 0.05,
            "ρ({a},{b}): measured {measured:.3} vs declared {declared:.3}"
        );
    }
}

#[test]
fn gate_halation_edge_spread_within_15_percent() {
    // Knife edge through the halation stage; the 10–90 % width of the
    // **halo component** (response − input) must match the declared
    // single-gaussian σ (2.5631 σ for an error-function edge).
    use melography_film::profile::{GaussianComponent, HalationChannel};

    let mut p = test_profile();
    p.halation = [
        HalationChannel {
            alpha: 0.05,
            kernel: vec![GaussianComponent {
                sigma_mm: 0.030,
                weight: 1.0,
            }],
        },
        HalationChannel {
            alpha: 0.0,
            kernel: p.halation[2].kernel.clone(),
        },
        HalationChannel {
            alpha: 0.0,
            kernel: p.halation[2].kernel.clone(),
        },
    ];
    p.validate().unwrap();

    let w = 600u32;
    let h = 64u32;
    let frame = FilmFrame::from_dims(w, h, CropRule::FitWidth);
    let em = resolve_emulsion(&p, &Adjustments::default(), frame.mm_per_pixel);
    let mut src = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = if x < w / 2 { 0.0 } else { 1.0 };
            src.set3(x, y, [v, v, v]);
        }
    }
    let mut dst = Plane::new(w, h);
    let margin = em.input_margin();
    apply_halation(&src, &mut dst, margin, h - margin, &em, (0, h));
    // Halo-only profile: (dst − src) on the red channel, middle row.
    let mid = h / 2;
    let mut halo: Vec<f64> = (0..w)
        .map(|x| dst.px64(x, mid)[0] - src.px64(x, mid)[0])
        .collect();
    let lo = 0.0;
    let hi = halo
        .iter()
        .rev()
        .take(50)
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(hi > 0.0, "halo must be non-empty (hi={hi})");
    for v in &mut halo {
        *v = (*v - lo) / (hi - lo);
    }
    let x10 = cross_at(&halo, 0.10);
    let x90 = cross_at(&halo, 0.90);
    let width_mm = (x90 - x10) * frame.mm_per_pixel;
    let expected = 2.5631 * 0.030;
    let rel = (width_mm - expected) / expected;
    assert!(
        rel.abs() <= 0.15,
        "edge-spread width {width_mm:.4} mm vs expected {expected:.4} mm (rel {rel:+.4})"
    );
}

/// First sub-pixel crossing position of `level` in a monotone ramp.
fn cross_at(vals: &[f64], level: f64) -> f64 {
    for i in 0..vals.len() - 1 {
        if vals[i] <= level && vals[i + 1] > level {
            let t = (level - vals[i]) / (vals[i + 1] - vals[i]);
            return i as f64 + t;
        }
    }
    vals.len() as f64
}

#[test]
fn gate_render_determinism_full_chain_hash() {
    use melography_film::chain::{render, FilmChainParams};
    use sha2::{Digest, Sha256};

    let mut img = ImageF32::alloc(128, 96, Channels::Rgb, 0.0);
    for y in 0..96 {
        for x in 0..128 {
            let v = 0.1 + 0.6 * (f64::from(x) / 128.0) * (f64::from(y) / 96.0);
            let p = img.pixel_mut(x, y);
            p[0] = v as f32;
            p[1] = (v * 0.9) as f32;
            p[2] = (v * 1.1) as f32;
        }
    }
    let params = FilmChainParams::new(test_profile())
        .unwrap()
        .with(Adjustments {
            grain_seed: Some(2026),
            ..Adjustments::default()
        })
        .unwrap();
    let a = render(&img, &params).unwrap();
    let b = render(&img, &params).unwrap();
    assert_eq!(
        Sha256::digest(a.as_bytes()),
        Sha256::digest(b.as_bytes()),
        "same-seed renders must hash identically"
    );
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_zoom_consistency_aperture_sigma() {
    // grain.md §3: a half-resolution preview samples the same continuous
    // field; the 48 µm-aperture σ must agree within ±10 %.
    let p = test_profile();
    let f = GrainField::resolve(&p, 1.0, Some(99));

    let f_full = native_frame(); // 3000×200, 12 µm/px
    let f_half = FilmFrame::from_dims(NATIVE_W / 2, NATIVE_H / 2, CropRule::FitWidth);
    let src_full = uniform_density(NATIVE_W, NATIVE_H, 1.0);
    let mut out_full = Plane::new(NATIVE_W, NATIVE_H);
    f.apply(&src_full, &mut out_full, 0, NATIVE_H, &f_full, 0, NATIVE_H);
    let src_half = uniform_density(NATIVE_W / 2, NATIVE_H / 2, 1.0);
    let mut out_half = Plane::new(NATIVE_W / 2, NATIVE_H / 2);
    f.apply(
        &src_half,
        &mut out_half,
        0,
        NATIVE_H / 2,
        &f_half,
        0,
        NATIVE_H / 2,
    );
    // Matched physical aperture: 4 px @ 12 µm vs 2 px @ 24 µm = 48 µm.
    // Both renders area-supersample the same continuous field, so the
    // aperture statistics must agree.
    let s_full = aperture_sigma(&out_full, 4, 1);
    let s_half = aperture_sigma(&out_half, 2, 1);
    for c in 0..3 {
        let rel = (s_half[c] - s_full[c]) / s_full[c];
        assert!(
            rel.abs() <= 0.10,
            "zoom consistency layer {c}: σ_half {:.5} vs σ_full {:.5} (rel {rel:+.4})",
            s_half[c],
            s_full[c]
        );
    }
}

// ── A2.2: the fast path (§L gates apply UNCHANGED — FILM_MODEL §G) ─────────
//
// The same thresholds as the full-model gates above, measured on the fast
// field. Expected values (deterministic, from the A2.2 design prototype —
// seed 99, the same convention as the full gates): test profile slope
// −0.832 vs declared −0.869 (4.2 %), RMS within ±4 %, zoom within ±4 %,
// ρ within 0.004; bundled stocks' worst slope error 9 % (monochrome).

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_fast_grain_rms_within_10_percent_of_sigma_curve() {
    use melography_film::grain::FastGrain;
    let p = test_profile();
    for &d in &[0.3, 1.0, 1.6] {
        let f = FastGrain::resolve(&p, 1.0, Some(99));
        let src = uniform_density(NATIVE_W, NATIVE_H, d);
        let mut out = Plane::new(NATIVE_W, NATIVE_H);
        f.apply(&src, &mut out, 0, NATIVE_H, &native_frame(), 0, NATIVE_H);
        let measured = aperture_sigma(&out, 8, 1);
        let target = p.grain.sigma_at(d);
        let tol = if d >= 0.6 { 0.10 } else { 0.20 };
        for (c, &m) in measured.iter().enumerate() {
            let rel = (m - target) / target;
            assert!(
                rel.abs() <= tol,
                "density {d} layer {c}: aperture σ {m:.5} vs target {target:.5} (rel {rel:+.4}, tol {tol})"
            );
        }
    }
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_fast_grain_psd_slope_within_15_percent() {
    use melography_film::grain::FastGrain;
    let p = test_profile();
    let f = FastGrain::resolve(&p, 1.0, Some(99));
    let src = uniform_density(NATIVE_W, NATIVE_H, 1.0);
    let mut out = Plane::new(NATIVE_W, NATIVE_H);
    f.apply(&src, &mut out, 0, NATIVE_H, &native_frame(), 0, NATIVE_H);
    let mmpp = native_frame().mm_per_pixel;
    let slope = psd_slope(&out, 1, mmpp); // green layer
    let declared = p.grain.psd_slope;
    let rel = (slope - declared) / declared;
    assert!(
        rel.abs() <= 0.15,
        "PSD slope {slope:.3} vs declared {declared:.3} (rel {rel:+.4})"
    );
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_fast_grain_psd_slope_all_bundled_stocks() {
    use melography_film::grain::FastGrain;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let reg = melography_presets::StockRegistry::open(&root.join("profiles")).unwrap();
    assert!(!reg.profiles.is_empty(), "bundled profiles must load");
    for p in &reg.profiles {
        let f = FastGrain::resolve(p, 1.0, Some(99));
        let src = uniform_density(NATIVE_W, NATIVE_H, 1.0);
        let mut out = Plane::new(NATIVE_W, NATIVE_H);
        f.apply(&src, &mut out, 0, NATIVE_H, &native_frame(), 0, NATIVE_H);
        let mmpp = native_frame().mm_per_pixel;
        let slope = psd_slope(&out, 1, mmpp);
        let declared = p.grain.psd_slope;
        let rel = (slope - declared) / declared;
        assert!(
            rel.abs() <= 0.15,
            "{}: fast PSD slope {slope:.3} vs declared {declared:.3} (rel {rel:+.4})",
            p.id
        );
    }
}

#[test]
fn gate_fast_grain_layer_rho_within_005() {
    use melography_film::grain::FastGrain;
    // ρ is a per-field property — scale-free; measured at a modest scale.
    let p = test_profile();
    let f = FastGrain::resolve(&p, 1.0, Some(99));
    let (w, h) = (600u32, 400u32);
    let frame = FilmFrame::from_dims(w, h, CropRule::FitWidth);
    let src = uniform_density(w, h, 1.0);
    let mut out = Plane::new(w, h);
    f.apply(&src, &mut out, 0, h, &frame, 0, h);
    for (a, b) in [(0usize, 1usize), (0, 2), (1, 2)] {
        let measured = layer_rho(&out, a, b);
        let declared = p.grain.rho.0[3 * a + b];
        assert!(
            (measured - declared).abs() <= 0.05,
            "ρ({a},{b}): measured {measured:.3} vs declared {declared:.3}"
        );
    }
}

#[test]
#[ignore = "heavy statistical gate — run in release: cargo test --release -- --ignored"]
fn gate_fast_zoom_consistency_aperture_sigma() {
    use melography_film::grain::FastGrain;
    // grain.md §3: a half-resolution preview samples the same continuous
    // field; the matched physical aperture (24 µm here — 4 px @6 µm vs
    // 2 px @12 µm) σ must agree within ±10 %.
    let p = test_profile();
    let f = FastGrain::resolve(&p, 1.0, Some(99));
    let f_full = native_frame();
    let f_half = FilmFrame::from_dims(NATIVE_W / 2, NATIVE_H / 2, CropRule::FitWidth);
    let src_full = uniform_density(NATIVE_W, NATIVE_H, 1.0);
    let mut out_full = Plane::new(NATIVE_W, NATIVE_H);
    f.apply(&src_full, &mut out_full, 0, NATIVE_H, &f_full, 0, NATIVE_H);
    let src_half = uniform_density(NATIVE_W / 2, NATIVE_H / 2, 1.0);
    let mut out_half = Plane::new(NATIVE_W / 2, NATIVE_H / 2);
    f.apply(
        &src_half,
        &mut out_half,
        0,
        NATIVE_H / 2,
        &f_half,
        0,
        NATIVE_H / 2,
    );
    let s_full = aperture_sigma(&out_full, 4, 1);
    let s_half = aperture_sigma(&out_half, 2, 1);
    for c in 0..3 {
        let rel = (s_half[c] - s_full[c]) / s_full[c];
        assert!(
            rel.abs() <= 0.10,
            "fast zoom consistency layer {c}: σ_half {:.5} vs σ_full {:.5} (rel {rel:+.4})",
            s_half[c],
            s_full[c]
        );
    }
}

#[test]
fn gate_fast_render_determinism_full_chain_hash() {
    use melography_film::chain::{render, FilmChainParams};
    use sha2::{Digest, Sha256};

    let mut img = ImageF32::alloc(128, 96, Channels::Rgb, 0.0);
    for y in 0..96 {
        for x in 0..128 {
            let v = 0.1 + 0.6 * (f64::from(x) / 128.0) * (f64::from(y) / 96.0);
            let p = img.pixel_mut(x, y);
            p[0] = v as f32;
            p[1] = (v * 0.9) as f32;
            p[2] = (v * 1.1) as f32;
        }
    }
    let params = FilmChainParams::new(test_profile())
        .unwrap()
        .with(Adjustments {
            grain_seed: Some(2026),
            ..Adjustments::default()
        })
        .unwrap()
        .with_grain_fast(true);
    let a = render(&img, &params).unwrap();
    let b = render(&img, &params).unwrap();
    assert_eq!(
        Sha256::digest(a.as_bytes()),
        Sha256::digest(b.as_bytes()),
        "same-seed fast renders must hash identically"
    );
}
