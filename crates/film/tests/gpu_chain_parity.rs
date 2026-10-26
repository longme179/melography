//! A2.4 full-chain GPU↔CPU parity gates (M5-A app wiring).
//!
//! CPU reference: the **real** monolithic `chain::render` — every stage
//! on (vignette + veiling + excitation + halation + response + grain +
//! print), the exact bytes an export produces. GPU:
//! `gpu_chain::render_gpu` over a persistent context (the app's preview
//! path). Composition budget: max |Δ| ≤ 2e-3 (each constituent kernel
//! holds 1e-3 standalone; the composition doubles it for cross-stage
//! accumulation through the nonlinear H&D response — measured numbers
//! land in the log, never hidden).
//!
//! Coverage: every shipped stock with all stages on; each spatial stage
//! individually disabled (isolating its contribution); the k=1 native
//! and k=2 mid supersampling regimes via tall FitHeight images; dispatch
//! determinism (two runs → identical bytes); and the disabled-film
//! identity. The stage-split ≡ fused bit-exactness gate lives in
//! `src/gpu_chain.rs` (it needs `pub(crate)` internals). Adapter-less
//! environments print an honest skip note (the film_pixel_parity
//! pattern — CI macos/windows run for real).

#![cfg(feature = "gpu-chain")]

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render, FilmChainParams};
use melography_film::gpu_chain::render_gpu;
use melography_film::optics::LensParams;
use melography_gpu::context::AdapterPolicy;
use melography_gpu::GpuError;
use std::sync::Arc;

/// Composition tolerance: 2× the per-kernel 1e-3 budget (see the module
/// docs — the H&D response can locally amplify stage deltas).
const TOL: f64 = 2e-3;

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn ctx() -> Option<Arc<melography_gpu::context::GpuContext>> {
    match melography_gpu::context::init(&AdapterPolicy::Auto) {
        Ok(c) => Some(c),
        Err(GpuError::NoAdapter) => None,
        Err(e) => panic!("gpu context init failed: {e}"),
    }
}

/// Deterministic exposure-domain test image: a smooth 2D gradient with
/// a bright disc and per-channel tint — sweeps the H&D curves, gives
/// the gaussian stages real structure and the grain gather a varying
/// σ(D) field. All values well inside the chain's working domain.
fn test_image(w: u32, h: u32, tint: f64) -> ImageF32 {
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    let data = img.data_mut();
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w.max(1) - 1);
            let fy = f64::from(y) / f64::from(h.max(1) - 1);
            let dx = fx - 0.6;
            let dy = fy - 0.35;
            let disc = (-((dx * dx + dy * dy) / 0.01).exp() * 4.0).exp();
            let v = (0.05 + 0.7 * fx + 0.25 * (1.0 - fy) + 0.5 * disc).min(1.6);
            let i = ((y * w + x) * 3) as usize;
            data[i] = (v * (1.0 + tint)) as f32;
            data[i + 1] = v as f32;
            data[i + 2] = (v * (1.0 - tint)) as f32;
        }
    }
    img
}

/// Lens with every spatial stage exercised: vignette, veiling (σ small
/// enough to keep CI taps bounded), default FOV.
fn all_on_lens() -> LensParams {
    LensParams {
        vignette_amount: 0.45,
        veiling_amount: 0.5,
        veiling_sigma_mm: 0.35,
        ..LensParams::default()
    }
}

fn max_delta(a: &ImageF32, b: &ImageF32) -> (f64, usize) {
    let mut worst = 0.0f64;
    let mut worst_at = 0usize;
    for (idx, (x, y)) in a.data().iter().zip(b.data()).enumerate() {
        let d = (f64::from(*x) - f64::from(*y)).abs();
        if d > worst {
            worst = d;
            worst_at = idx;
        }
    }
    (worst, worst_at)
}

/// One full-chain parity case: CPU monolithic render vs GPU composition.
fn chain_case(
    name: &str,
    params: &FilmChainParams,
    img: &ImageF32,
    ctx: &Arc<melography_gpu::context::GpuContext>,
) {
    let cpu = render(img, params).expect("cpu render");
    let gpu = render_gpu(img, params, ctx).expect("gpu render");
    let (worst, at) = max_delta(&cpu, &gpu);
    eprintln!(
        "{name}: full-chain worst |Δ| = {worst:.3e} at sample {at} ({}×{})",
        img.width(),
        img.height()
    );
    assert!(
        worst <= TOL,
        "{name}: full-chain GPU parity gate failed (worst |Δ| = {worst:.3e} > {TOL}) at sample {at}"
    );
}

fn params_for(stock: &melography_film::profile::StockProfile, lens: LensParams) -> FilmChainParams {
    FilmChainParams::new(stock.clone())
        .expect("profile loads")
        .with_adjustments(Default::default(), lens)
        .expect("defaults validate")
}

#[test]
fn full_chain_parity_all_stocks_all_stages_on() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here (CI macos/windows run for real)");
        return;
    };
    eprintln!(
        "adapter: {} ({}, {})",
        ctx.report.name, ctx.report.backend, ctx.report.device_type
    );
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    // Report EVERY stock before asserting (the A2.3 lesson: one panic
    // with all profiles = one CI round with full information). The
    // worst pixel position + channel travels with each number.
    let mut results: Vec<(String, f64, (u32, u32, usize))> = Vec::new();
    for (i, stock) in reg.profiles.iter().enumerate() {
        let img = test_image(112, 84, 0.04 * (i as f64));
        let params = params_for(stock, all_on_lens());
        let cpu = render(&img, &params).expect("cpu render");
        let gpu = render_gpu(&img, &params, &ctx).expect("gpu render");
        let (worst, at) = max_delta(&cpu, &gpu);
        let px = (at / 3) as u32;
        let pos = (px % img.width(), px / img.width(), at % 3);
        eprintln!(
            "{}: full-chain worst |Δ| = {worst:.3e} at (x={}, y={}, ch={}) (all stages on, 112×84)",
            stock.id, pos.0, pos.1, pos.2
        );
        results.push((stock.id.clone(), worst, pos));
    }
    let worst_overall = results.iter().map(|r| r.1).fold(0.0f64, f64::max);
    eprintln!("all-stocks battery worst |Δ| = {worst_overall:.3e}");
    for (id, worst, pos) in &results {
        assert!(
            *worst <= TOL,
            "{id}: full-chain parity failed (worst |Δ| = {worst:.3e} > {TOL}) at (x={}, y={}, ch={})",
            pos.0,
            pos.1,
            pos.2
        );
    }
}

/// Mono-specific isolation (the round-4/6 finding: classic-monochrome-400
/// diverges while every warm case sits at 1e-7 — each spatial stage off
/// for the mono stock pins WHICH stage carries the mono delta; this leg
/// only REPORTS, the battery above owns the verdict).
#[test]
fn full_chain_parity_mono_stage_isolation() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here");
        return;
    };
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    let stock = reg.by_id("classic-monochrome-400").expect("mono stock");
    let img = test_image(112, 84, 0.02);
    let cases: &[(&str, f64, f64, f64)] = &[
        ("mono/vignette-off", 0.0, 0.5, 1.0),
        ("mono/veiling-off", 0.45, 0.0, 1.0),
        ("mono/grain-off", 0.45, 0.5, 0.0),
        ("mono/all-spatial-off", 0.0, 0.0, 0.0),
    ];
    for (name, vign, veil, grain) in cases {
        let lens = LensParams {
            vignette_amount: *vign,
            veiling_amount: *veil,
            veiling_sigma_mm: 0.35,
            ..LensParams::default()
        };
        let mut params = params_for(stock, lens);
        params.adjustments.grain_intensity = *grain;
        let cpu = render(&img, &params).expect("cpu render");
        let gpu = render_gpu(&img, &params, &ctx).expect("gpu render");
        let (worst, at) = max_delta(&cpu, &gpu);
        let px = (at / 3) as u32;
        eprintln!(
            "{name}: worst |Δ| = {worst:.3e} at (x={}, y={}, ch={})",
            px % img.width(),
            px / img.width(),
            at % 3
        );
    }

    // ── Standalone grain legs (the A2.3 grain_case pattern) ──────────
    // Isolates the KERNEL at this geometry from the composition: the
    // same GrainField, the same gpu_params pack, a synthetic density
    // plane, CPU `GrainField::apply` vs GPU `run_grain` — at the A2.3
    // geometry (256×192, exactly-representable mmpp = 0.140625) AND the
    // battery geometry (112×84, INEXACT mmpp = 36/112) for the mono
    // stock, plus a warm control at 112×84.
    use melography_film::frame::{CropRule, FilmFrame};
    use melography_film::grain::GrainField;
    use melography_film::optics::Plane;
    use melography_gpu::spatial::run_grain;

    fn density_plane(w: u32, h: u32) -> Plane {
        let mut p = Plane::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w.max(1) - 1);
                let fy = f64::from(y) / f64::from(h.max(1) - 1);
                let disc =
                    (-(((fx - 0.6) * (fx - 0.6) + (fy - 0.35) * (fy - 0.35)) / 0.01).exp() * 3.0)
                        .exp();
                let v = (0.02 + 2.2 * fx + 0.4 * (1.0 - fy) + 1.2 * disc).min(3.0);
                p.set3(x, y, [v * 1.03, v, v * 0.97]);
            }
        }
        p
    }

    fn grain_leg(name: &str, stock: &melography_film::profile::StockProfile, w: u32, h: u32) {
        let field = GrainField::resolve(stock, 1.0, None);
        let frame = FilmFrame::from_dims(w, h, stock.crop_rule);
        let src = density_plane(w, h);
        let mut cpu = Plane::new(w, h);
        field.apply(&src, &mut cpu, 0, h, &frame, 0, h);
        let params = field.gpu_params(&frame, w, h);
        let gpu = run_grain(&params, &src.data).expect("gpu grain dispatch");
        let mut worst = 0.0f64;
        let mut worst_at = 0usize;
        for (idx, (c, g)) in cpu.data.iter().zip(&gpu).enumerate() {
            let d = (f64::from(*c) - f64::from(*g)).abs();
            if d > worst {
                worst = d;
                worst_at = idx;
            }
        }
        eprintln!(
            "{name}: standalone grain worst |Δ| = {worst:.3e} at sample {worst_at} (k={}, mmpp={:.6})",
            params.k, frame.mm_per_pixel
        );
    }

    let warm = reg.by_id("warm-400-negative").expect("warm stock");
    grain_leg("standalone/mono@256x192", stock, 256, 192);
    grain_leg("standalone/mono@112x84", stock, 112, 84);
    grain_leg("standalone/warm@112x84", warm, 112, 84);
    grain_leg("standalone/mono@128x96", stock, 128, 96);
    let _ = CropRule::FitWidth; // (the LUT/leg imports stay used if legs change)

    // ── Leg A: the composition with grain via the STANDALONE runner ──
    // The standalone legs above pass at 1e-6 while the composition
    // diverges at 3.9e-2 — the only mechanical differences are (a) the
    // persistent-context dispatch (`run_grain_on`) vs the fresh-acquire
    // `run_grain`, and (b) the grain input values. Leg A swaps (a);
    // Leg B swaps (b). Between them, one must flip.
    {
        use melography_gpu::spatial::run_grain as run_grain_standalone;
        // Rebuild the composition by hand, one dispatch at a time,
        // exactly as gpu_chain::render_gpu does — but grain through the
        // standalone runner.
        use melography_film::chain::ChainStages;
        use melography_film::gpu_bridge::{
            pack_film_pixel_params, pack_vignette_gpu_params, pack_veiling_gpu_params,
        };
        use melography_gpu::film_pixel::run_film_pixel_on;
        use melography_gpu::vignette::run_vignette_on;

        let img = test_image(112, 84, 0.02);
        let lens = LensParams {
            vignette_amount: 0.45,
            veiling_amount: 0.5,
            veiling_sigma_mm: 0.35,
            ..LensParams::default()
        };
        let mut p = params_for(stock, lens);
        p.grain_fast = false;
        let (w, h) = (img.width(), img.height());
        let st = ChainStages::resolve(&p, w, h);
        let mut cur: Vec<f32> = img.data().to_vec();
        let vp = pack_vignette_gpu_params(&st.lens, &st.frame, w, h);
        cur = run_vignette_on(&ctx, &vp, &cur).expect("vignette");
        let veil = pack_veiling_gpu_params(&st.veiling, st.lens.veiling_amount, w, h);
        cur = melography_gpu::spatial::run_halation_on(&ctx, &veil, &cur).expect("veiling");
        let mut fp = pack_film_pixel_params(&st.em, &st.print, &p.adjustments);
        fp.stage = 1;
        cur = run_film_pixel_on(&ctx, &fp, &cur).expect("excite");
        fp.stage = 2;
        cur = run_film_pixel_on(&ctx, &fp, &cur).expect("respond");
        // GRAIN VIA THE STANDALONE RUNNER (fresh acquire):
        let gp = match &st.grain {
            melography_film::grain::GrainStage::Full(f) => f.gpu_params(&st.frame, w, h),
            _ => unreachable!(),
        };
        cur = run_grain_standalone(&gp, &cur).expect("grain standalone");
        fp.stage = 4;
        cur = run_film_pixel_on(&ctx, &fp, &cur).expect("print");
        let cpu = render(&img, &params_for(stock, lens)).expect("cpu render");
        let mut worst = 0.0f64;
        for (c, g) in cpu.data().iter().zip(&cur) {
            worst = worst.max((f64::from(*c) - f64::from(*g)).abs());
        }
        eprintln!("legA/composition-grain-standalone: worst |Δ| = {worst:.3e}");
    }

    // ── Leg B: standalone grain fed the REAL respond output ─────────
    // The CPU stages run manually (all pub) to the response output,
    // then GrainField::apply (CPU, f64 plane) vs run_grain (GPU, the
    // same values as f32) — isolates the input-values question from
    // the composition entirely.
    {
        use melography_film::chain::ChainStages;
        use melography_film::emulsion::{apply_excitation_inplace, apply_response_inplace};
        use melography_film::grain::GrainField;
        use melography_film::optics::{apply_vignette_inplace, Plane};
        use melography_gpu::spatial::run_grain;

        let img = test_image(112, 84, 0.02);
        let lens = LensParams {
            vignette_amount: 0.45,
            veiling_amount: 0.5,
            veiling_sigma_mm: 0.35,
            ..LensParams::default()
        };
        let mut p = params_for(stock, lens);
        p.grain_fast = false;
        let (w, h) = (img.width(), img.height());
        let st = ChainStages::resolve(&p, w, h);
        // CPU stages to the response output on a full-image plane.
        let mut plane = Plane::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let px = img.pixel(x, y);
                plane.set3(x, y, [f64::from(px[0]), f64::from(px[1]), f64::from(px[2])]);
            }
        }
        apply_vignette_inplace(&mut plane, 0, h, 0, &st.lens, &st.frame);
        // (veiling skipped in Leg B — the input domain question is the
        // response output's VALUES, and vignette already varies them)
        apply_excitation_inplace(&mut plane, 0, h, &st.em, p.adjustments.ev, p.adjustments.pre_desaturate);
        apply_response_inplace(&mut plane, 0, h, &st.em);
        let field = match &st.grain {
            melography_film::grain::GrainStage::Full(f) => f,
            _ => unreachable!(),
        };
        let mut cpu_g = Plane::new(w, h);
        field.apply(&plane, &mut cpu_g, 0, h, &st.frame, 0, h);
        let gp = field.gpu_params(&st.frame, w, h);
        let src_f32: Vec<f32> = plane.data.iter().map(|v| *v as f32).collect();
        let gpu_g = run_grain(&gp, &src_f32).expect("grain on respond output");
        let mut worst = 0.0f64;
        let mut worst_at = 0usize;
        for (idx, (c, g)) in cpu_g.data.iter().zip(&gpu_g).enumerate() {
            let d = (f64::from(*c) - f64::from(*g)).abs();
            if d > worst {
                worst = d;
                worst_at = idx;
            }
        }
        // Report the density stats too (the σ(D) input domain).
        let mut dmin = f64::MAX;
        let mut dmax = f64::MIN;
        for v in &plane.data {
            dmin = dmin.min(*v);
            dmax = dmax.max(*v);
        }
        eprintln!(
            "legB/standalone-grain-on-respond-output: worst |Δ| = {worst:.3e} at sample {worst_at} (density range {dmin:.4}..{dmax:.4})"
        );
    }
}

#[test]
fn full_chain_parity_stage_isolation() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here");
        return;
    };
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    let stock = reg.by_id("warm-400-negative").expect("warm stock");
    let img = test_image(96, 72, 0.03);

    // Each spatial stage disabled in turn (and all off): isolates its
    // contribution and proves the skip paths match the CPU skips.
    let cases: &[(&str, f64, f64, f64)] = &[
        ("vignette-off", 0.0, 0.5, 1.0),
        ("veiling-off", 0.45, 0.0, 1.0),
        ("grain-off", 0.45, 0.5, 0.0),
        ("all-spatial-off", 0.0, 0.0, 0.0),
    ];
    for (name, vign, veil, grain) in cases {
        let lens = LensParams {
            vignette_amount: *vign,
            veiling_amount: *veil,
            veiling_sigma_mm: 0.35,
            ..LensParams::default()
        };
        let mut params = params_for(stock, lens);
        params.adjustments.grain_intensity = *grain;
        chain_case(name, &params, &img, &ctx);
    }
}

#[test]
fn full_chain_parity_supersampling_regimes() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here");
        return;
    };
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    let stock = reg.by_id("warm-400-negative").expect("warm stock");
    // Tall FitHeight images pin the k rule: 48×2000 → 12 µm/px (k=2),
    // 24×4000 → 6 µm/px (k=1, native grain scale). Veiling stays OFF
    // here: at 6 µm/px its σ = 58 px (radius 175) convolution over a
    // tall image is minutes on a software adapter — the k regime is the
    // grain question, and veiling parity is covered at preview scale.
    let tall_lens = LensParams {
        vignette_amount: 0.45,
        veiling_amount: 0.0,
        ..LensParams::default()
    };
    for (name, w, h) in [("k2-mid", 48u32, 2000u32), ("k1-native", 24, 4000)] {
        let img = test_image(w, h, 0.02);
        let params = params_for(stock, tall_lens);
        chain_case(name, &params, &img, &ctx);
    }
}

#[test]
fn gpu_chain_determinism() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here");
        return;
    };
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    let stock = reg.by_id("cool-800-night").expect("cool stock");
    let img = test_image(96, 72, 0.05);
    let params = params_for(stock, all_on_lens());
    let a = render_gpu(&img, &params, &ctx).expect("run 1");
    let b = render_gpu(&img, &params, &ctx).expect("run 2");
    assert_eq!(
        a.data(),
        b.data(),
        "two identical GPU compositions must be bit-equal"
    );
}

#[test]
fn gpu_chain_disabled_film_is_identity() {
    let Some(ctx) = ctx() else {
        eprintln!("SKIP: no GPU adapter here");
        return;
    };
    let reg =
        melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
    let stock = reg.by_id("classic-monochrome-400").expect("mono stock");
    let mut params = params_for(stock, all_on_lens());
    params.adjustments.enabled = false;
    let img = test_image(64, 48, 0.0);
    let out = render_gpu(&img, &params, &ctx).expect("identity render");
    assert_eq!(
        out.data(),
        img.data(),
        "disabled film must be the identity (CPU contract)"
    );
}
