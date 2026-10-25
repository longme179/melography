//! M2 performance evidence (PLAN §4 re-pin; every number prints the
//! machine context — honest measurements, not marketing).
//!
//! `#[ignore]`d: run in release for the budget table —
//! `cargo test --release -p melography-film --test perf -- --ignored --nocapture`.
//!
//! Measures, per shipped stock:
//! * **preview leg** — film chain at 2048×1536 (fit-to-window size for a
//!   12 MP 4:3 source, the PLAN §4 preview scenario) — p50/p95 over runs;
//! * **export leg** — film chain at 6000×4000 (24 MP target) — wall time;
//! * **resolve cost** — emulsion/print/grain-field resolution incl. the
//!   grain self-calibration (per parameter change, cached in the app).

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render, render_threaded, FilmChainParams};
use melography_film::emulsion::resolve_emulsion;
use melography_film::grain::{FastGrain, GrainField};
use melography_film::print::Print;
use melography_film::Adjustments;
use std::time::Instant;

fn synthetic(w: u32, h: u32) -> ImageF32 {
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let fy = f64::from(y) / f64::from(h - 1);
            let dx = fx - 0.6;
            let dy = fy - 0.4;
            let disc = (-((dx * dx + dy * dy) / 0.01).exp() * 3.0).exp();
            let v = (0.03 + 0.5 * fx + 0.3 * (1.0 - fy) + 1.5 * disc).min(3.0);
            let p = img.pixel_mut(x, y);
            p[0] = (v * 1.04) as f32;
            p[1] = v as f32;
            p[2] = (v * 0.92) as f32;
        }
    }
    img
}

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
#[ignore = "perf evidence — run in release with --ignored --nocapture"]
fn perf_preview_and_export_budgets() {
    let reg = melography_presets::StockRegistry::open(&repo_root().join("profiles")).unwrap();
    println!(
        "machine: {} cores",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    );

    // ── resolve cost (per parameter change in the app) ─────────────────
    for p in &reg.profiles {
        let t = Instant::now();
        let em = resolve_emulsion(p, &Adjustments::default(), 36.0 / 2048.0);
        let grain = GrainField::resolve(p, 1.0, None);
        let print = Print::resolve(p, None, 0.0);
        let _ = (&em, &print, grain.enabled);
        println!(
            "resolve[{}]: {:.1} ms (incl. grain self-calibration)",
            p.id,
            t.elapsed().as_secs_f64() * 1000.0
        );
        let t = Instant::now();
        let fast = FastGrain::resolve(p, 1.0, None);
        let _ = fast.enabled;
        println!(
            "resolve-fast[{}]: {:.1} ms (A2.2 fast self-calibration)",
            p.id,
            t.elapsed().as_secs_f64() * 1000.0
        );
    }

    // ── preview leg: 2048×1536 ───────────────────────────────────────────
    let preview_img = synthetic(2048, 1536);
    let runs = 7;
    for p in &reg.profiles {
        let params = FilmChainParams::new(p.clone()).unwrap();
        // warmup
        let _ = render(&preview_img, &params).unwrap();
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let _ = render(&preview_img, &params).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = times[times.len() / 2];
        let p95 = times[times.len() - 1];
        println!(
            "preview-2048x1536[{}]: p50 {p50:.0} ms · p95 {p95:.0} ms (n={runs})",
            p.id
        );
    }

    // ── preview leg, threaded (A2.1): same scenario via render_threaded ─
    // Output is byte-identical to the sequential leg (the chain tests
    // assert it); this leg is the speedup evidence for the PLAN §4
    // re-pin. workers=0 → one chunk per logical core (the count printed
    // in the machine line above).
    for p in &reg.profiles {
        let params = FilmChainParams::new(p.clone()).unwrap();
        // warmup
        let _ = render_threaded(&preview_img, &params, 0).unwrap();
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let _ = render_threaded(&preview_img, &params, 0).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = times[times.len() / 2];
        let p95 = times[times.len() - 1];
        println!(
            "preview-threaded-2048x1536[{}]: p50 {p50:.0} ms · p95 {p95:.0} ms (n={runs})",
            p.id
        );
    }

    // ── preview leg, fast grain (A2.2): same scenario, grain fast path ─
    // Output is byte-identical across worker counts (chain tests assert
    // it); this leg is the A2.2 speedup evidence for the PLAN §4 re-pin.
    for p in &reg.profiles {
        let params = FilmChainParams::new(p.clone())
            .unwrap()
            .with_grain_fast(true);
        // warmup
        let _ = render_threaded(&preview_img, &params, 0).unwrap();
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let t = Instant::now();
            let _ = render_threaded(&preview_img, &params, 0).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = times[times.len() / 2];
        let p95 = times[times.len() - 1];
        println!(
            "preview-fast-threaded-2048x1536[{}]: p50 {p50:.0} ms · p95 {p95:.0} ms (n={runs})",
            p.id
        );
    }

    // ── export leg: 6000×4000 (24 MP) ───────────────────────────────────
    let export_img = synthetic(6000, 4000);
    for p in &reg.profiles {
        let params = FilmChainParams::new(p.clone()).unwrap();
        let t = Instant::now();
        let out = render(&export_img, &params).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        // sanity: output non-trivial
        let max = out.data().iter().cloned().fold(0.0f32, f32::max);
        println!("export-6000x4000[{}]: {ms:.0} ms (max {max:.3})", p.id);
    }
}
