//! Spatial-kernel CPU↔GPU parity gates (A2.3: FILM_MODEL §G grain gather
//! + §C2 halation).
//!
//! CPU reference: the **real** film-chain stages — `GrainField::apply`
//! (full model, monolithic full-image strip) and `apply_halation` with
//! the same resolved `Emulsion`. GPU: `run_grain` / `run_halation` on
//! the same input plane with parameters packed from the same resolved
//! stages.
//!
//! Gate: max |Δ| ≤ 1e-3 (density / exposure units) across every case.
//! Coverage: all shipped stocks (color + mono layouts, distinct ρ
//! matrices, σ(D) curves and seeds), the k ∈ {1, 2, 4} supersampling
//! regimes (native / 12 µm / preview scale via FitHeight tall images),
//! a σ(D) = 0 dark region (cells skipped), grain disabled
//! (pass-through), halation attenuated / off / preview-scale kernels,
//! and dispatch determinism (same input twice → identical bytes).
//! Adapter-less environments print an honest skip note (the
//! film_pixel_parity pattern; CI macos/windows run for real).

#![cfg(feature = "wgpu")]

use melography_film::emulsion::{apply_halation, resolve_emulsion, Adjustments};
use melography_film::frame::{CropRule, FilmFrame};
use melography_film::gpu_bridge::pack_halation_gpu_params;
use melography_film::grain::GrainField;
use melography_film::optics::Plane;
use melography_film::profile::StockProfile;
use melography_gpu::spatial::{first_adapter_report, run_grain, run_halation};

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Density-domain input (post-§D): a gradient over [0.02, 2.6] plus a
/// bright disc reaching ~3.0, per-channel tinted — sweeps σ(D) across
/// its whole curve range and gives the gather a real density field.
fn density_plane(w: u32, h: u32) -> Plane {
    let mut p = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let fy = f64::from(y) / f64::from(h - 1);
            let dx = fx - 0.6;
            let dy = fy - 0.4;
            let disc = (-((dx * dx + dy * dy) / 0.01).exp() * 3.0).exp();
            let v = (0.02 + 2.2 * fx + 0.4 * (1.0 - fy) + 1.2 * disc).min(3.0);
            p.set3(x, y, [v * 1.03, v, v * 0.97]);
        }
    }
    p
}

/// Excitation-domain input (§C1/C3 output): values in [0.01, 3.5].
fn excitation_plane(w: u32, h: u32) -> Plane {
    let mut p = Plane::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let fy = f64::from(y) / f64::from(h - 1);
            let dx = fx - 0.55;
            let dy = fy - 0.45;
            let disc = (-((dx * dx + dy * dy) / 0.008).exp() * 3.0).exp();
            let v = (0.01 + 2.5 * fx + 0.3 * (1.0 - fy) + 1.3 * disc).min(3.5);
            p.set3(x, y, [v * 1.05, v, v * 0.9]);
        }
    }
    p
}

/// One grain parity case: CPU full-model strip vs GPU gather, worst |Δ|.
fn grain_case(
    name: &str,
    profile: &StockProfile,
    intensity: f64,
    frame: &FilmFrame,
    w: u32,
    h: u32,
) -> f64 {
    let field = GrainField::resolve(profile, intensity, None);
    let src = density_plane(w, h);
    let mut cpu = Plane::new(w, h);
    field.apply(&src, &mut cpu, 0, h, frame, 0, h);
    let params = field.gpu_params(frame, w, h);
    let gpu = run_grain(&params, &src.data).expect("gpu grain dispatch");
    let mut worst = 0.0f64;
    for (c, g) in cpu.data.iter().zip(&gpu) {
        worst = worst.max((f64::from(*c) - f64::from(*g)).abs());
    }
    eprintln!("{name}: grain worst |Δ| = {worst:.3e} (k={}, {w}×{h})", params.k);
    assert!(
        worst <= 1e-3,
        "{name}: grain GPU parity gate failed (worst |Δ| = {worst:.3e} > 1e-3)"
    );
    worst
}

/// One halation parity case: CPU separable conv vs GPU h+v, worst |Δ|.
fn halation_case(
    name: &str,
    profile: &StockProfile,
    adj: &Adjustments,
    mm_per_pixel: f64,
    w: u32,
    h: u32,
) -> f64 {
    let em = resolve_emulsion(profile, adj, mm_per_pixel);
    let src = excitation_plane(w, h);
    let mut cpu = Plane::new(w, h);
    apply_halation(&src, &mut cpu, 0, h, &em, (0, h));
    let params = pack_halation_gpu_params(&em, w, h);
    let gpu = run_halation(&params, &src.data).expect("gpu halation dispatch");
    let mut worst = 0.0f64;
    for (c, g) in cpu.data.iter().zip(&gpu) {
        worst = worst.max((f64::from(*c) - f64::from(*g)).abs());
    }
    eprintln!(
        "{name}: halation worst |Δ| = {worst:.3e} (radii {:?}, {w}×{h})",
        params.radius
    );
    assert!(
        worst <= 1e-3,
        "{name}: halation GPU parity gate failed (worst |Δ| = {worst:.3e} > 1e-3)"
    );
    worst
}

#[test]
fn grain_gpu_parity() {
    let adapter = match first_adapter_report() {
        Some(a) => a,
        None => {
            eprintln!("SKIP: no GPU adapter here (CI macos/windows run for real)");
            return;
        }
    };
    eprintln!(
        "adapter: {} ({}, {})",
        adapter.name, adapter.backend, adapter.device_type
    );
    let root = repo_root();
    let reg = melography_presets::StockRegistry::open(&root.join("profiles")).expect("profiles");

    // Main battery: every shipped stock at preview-like supersampling
    // (FitWidth 256×192 → 140 µm/px → k = 4).
    let mut worst_overall = 0.0f64;
    for p in &reg.profiles {
        let frame = FilmFrame::from_dims(256, 192, p.crop_rule);
        worst_overall = worst_overall.max(grain_case(&p.id, p, 1.0, &frame, 256, 192));
    }

    let warm = reg.by_id("warm-400-negative").expect("warm stock");

    // k = 1 (native 6 µm/px): tall FitHeight image.
    let f_native = FilmFrame::from_dims(48, 4000, CropRule::FitHeight);
    worst_overall = worst_overall.max(grain_case(
        "warm-400-negative/k1-native",
        warm,
        1.0,
        &f_native,
        48,
        4000,
    ));

    // k = 2 (12 µm/px).
    let f_mid = FilmFrame::from_dims(96, 2000, CropRule::FitHeight);
    worst_overall = worst_overall.max(grain_case(
        "warm-400-negative/k2-mid",
        warm,
        1.0,
        &f_mid,
        96,
        2000,
    ));

    // σ(D) = 0 dark region: densities below 0.4 skip their cells on
    // BOTH sides (the cache-build `continue` / the shader `continue`).
    let mut dark = warm.clone();
    dark.grain.sigma_curve.density = vec![0.0, 0.4, 1.1, 2.0];
    dark.grain.sigma_curve.sigma = vec![0.0, 0.0, 0.04, 0.02];
    let f_dark = FilmFrame::from_dims(192, 144, CropRule::FitWidth);
    worst_overall = worst_overall.max(grain_case(
        "sigma-zero-dark-region",
        &dark,
        1.0,
        &f_dark,
        192,
        144,
    ));

    // Grain disabled: pass-through on both sides.
    worst_overall = worst_overall.max(grain_case(
        "warm-400-negative/grain-off",
        warm,
        0.0,
        &f_dark,
        192,
        144,
    ));

    // Dispatch determinism: same input twice → identical bytes.
    let field = GrainField::resolve(warm, 1.0, None);
    let src = density_plane(192, 144);
    let params = field.gpu_params(&f_dark, 192, 144);
    let a = run_grain(&params, &src.data).expect("gpu grain run 1");
    let b = run_grain(&params, &src.data).expect("gpu grain run 2");
    assert_eq!(a, b, "GPU grain must be deterministic across dispatches");

    eprintln!(
        "grain parity: worst over all cases = {worst_overall:.3e} (gate ≤ 1e-3)"
    );
}

#[test]
fn halation_gpu_parity() {
    let adapter = match first_adapter_report() {
        Some(a) => a,
        None => {
            eprintln!("SKIP: no GPU adapter here (CI macos/windows run for real)");
            return;
        }
    };
    eprintln!(
        "adapter: {} ({}, {})",
        adapter.name, adapter.backend, adapter.device_type
    );
    let root = repo_root();
    let reg = melography_presets::StockRegistry::open(&root.join("profiles")).expect("profiles");

    // Export-scale kernels (6 µm/px → radii up to ~28 px) on a modest
    // plane — halation is a pixel-space conv, so the resolve scale and
    // the plane size are independent.
    let adj = Adjustments::default();
    let mut worst_overall = 0.0f64;
    for p in &reg.profiles {
        worst_overall = worst_overall.max(halation_case(&p.id, p, &adj, 0.006, 320, 240));
    }

    let warm = reg.by_id("warm-400-negative").expect("warm stock");

    // Attenuated variant.
    let half = Adjustments { halation_intensity: 0.5, ..Adjustments::default() };
    worst_overall = worst_overall.max(halation_case(
        "warm-400-negative/intensity-0.5",
        warm,
        &half,
        0.006,
        320,
        240,
    ));

    // Preview-scale kernels (2048-wide FitWidth → 17.6 µm/px, radius 1–3).
    worst_overall = worst_overall.max(halation_case(
        "warm-400-negative/preview-scale",
        warm,
        &adj,
        36.0 / 2048.0,
        320,
        240,
    ));

    // Halation off: every channel inactive → pure pass-through.
    let off = Adjustments { halation_intensity: 0.0, ..Adjustments::default() };
    worst_overall = worst_overall.max(halation_case(
        "warm-400-negative/halation-off",
        warm,
        &off,
        0.006,
        320,
        240,
    ));

    // Dispatch determinism.
    let em = resolve_emulsion(warm, &adj, 0.006);
    let src = excitation_plane(192, 144);
    let params = pack_halation_gpu_params(&em, 192, 144);
    let a = run_halation(&params, &src.data).expect("gpu halation run 1");
    let b = run_halation(&params, &src.data).expect("gpu halation run 2");
    assert_eq!(a, b, "GPU halation must be deterministic across dispatches");

    eprintln!(
        "halation parity: worst over all cases = {worst_overall:.3e} (gate ≤ 1e-3)"
    );
}
