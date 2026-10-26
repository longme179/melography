//! A2.3 GPU spatial-kernel perf evidence (PLAN §4 re-pin; the owner's
//! machine is the reference — CI adapters are correctness gates, not a
//! perf reference).
//!
//! `#[ignore]`d: run in release —
//! `cargo test --release -p melography-gpu --test spatial_perf -- --ignored --nocapture`
//!
//! Measures, per shipped stock, the wall time of one `run_grain` /
//! `run_halation` dispatch (fresh device per call — the harness
//! pattern; the tiny-image baseline line isolates that fixed overhead,
//! the app-wiring step owns the persistent context).

#![cfg(feature = "wgpu")]

use melography_film::emulsion::{resolve_emulsion, Adjustments};
use melography_film::frame::FilmFrame;
use melography_film::gpu_bridge::pack_halation_gpu_params;
use melography_film::grain::GrainField;
use melography_film::optics::Plane;
use melography_gpu::spatial::{first_adapter_report, run_grain, run_halation};
use std::time::Instant;

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn density_plane(w: u32, h: u32) -> Plane {
    let mut p = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let fy = f64::from(y) / f64::from(h - 1);
            let v = (0.02 + 2.2 * fx + 0.4 * (1.0 - fy)).min(3.0);
            p.set3(x, y, [v * 1.03, v, v * 0.97]);
        }
    }
    p
}

fn excitation_plane(w: u32, h: u32) -> Plane {
    let mut p = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let v = (0.01 + 2.5 * fx).min(3.5);
            p.set3(x, y, [v * 1.05, v, v * 0.9]);
        }
    }
    p
}

#[test]
#[ignore = "perf evidence — run in release with --ignored --nocapture"]
fn gpu_spatial_perf() {
    let adapter = match first_adapter_report() {
        Some(a) => a,
        None => {
            eprintln!("SKIP: no GPU adapter in this environment");
            return;
        }
    };
    eprintln!(
        "adapter: {} ({}, {})",
        adapter.name, adapter.backend, adapter.device_type
    );
    println!(
        "machine: {} cores",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    );
    let root = repo_root();
    let reg = melography_presets::StockRegistry::open(&root.join("profiles")).expect("profiles");

    // Fixed overhead baseline: fresh device acquisition + tiny dispatch.
    {
        let p = &reg.profiles[0];
        let frame = FilmFrame::from_dims(8, 8, p.crop_rule);
        let field = GrainField::resolve(p, 1.0, None);
        let src = density_plane(8, 8);
        let params = field.gpu_params(&frame, 8, 8);
        let t = Instant::now();
        let _ = run_grain(&params, &src.data).unwrap();
        println!(
            "baseline-8x8-grain: {:.1} ms (device acquire + dispatch)",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }

    // ── Preview scenario: 2048×1536 ─────────────────────────────────
    let (pw, ph) = (2048u32, 1536u32);
    for p in &reg.profiles {
        let frame = FilmFrame::from_dims(pw, ph, p.crop_rule);
        let field = GrainField::resolve(p, 1.0, None);
        let src = density_plane(pw, ph);
        let params = field.gpu_params(&frame, pw, ph);
        let _ = run_grain(&params, &src.data).unwrap(); // warmup
        let mut times = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let _ = run_grain(&params, &src.data).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "gpu-grain-2048x1536[{}]: p50 {:.1} ms · max {:.1} ms (n=5, k={})",
            p.id,
            times[times.len() / 2],
            times[times.len() - 1],
            params.k
        );
    }

    // ── Export scenario: 6000×4000 (k = 1) ─────────────────────────
    let (ew, eh) = (6000u32, 4000u32);
    for p in &reg.profiles {
        let frame = FilmFrame::from_dims(ew, eh, p.crop_rule);
        let field = GrainField::resolve(p, 1.0, None);
        let src = density_plane(ew, eh);
        let params = field.gpu_params(&frame, ew, eh);
        let t = Instant::now();
        let _ = run_grain(&params, &src.data).unwrap();
        println!(
            "gpu-grain-6000x4000[{}]: {:.1} ms (k={})",
            p.id,
            t.elapsed().as_secs_f64() * 1000.0,
            params.k
        );
    }

    // ── Halation at both scales ────────────────────────────────────
    for p in &reg.profiles {
        let em = resolve_emulsion(p, &Adjustments::default(), 36.0 / 2048.0);
        let src = excitation_plane(pw, ph);
        let params = pack_halation_gpu_params(&em, pw, ph);
        let t = Instant::now();
        let _ = run_halation(&params, &src.data).unwrap();
        println!(
            "gpu-halation-2048x1536[{}]: {:.1} ms (radii {:?})",
            p.id,
            t.elapsed().as_secs_f64() * 1000.0,
            params.radius
        );
    }
    for p in &reg.profiles {
        let em = resolve_emulsion(p, &Adjustments::default(), 36.0 / 6000.0);
        let src = excitation_plane(ew, eh);
        let params = pack_halation_gpu_params(&em, ew, eh);
        let t = Instant::now();
        let _ = run_halation(&params, &src.data).unwrap();
        println!(
            "gpu-halation-6000x4000[{}]: {:.1} ms (radii {:?})",
            p.id,
            t.elapsed().as_secs_f64() * 1000.0,
            params.radius
        );
    }
}
