//! A2.4 full-chain GPU preview perf evidence (PLAN §4 re-pin; the
//! owner's machine is the reference — CI adapters are correctness
//! gates, not a perf reference).
//!
//! `#[ignore]`d: run in release —
//! `cargo test --release -p melography-film --features gpu-chain --test gpu_chain_perf -- --ignored --nocapture`
//!
//! Measures, per shipped stock, the wall time of one full
//! `gpu_chain::render_gpu` composition over the persistent context
//! (the app's preview path: one context, cached pipelines, ping-pong
//! buffers) at preview scale (2048×1536) and export scale (6000×4000),
//! plus the one-shot context-init cost (the cold-start line the first
//! preview pays) and the CPU threaded fallback for comparison on the
//! same image.

#![cfg(feature = "gpu-chain")]

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render_threaded, FilmChainParams};
use melography_film::gpu_chain::render_gpu;
use melography_film::optics::LensParams;
use melography_gpu::context::AdapterPolicy;
use melography_gpu::GpuError;
use std::sync::Arc;
use std::time::Instant;

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn test_image(w: u32, h: u32) -> ImageF32 {
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    let data = img.data_mut();
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w.max(1) - 1);
            let fy = f64::from(y) / f64::from(h.max(1) - 1);
            let disc =
                (-(((fx - 0.6) * (fx - 0.6) + (fy - 0.35) * (fy - 0.35)) / 0.01).exp() * 4.0).exp();
            let v = (0.05 + 0.7 * fx + 0.25 * (1.0 - fy) + 0.5 * disc).min(1.6);
            let i = ((y * w + x) * 3) as usize;
            data[i] = (v * 1.04) as f32;
            data[i + 1] = v as f32;
            data[i + 2] = (v * 0.96) as f32;
        }
    }
    img
}

fn all_on_lens() -> LensParams {
    LensParams {
        vignette_amount: 0.45,
        veiling_amount: 0.5,
        veiling_sigma_mm: 0.35,
        ..LensParams::default()
    }
}

#[test]
#[ignore = "perf evidence — run in release with --ignored --nocapture"]
fn gpu_full_chain_perf() {
    // Cold-start line first: the one-shot context init (instance +
    // adapter + device + every pipeline compiled) — the price of the
    // FIRST preview after launch.
    let t0 = Instant::now();
    let ctx: Arc<_> = match melography_gpu::context::init(&AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter in this environment");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
    };
    println!(
        "context-init (cold, once): {:.1} ms",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    println!(
        "adapter: {} ({}, {})",
        ctx.report.name, ctx.report.backend, ctx.report.device_type
    );
    println!(
        "machine: {} cores",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    );

    let reg = melography_presets::StockRegistry::open(&repo_root().join("profiles"))
        .expect("profiles");

    for (w, h, label) in [
        (2048u32, 1536u32, "preview-2048x1536"),
        (6000, 4000, "export-6000x4000"),
    ] {
        let img = test_image(w, h);
        for stock in &reg.profiles {
            let params = FilmChainParams::new(stock.clone())
                .expect("profile loads")
                .with_adjustments(Default::default(), all_on_lens())
                .expect("defaults validate");
            // GPU composition (persistent context).
            let t = Instant::now();
            let gpu = render_gpu(&img, &params, &ctx).expect("gpu render");
            let gpu_ms = t.elapsed().as_secs_f64() * 1000.0;
            // CPU threaded fallback (the app's fallback leg), fast grain
            // — same as the preview fallback the app would run.
            let mut cpu_params = params.clone();
            cpu_params.grain_fast = true;
            let t = Instant::now();
            let cpu = render_threaded(&img, &cpu_params, 0).expect("cpu render");
            let cpu_ms = t.elapsed().as_secs_f64() * 1000.0;
            // Honest sanity: same domain, bounded difference.
            let mut worst = 0.0f64;
            for (g, c) in gpu.data().iter().zip(cpu.data()) {
                worst = worst.max((f64::from(*g) - f64::from(*c)).abs());
            }
            println!(
                "{label} {}: gpu {gpu_ms:.0} ms | cpu-threaded-fast {cpu_ms:.0} ms | |Δ|gpu-cpu {worst:.3e}"
                , stock.id
            );
        }
    }
}
