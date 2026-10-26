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

    fn repo_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap()
    }

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
        let reg =
            melography_presets::StockRegistry::open(&repo_root().join("profiles")).expect("profiles");
        let stock = reg.by_id("soft-pastel-160").expect("pastel stock");
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
}
