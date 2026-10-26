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

    /// A2.4 mono-divergence diagnostics (CI rounds 8–11): the standalone
    /// grain legs pass at ~1e-6 while the composition diverges at 3.9e-2
    /// for classic-monochrome-400 only. Leg A: the composition rebuilt
    /// dispatch-by-dispatch but grain through the STANDALONE fresh-acquire
    /// runner. Leg B: standalone grain fed the REAL respond output (CPU
    /// stages run manually). Between them one must flip: A flips => the
    /// persistent-context dispatch; B flips => the respond-output VALUES.
    #[test]
    fn mono_composition_legs_a_b() {
        use crate::optics::LensParams;
        let ctx = match melography_gpu::context::init(&melography_gpu::context::AdapterPolicy::Auto)
        {
            Ok(c) => c,
            Err(melography_gpu::GpuError::NoAdapter) => {
                eprintln!("SKIP: no GPU adapter here");
                return;
            }
            Err(e) => panic!("gpu context init failed: {e}"),
        };
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let profiles = root.join("profiles");
        let reg = melography_presets::StockRegistry::open(&profiles).expect("profiles");
        let stock = reg.by_id("classic-monochrome-400").expect("mono stock");

        let img = crate::gpu_chain::tests::leg_image(112, 84, 0.02);
        let lens = LensParams {
            vignette_amount: 0.45,
            veiling_amount: 0.5,
            veiling_sigma_mm: 0.35,
            ..LensParams::default()
        };
        let mut p = crate::chain::FilmChainParams::new(stock.clone())
            .expect("profile loads")
            .with_adjustments(Default::default(), lens)
            .expect("defaults validate");
        p.grain_fast = false;
        let (w, h) = (img.width(), img.height());
        let st = crate::chain::ChainStages::resolve(&p, w, h);

        // ── Leg A: composition with grain via the standalone runner ──
        use crate::gpu_bridge::{
            pack_film_pixel_params, pack_veiling_gpu_params, pack_vignette_gpu_params,
        };
        use melography_gpu::film_pixel::run_film_pixel_on;
        use melography_gpu::spatial::run_grain as run_grain_standalone;
        use melography_gpu::vignette::run_vignette_on;

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
        let gp = match &st.grain {
            crate::grain::GrainStage::Full(f) => f.gpu_params(&st.frame, w, h),
            _ => unreachable!(),
        };
        cur = run_grain_standalone(&gp, &cur).expect("grain standalone");
        fp.stage = 4;
        cur = run_film_pixel_on(&ctx, &fp, &cur).expect("print");
        let cpu = crate::chain::render(&img, &p).expect("cpu render");
        let mut worst = 0.0f64;
        for (c, g) in cpu.data().iter().zip(&cur) {
            worst = worst.max((f64::from(*c) - f64::from(*g)).abs());
        }
        eprintln!("legA/composition-grain-standalone: worst |Δ| = {worst:.3e}");

        // ── Leg B: standalone grain fed the REAL respond output ──────
        use crate::emulsion::{apply_excitation_inplace, apply_response_inplace};
        use crate::grain::GrainField;
        use crate::optics::{apply_vignette_inplace, Plane};

        let mut plane = Plane::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let px = img.pixel(x, y);
                plane.set3(x, y, [f64::from(px[0]), f64::from(px[1]), f64::from(px[2])]);
            }
        }
        apply_vignette_inplace(&mut plane, 0, h, 0, &st.lens, &st.frame);
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
        let gpu_g = run_grain_standalone(&gp, &src_f32).expect("grain on respond output");
        let mut worst = 0.0f64;
        let mut worst_at = 0usize;
        for (idx, (c, g)) in cpu_g.data.iter().zip(&gpu_g).enumerate() {
            let d = (f64::from(*c) - f64::from(*g)).abs();
            if d > worst {
                worst = d;
                worst_at = idx;
            }
        }
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

    /// The leg image (the battery's test image, reused verbatim).
    fn leg_image(w: u32, h: u32, tint: f64) -> melography_core::ImageF32 {
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
}
