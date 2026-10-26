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
