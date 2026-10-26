//! A2.4 — full-chain GPU film preview: the WGSL composition.
//!
//! This is the app-wiring step the A2.3 print left open: one persistent
//! context ([`melography_gpu::context::GpuContext`]), every kernel
//! dispatched over ping-pong buffers, in EXACTLY the CPU strip order —
//!
//! ```text
//! §B  vignette      (WGSL, per-column/row offset LUTs)
//! §B  veiling       (halation h+v dispatch in blend mode)
//! §A5/§C1/C3 excitation  (film_pixel stage 1)
//! §C2 halation      (halation h+v dispatch, additive)
//! §D/§E response    (film_pixel stage 2)
//! §G  grain         (the FULL model gather — not the A2.2 fast path:
//!                    GPU grain is full-quality at preview speed; the
//!                    fast field stays the no-adapter CPU preview leg)
//! §F  print         (film_pixel stage 3)
//! ```
//!
//! The film_pixel stages run as three stage-flagged dispatches of the
//! SAME shader (bit 0/1/2 of flags>>4) — the fused math functions
//! unchanged, and f32 buffer round-trips are lossless, so
//! split ≡ fused holds bit-exact by construction (gated). The CPU
//! monolithic `render` stays the reference path everywhere; export and
//! the CLI never touch this module.
//!
//! Parity contract: `gpu_chain_parity` gates the composition against
//! the CPU chain (all stages on) at ≤ 2e-3 max |Δ| — each constituent
//! kernel holds ≤ 1e-3 on its own; the composition budget doubles that
//! for cross-stage accumulation through the nonlinear response (every
//! number reported, never hidden).

use crate::chain::{ChainStages, FilmChainParams};
use crate::gpu_bridge::{
    pack_film_pixel_params, pack_halation_gpu_params, pack_veiling_gpu_params,
    pack_vignette_gpu_params,
};
use melography_core::{Channels, ImageF32};
use melography_gpu::context::GpuContext;
use melography_gpu::film_pixel_types::FilmPixelGpuParams;
use melography_gpu::spatial_types::HalationGpuParams;
use melography_gpu::{film_pixel, spatial, vignette, GpuError};

/// Render the full film chain on the GPU (preview leg). Mirrors
/// `chain::render` semantics: RGB-only input, `adjustments.enabled ==
/// false` is the identity (same early-out as the CPU), and the CPU
/// reference stays authoritative — this path exists for the preview
/// budget, never for export.
pub fn render_gpu(
    img: &ImageF32,
    params: &FilmChainParams,
    ctx: &GpuContext,
) -> Result<ImageF32, GpuError> {
    if img.channels() != Channels::Rgb {
        return Err(GpuError::Wgpu(format!(
            "gpu chain requires RGB input (got {:?})",
            img.channels()
        )));
    }
    if !params.adjustments.enabled {
        return Ok(img.clone());
    }
    // The GPU leg ALWAYS renders the full grain model — `grain_fast`
    // governs the CPU preview fallback only (the A2.3 quality contract:
    // full-quality grain at preview speed).
    let mut p = params.clone();
    p.grain_fast = false;
    let (w, h) = (img.width(), img.height());
    let st = ChainStages::resolve(&p, w, h);

    let mut cur: Vec<f32> = img.data().to_vec();

    // -- §B vignette (k ≤ 0 is the exact identity on both sides: skip) --
    let k = 4.0 * st.lens.vignette_amount;
    if k > 0.0 {
        let vp = pack_vignette_gpu_params(&st.lens, &st.frame, w, h);
        cur = vignette::run_vignette_on(ctx, &vp, &cur)?;
    }

    // -- §B veiling bloom (blend-mode h+v over the vignetted plane) ----
    if st.veiling_on {
        let vp = pack_veiling_gpu_params(&st.veiling, st.lens.veiling_amount, w, h);
        cur = spatial::run_halation_on(ctx, &vp, &cur)?;
    }

    // -- §A5/§C1/C3 excitation (film_pixel stage 1) --------------------
    let mut stage_pack: FilmPixelGpuParams =
        pack_film_pixel_params(&st.em, &st.print, &p.adjustments);
    stage_pack.stage = 1;
    cur = film_pixel::run_film_pixel_on(ctx, &stage_pack, &cur)?;

    // -- §C2 halation (additive h+v; all-inactive packs skip) ----------
    let hp: HalationGpuParams = pack_halation_gpu_params(&st.em, w, h);
    if hp.radius.iter().any(|r| *r > 0) {
        cur = spatial::run_halation_on(ctx, &hp, &cur)?;
    }

    // -- §D/§E response (film_pixel stage 2) ---------------------------
    stage_pack.stage = 2;
    cur = film_pixel::run_film_pixel_on(ctx, &stage_pack, &cur)?;

    // -- §G grain (the FULL §G model gather) ---------------------------
    let gp = match &st.grain {
        crate::grain::GrainStage::Full(f) => f.gpu_params(&st.frame, w, h),
        crate::grain::GrainStage::Fast(_) => {
            // Unreachable: grain_fast was forced false above — but a
            // panic in the render path is never the answer; fail loudly
            // to the caller (it falls back to the CPU leg).
            return Err(GpuError::Wgpu(
                "gpu chain: fast-grain stage resolved despite grain_fast = false".into(),
            ));
        }
    };
    if gp.enabled {
        cur = spatial::run_grain_on(ctx, &gp, &cur)?;
    }

    // -- §F print (film_pixel stage 3) ---------------------------------
    stage_pack.stage = 4;
    cur = film_pixel::run_film_pixel_on(ctx, &stage_pack, &cur)?;

    let mut out = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    out.data_mut().copy_from_slice(&cur);
    Ok(out)
}

#[cfg(all(test, feature = "gpu-chain"))]
mod tests {
    use super::*;

    fn test_image(w: u32, h: u32, tint: f64) -> ImageF32 {
        let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
        let data = img.data_mut();
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w.max(1) - 1);
                let fy = f64::from(y) / f64::from(h.max(1) - 1);
                let disc = (-(((fx - 0.6) * (fx - 0.6) + (fy - 0.35) * (fy - 0.35)) / 0.01).exp()
                    * 4.0)
                    .exp();
                let v = (0.05 + 0.7 * fx + 0.25 * (1.0 - fy) + 0.5 * disc).min(1.6);
                let i = ((y * w + x) * 3) as usize;
                data[i] = (v * (1.0 + tint)) as f32;
                data[i + 1] = v as f32;
                data[i + 2] = (v * (1.0 - tint)) as f32;
            }
        }
        img
    }

    /// The split dispatches (stage 1 / 2 / 4 through this module, spatial
    /// stages disabled) and the standalone fused dispatch (stage 7) use
    /// the SAME shader functions, and f32 buffer round-trips are
    /// lossless — so the two paths must agree BIT-EXACTLY. This pins the
    /// stage-split refactor itself: a divergence is a stage-boundary
    /// bug, not a tolerance matter. (Lives here, not in tests/, because
    /// it reaches the `pub(crate)` ChainStages resolve.)
    #[test]
    fn stage_split_matches_fused_bit_exact() {
        let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto)
        {
            Ok(c) => c,
            Err(GpuError::NoAdapter) => {
                eprintln!("SKIP: no GPU adapter here (CI macos/windows run for real)");
                return;
            }
            Err(e) => panic!("gpu context init failed: {e}"),
        };
        // The in-crate fixture profile (NOT the presets registry): the
        // self-dev-dependency that turns `gpu-chain` on for tests puts a
        // SECOND copy of this crate in the dev graph, and mixing
        // `crate::` types with registry-loaded profiles crosses the two
        // copies (E0308). The split≡fused equivalence is stock-agnostic
        // pixel math; the all-stocks parity battery lives in
        // tests/gpu_chain_parity.rs against the real registry.
        let stock = crate::profile::testing::test_profile();
        let img = test_image(96, 72, 0.03);
        // Spatial stages off keeps the split path pure pixel math.
        let lens = crate::optics::LensParams {
            vignette_amount: 0.0,
            veiling_amount: 0.0,
            ..crate::optics::LensParams::default()
        };
        let mut params = crate::chain::FilmChainParams::new(stock.clone())
            .expect("profile loads")
            .with_adjustments(Default::default(), lens)
            .expect("defaults validate");
        params.adjustments.grain_intensity = 0.0;
        params.adjustments.halation_intensity = 0.0;

        let gpu_split = render_gpu(&img, &params, &ctx).expect("split composition");

        // Fused reference: the standalone film_pixel leg, stage = 7.
        let mut p = params.clone();
        p.grain_fast = false;
        let st = crate::chain::ChainStages::resolve(&p, img.width(), img.height());
        let mut fp = crate::gpu_bridge::pack_film_pixel_params(&st.em, &st.print, &p.adjustments);
        fp.stage = 7;
        let fused = melography_gpu::film_pixel::run_film_pixel_on(&ctx, &fp, img.data())
            .expect("fused dispatch");

        assert_eq!(
            gpu_split.data(),
            fused,
            "split composition must be bit-equal to the fused dispatch (spatial off)"
        );
    }

    #[test]
/// Mono-specific isolation (the round-4/6 finding: classic-monochrome-400
/// diverges while every warm case sits at 1e-7 — each spatial stage off
/// for the mono stock pins WHICH stage carries the mono delta; this leg
/// only REPORTS, the battery above owns the verdict).
#[test]
fn mono_diagnostics_stage_isolation_and_legs() {
    let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(melography_gpu::GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter here");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
    };

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

    fn test_image(w: u32, h: u32, tint: f64) -> melography_core::ImageF32 {
        let mut img = melography_core::ImageF32::alloc(w, h, melography_core::Channels::Rgb, 0.0);
        let data = img.data_mut();
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w.max(1) - 1);
                let fy = f64::from(y) / f64::from(h.max(1) - 1);
                let disc =
                    (-(((fx - 0.6) * (fx - 0.6) + (fy - 0.35) * (fy - 0.35)) / 0.01).exp() * 4.0)
                        .exp();
                let v = (0.05 + 0.7 * fx + 0.25 * (1.0 - fy) + 0.5 * disc).min(1.6);
                let i = ((y * w + x) * 3) as usize;
                data[i] = (v * (1.0 + tint)) as f32;
                data[i + 1] = v as f32;
                data[i + 2] = (v * (1.0 - tint)) as f32;
            }
        }
        img
    }

    fn params_for(stock: &crate::profile::StockProfile, lens: crate::optics::LensParams) -> crate::chain::FilmChainParams {
        crate::chain::FilmChainParams::new(stock.clone())
            .expect("profile loads")
            .with_adjustments(Default::default(), lens)
            .expect("defaults validate")
    }

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
        let cpu = crate::chain::render(&img, &params).expect("cpu render");
        let gpu = super::render_gpu(&img, &params, &ctx).expect("gpu render");
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
    use crate::frame::{CropRule, FilmFrame};
    use crate::grain::GrainField;
    use crate::optics::Plane;
    use melography_gpu::spatial::run_grain;

    fn density_plane(w: u32, h: u32) -> Plane {
        let mut p = Plane::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w.max(1) - 1);
                let fy = f64::from(y) / f64::from(h.max(1) - 1);
                let disc = (-(((fx - 0.6) * (fx - 0.6) + (fy - 0.35) * (fy - 0.35)) / 0.01).exp()
                    * 3.0)
                    .exp();
                let v = (0.02 + 2.2 * fx + 0.4 * (1.0 - fy) + 1.2 * disc).min(3.0);
                p.set3(x, y, [v * 1.03, v, v * 0.97]);
            }
        }
        p
    }

    fn grain_leg(name: &str, stock: &crate::profile::StockProfile, w: u32, h: u32) {
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
        use crate::chain::ChainStages;
        use crate::gpu_bridge::{
            pack_film_pixel_params, pack_veiling_gpu_params, pack_vignette_gpu_params,
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
            crate::grain::GrainStage::Full(f) => f.gpu_params(&st.frame, w, h),
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
        use crate::chain::ChainStages;
        use crate::emulsion::{apply_excitation_inplace, apply_response_inplace};
        use crate::grain::GrainField;
        use crate::optics::apply_vignette_inplace;
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
        apply_excitation_inplace(
            &mut plane,
            0,
            h,
            &st.em,
            p.adjustments.ev,
            p.adjustments.pre_desaturate,
        );
        apply_response_inplace(&mut plane, 0, h, &st.em);
        let field = match &st.grain {
            crate::grain::GrainStage::Full(f) => f,
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
            dmin = dmin.min(f64::from(*v));
            dmax = dmax.max(f64::from(*v));
        }
        eprintln!(
            "legB/standalone-grain-on-respond-output: worst |Δ| = {worst:.3e} at sample {worst_at} (density range {dmin:.4}..{dmax:.4})"
        );
    }
}

#[test]
fn full_chain_parity_stage_isolation() {
    let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(melography_gpu::GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter here");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
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
    let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(melography_gpu::GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter here");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
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
    let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(melography_gpu::GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter here");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
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
    let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto) {
        Ok(c) => c,
        Err(melography_gpu::GpuError::NoAdapter) => {
            eprintln!("SKIP: no GPU adapter here");
            return;
        }
        Err(e) => panic!("gpu context init failed: {e}"),
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
}
