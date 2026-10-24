//! Film pixel-kernel CPU↔GPU parity gate (FILM_MODEL §C1/C3–F scope).
//!
//! CPU reference: the **real** chain (`melography_film::chain::render`)
//! with the spatial stages disabled (lens off, halation 0, grain 0) —
//! exactly the per-pixel scope of the WGSL kernel. GPU: `run_film_pixel`
//! on the same inputs with parameters packed from the same resolved
//! `Emulsion`/`Print`.
//!
//! Gate: max |Δ| ≤ 1e-3 (linear) per channel across a 657-sample battery
//! spanning the exposure range for every shipped stock profile — the
//! profiles cover negative/reversal polarities, color/mono layouts, and
//! all three inversion paths, so the kernel's full branch surface is
//! exercised (issue #10 GPU leg; adapter-less environments print an
//! honest skip note, matching the M1 lut3d parity pattern).

#![cfg(feature = "wgpu")]

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render, FilmChainParams};
use melography_film::emulsion::{resolve_emulsion, Adjustments};
use melography_film::gpu_bridge::pack_film_pixel_params;
use melography_film::print::Print;
use melography_film::profile::InversionPath;

fn battery() -> Vec<f32> {
    // 657 exposures spanning the working range, per channel offset so the
    // layers traverse different curve segments.
    let n = 219;
    let mut v = Vec::with_capacity(n * 3);
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let e = 10f64.powf(-3.6 + 3.8 * t);
        v.push((e * 1.08) as f32);
        v.push((e * 0.96) as f32);
        v.push((e * 0.82) as f32);
    }
    v
}

#[test]
fn film_pixel_gpu_parity() {
    let adapter = match melography_gpu::film_pixel::first_adapter_report() {
        Some(a) => a,
        None => {
            eprintln!("SKIP: no GPU adapter in this environment (CI macos/windows run for real)");
            return;
        }
    };
    eprintln!(
        "adapter: {} ({}, {})",
        adapter.name, adapter.backend, adapter.device_type
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let reg =
        melography_presets::StockRegistry::open(&root.join("profiles")).expect("shipped profiles");
    let inputs = battery();

    // Cases: each shipped stock (its default path) + explicit path
    // overrides to cover all three inversion branches.
    let mut cases: Vec<(String, melography_film::StockProfile, Option<InversionPath>)> = Vec::new();
    for p in &reg.profiles {
        cases.push((p.id.clone(), p.clone(), None));
    }
    let warm = reg.by_id("warm-400-negative").unwrap().clone();
    for path in [InversionPath::Scanner, InversionPath::DirectScan] {
        cases.push((
            format!("warm-400-negative/{path:?}"),
            warm.clone(),
            Some(path),
        ));
    }

    let mut worst_overall = 0.0f64;
    for (name, profile, path_override) in cases {
        // CPU reference: real chain, spatial stages off.
        let adj = Adjustments {
            halation_intensity: 0.0,
            grain_intensity: 0.0,
            inversion_path: path_override,
            ..Adjustments::default()
        };
        let params = FilmChainParams::new(profile.clone())
            .unwrap()
            .with_adjustments(adj.clone(), Default::default())
            .unwrap();
        let mut img = ImageF32::alloc(219, 1, Channels::Rgb, 0.0);
        img.data_mut().copy_from_slice(&inputs);
        let cpu = render(&img, &params).expect("cpu render");

        // GPU: same resolved parameters through the packing bridge.
        let em = resolve_emulsion(&profile, &adj, 36.0 / 219.0);
        let print = Print::resolve(&profile, path_override, 0.0);
        let gpu_params = pack_film_pixel_params(&em, &print, &adj);
        let gpu =
            melography_gpu::film_pixel::run_film_pixel(&gpu_params, &inputs).expect("gpu dispatch");

        let mut worst = 0.0f64;
        for (c, g) in cpu.data().iter().zip(&gpu) {
            worst = worst.max((f64::from(*c) - f64::from(*g)).abs());
        }
        eprintln!("{name}: worst |Δ| = {worst:.3e}");
        assert!(
            worst <= 1e-3,
            "{name}: GPU parity gate failed (worst |Δ| = {worst:.3e} > 1e-3)"
        );
        worst_overall = worst_overall.max(worst);
    }
    eprintln!("film pixel parity: worst over all stocks = {worst_overall:.3e} (gate ≤ 1e-3)");
}
