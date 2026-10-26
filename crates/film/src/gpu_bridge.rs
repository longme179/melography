//! GPU parameter bridge: resolved `Emulsion`/`Print` → plain-data
//! `FilmPixelGpuParams` for the WGSL film pixel kernel (mirror-layer
//! contract, COLOR_PIPELINE §8).
//!
//! Everything the kernel needs is **resolved on the CPU in f64** (γ
//! guards, cross-process d_min shifts, the exposure anchor) and shipped
//! as f32 — the GPU never re-derives policy, only evaluates the same
//! formulas. That keeps the parity gate meaningful: a mismatch is a bug
//! in one of the two mirrors, not a policy difference.

use crate::emulsion::{Adjustments, CrossProcessMode, Emulsion};
use crate::print::Print;
use crate::profile::{InversionPath, LayerLayout, Polarity};
use melography_gpu::film_pixel_types::{CurveGpu, FilmPixelGpuParams};
use melography_gpu::spatial_types::HalationGpuParams;

fn curve_gpu(spline: &crate::curves::MonotoneSpline) -> CurveGpu {
    let (xs, ys, ms) = spline.control_data();
    CurveGpu {
        xs: xs.iter().map(|v| *v as f32).collect(),
        ys: ys.iter().map(|v| *v as f32).collect(),
        ms: ms.iter().map(|v| *v as f32).collect(),
    }
}

/// Pack the per-pixel film stages (§C1/C3, §D/E, §F) for the GPU.
///
/// Note: `Emulsion` already carries the resolved γ multipliers and
/// crosstalk scale; the cross-process D_min shift is baked here because
/// the CPU applies it at response time from `em.cross_process`.
pub fn pack_film_pixel_params(
    em: &Emulsion,
    print: &Print,
    adj: &Adjustments,
) -> FilmPixelGpuParams {
    let mut d_min = em.d_min;
    match em.cross_process {
        CrossProcessMode::E6InC41 => d_min[0] += 0.08,
        CrossProcessMode::C41InE6 => {
            for v in &mut d_min {
                *v = (*v * 0.6).max(0.0);
            }
        }
        CrossProcessMode::None => {}
    }
    let total_scale = em.exposure_scale * em.push_exposure * 2f64.powf(adj.ev);
    let path = match print.path {
        InversionPath::Paper => 0,
        InversionPath::Scanner => 1,
        InversionPath::DirectScan => 2,
    };
    FilmPixelGpuParams {
        layers_mono: em.layers == LayerLayout::Monochrome,
        polarity_reversal: em.polarity == Polarity::Reversal,
        path,
        x_floor: em.x_floor as f32,
        total_scale: total_scale as f32,
        pre_desat: adj.pre_desaturate as f32,
        gamma_mult: [
            em.gamma_mult[0] as f32,
            em.gamma_mult[1] as f32,
            em.gamma_mult[2] as f32,
        ],
        crosstalk_scale: em.crosstalk_scale as f32,
        bleach_bypass: em.bleach_bypass as f32,
        print_exposure: print.l_p as f32,
        paper_white_density: print.paper_white_density as f32,
        scanner_black: print.scanner_black as f32,
        scanner_white: print.scanner_white as f32,
        scanner_gamma: print.scanner_gamma as f32,
        scanner_glare: print.scanner_glare as f32,
        m_exc: em.m_exc.0.map(|v| v as f32),
        crosstalk: em.crosstalk.0.map(|v| v as f32),
        d_min: d_min.map(|v| v as f32),
        d_mask: print.d_mask.map(|v| v as f32),
        output_matrix: print.output_matrix.0.map(|v| v as f32),
        scanner_matrix: print.scanner_matrix.0.map(|v| v as f32),
        hd_curves: em.splines.iter().map(curve_gpu).collect(),
        paper_curves: print.paper.iter().map(curve_gpu).collect(),
    }
}

/// A2.3 — pack the resolved halation kernels (§C2) for the WGSL
/// h+v convolution. A channel is ACTIVE iff its layer is active for the
/// layout AND `alpha > 0` AND the kernel is not the identity — exactly
/// the CPU `apply_halation` skip condition (inactive channels
/// pass through untouched; sub-pixel halos are skipped there too, so
/// the packer must not smuggle in a constant-term approximation).
pub fn pack_halation_gpu_params(em: &Emulsion, w: u32, h: u32) -> HalationGpuParams {
    let active_layers: &[usize] = match em.layers {
        LayerLayout::Color => &[0, 1, 2],
        LayerLayout::Monochrome => &[0],
    };
    let mut alpha = [0.0f32; 3];
    let mut radius = [0u32; 3];
    let mut tap_offset = [0u32; 3];
    let mut taps: Vec<f32> = Vec::new();
    for c in 0..3 {
        let hk = &em.halation[c];
        if !active_layers.contains(&c) || hk.alpha <= 0.0 || hk.kernel.is_identity() {
            continue;
        }
        alpha[c] = hk.alpha as f32;
        radius[c] = hk.kernel.radius_px;
        tap_offset[c] = taps.len() as u32;
        taps.extend(hk.kernel.taps.iter().map(|t| *t as f32));
    }
    HalationGpuParams {
        w,
        h,
        alpha,
        radius,
        tap_offset,
        taps,
    }
}
