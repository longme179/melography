//! Plain-data parameter types for the film pixel kernel — always
//! compiled (no `wgpu` feature needed) so `melography-film` can build
//! GPU parameter packs without pulling the graphics stack.

/// One monotone spline as plain data (x, y, tangent triplets).
#[derive(Debug, Clone)]
pub struct CurveGpu {
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub ms: Vec<f32>,
}

/// Plain-data parameters for the film pixel kernel (built by callers from
/// the resolved `Emulsion`/`Print`; every field mirrors a CPU quantity).
#[derive(Debug, Clone)]
pub struct FilmPixelGpuParams {
    pub layers_mono: bool,
    pub polarity_reversal: bool,
    /// 0 = paper, 1 = scanner, 2 = direct-scan.
    pub path: u32,
    pub x_floor: f32,
    /// exposure_scale · push_exposure · 2^EV (total per-pixel scale).
    pub total_scale: f32,
    pub pre_desat: f32,
    pub gamma_mult: [f32; 3],
    pub crosstalk_scale: f32,
    pub bleach_bypass: f32,
    pub print_exposure: f32,
    pub paper_white_density: f32,
    pub scanner_black: f32,
    pub scanner_white: f32,
    pub scanner_gamma: f32,
    pub scanner_glare: f32,
    pub m_exc: [f32; 9],
    pub crosstalk: [f32; 9],
    pub d_min: [f32; 3],
    pub d_mask: [f32; 3],
    pub output_matrix: [f32; 9],
    pub scanner_matrix: [f32; 9],
    /// Per-layer H&D curves (3 layers for color, 1 for mono; equal control
    /// counts — the packer rejects anything else, honestly).
    pub hd_curves: Vec<CurveGpu>,
    pub paper_curves: Vec<CurveGpu>,
    /// A2.4 stage selection (which per-pixel stages this dispatch runs):
    /// bit 0 = excitation (§A5 pre-desat + §C1/C3), bit 1 = response
    /// (§D/§E), bit 2 = print (§F). `7` = the fused all-stages dispatch
    /// (the pre-A2.4 behaviour, and what standalone parity legs use);
    /// the full-chain GPU composition runs 1, 2 and 4 as separate
    /// dispatches with the spatial kernels (veiling/halation/grain)
    /// interleaved between them — same math, f32 buffer round-trips are
    /// lossless, so split ≡ fused is gated bit-exact.
    pub stage: u8,
}

/// Packed f32 layout (documented; the WGSL side decodes the same offsets):
///
/// ```text
/// [0]  flags = mono | reversal<<1 | path<<2 | stage<<4
///      (path is 2 bits: <<2 and <<3; stage is 3 bits, A2.4)
/// [1]  n_hd (control points per layer)
/// [2]  n_paper
/// [3]  x_floor
/// [4]  total_scale
/// [5]  pre_desat
/// [6..9)   gamma_mult
/// [9]  crosstalk_scale
/// [10] bleach_bypass
/// [11] print_exposure
/// [12] paper_white_density
/// [13] scanner_black
/// [14] scanner_white
/// [15] scanner_gamma
/// [16] scanner_glare
/// [17..26)  m_exc
/// [26..35)  crosstalk
/// [35..38)  d_min
/// [38..41)  d_mask
/// [41..50)  output_matrix
/// [50..59)  scanner_matrix
/// [59..)    hd curves: per layer, n_hd × (x, y, m)
/// …         paper curves: per layer, n_paper × (x, y, m)
/// ```
/// Used by the `wgpu`-gated dispatch in `film_pixel.rs`.
#[allow(dead_code)]
pub(crate) fn pack(params: &FilmPixelGpuParams) -> Result<Vec<f32>, String> {
    let n_hd_set: Vec<usize> = params.hd_curves.iter().map(|c| c.xs.len()).collect();
    if n_hd_set.iter().min() != n_hd_set.iter().max() {
        return Err(format!(
            "GPU film kernel requires equal H&D control counts per layer (got {n_hd_set:?})"
        ));
    }
    let n_pp_set: Vec<usize> = params.paper_curves.iter().map(|c| c.xs.len()).collect();
    if n_pp_set.iter().min() != n_pp_set.iter().max() {
        return Err(format!(
            "GPU film kernel requires equal paper control counts per layer (got {n_pp_set:?})"
        ));
    }
    let n_hd = n_hd_set[0];
    let n_pp = n_pp_set[0];
    let mut p: Vec<f32> = Vec::with_capacity(59 + 3 * 3 * (n_hd + n_pp));
    let mut flags = 0u32;
    if params.layers_mono {
        flags |= 1;
    }
    if params.polarity_reversal {
        flags |= 2;
    }
    flags |= params.path << 2;
    flags |= u32::from(params.stage & 7) << 4;
    p.push(flags as f32);
    p.push(n_hd as f32);
    p.push(n_pp as f32);
    p.push(params.x_floor);
    p.push(params.total_scale);
    p.push(params.pre_desat);
    p.extend_from_slice(&params.gamma_mult);
    p.push(params.crosstalk_scale);
    p.push(params.bleach_bypass);
    p.push(params.print_exposure);
    p.push(params.paper_white_density);
    p.push(params.scanner_black);
    p.push(params.scanner_white);
    p.push(params.scanner_gamma);
    p.push(params.scanner_glare);
    p.extend_from_slice(&params.m_exc);
    p.extend_from_slice(&params.crosstalk);
    p.extend_from_slice(&params.d_min);
    p.extend_from_slice(&params.d_mask);
    p.extend_from_slice(&params.output_matrix);
    p.extend_from_slice(&params.scanner_matrix);
    // Curve slots are ALWAYS padded to 3 (the WGSL base offsets reserve
    // 3·n_hd floats for the H&D block — the mono layout bug, CI runs
    // 40075841352: reading paper curves past the packed length on real
    // adapters returned garbage). Inactive slots duplicate layer 0;
    // the shader never reads them, the offsets stay put.
    for slot in 0..3 {
        let curve = params
            .hd_curves
            .get(slot)
            .or_else(|| params.hd_curves.first())
            .ok_or("empty hd_curves")?;
        for i in 0..n_hd {
            p.push(curve.xs[i]);
            p.push(curve.ys[i]);
            p.push(curve.ms[i]);
        }
    }
    for slot in 0..3 {
        let curve = params
            .paper_curves
            .get(slot)
            .or_else(|| params.paper_curves.first())
            .ok_or("empty paper_curves")?;
        for i in 0..n_pp {
            p.push(curve.xs[i]);
            p.push(curve.ys[i]);
            p.push(curve.ms[i]);
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(n: usize) -> CurveGpu {
        CurveGpu {
            xs: (0..n).map(|i| i as f32).collect(),
            ys: (0..n).map(|i| i as f32 * 0.1).collect(),
            ms: (0..n).map(|i| i as f32 * 0.05).collect(),
        }
    }

    /// The layout contract: the buffer always reserves 3 curve slots per
    /// block (mono pads) — the exact invariant that broke on real adapters
    /// (CI 40075841352) while local struct-level sims stayed green.
    #[test]
    fn pack_reserves_three_curve_slots_for_mono() {
        let p = FilmPixelGpuParams {
            layers_mono: true,
            polarity_reversal: false,
            path: 0,
            x_floor: 1e-5,
            total_scale: 1.0,
            pre_desat: 0.0,
            gamma_mult: [1.0; 3],
            crosstalk_scale: 1.0,
            bleach_bypass: 0.0,
            print_exposure: 1.0,
            paper_white_density: 0.02,
            scanner_black: -3.0,
            scanner_white: -0.2,
            scanner_gamma: 1.0,
            scanner_glare: 0.0,
            m_exc: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            crosstalk: [0.0; 9],
            d_min: [0.0; 3],
            d_mask: [0.0; 3],
            output_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            scanner_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            hd_curves: vec![curve(4)],
            paper_curves: vec![curve(5)],
        };
        let packed = pack(&p).unwrap();
        // 59 fixed + 3·(3·n_hd) + 3·(3·n_pp)
        let expected = 59 + 3 * 3 * 4 + 3 * 3 * 5;
        assert_eq!(
            packed.len(),
            expected,
            "3-slot reservation must hold for mono"
        );
        // Paper slot 0 starts right after the 3 hd slots.
        let pp0 = 59 + 3 * 3 * 4;
        assert_eq!(packed[pp0], 0.0, "paper slot 0 x[0]");
    }
}
