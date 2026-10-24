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
}

/// Packed f32 layout (documented; the WGSL side decodes the same offsets):
///
/// ```text
/// [0]  flags = mono | reversal<<1 | path<<2
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
    for curve in &params.hd_curves {
        for i in 0..n_hd {
            p.push(curve.xs[i]);
            p.push(curve.ys[i]);
            p.push(curve.ms[i]);
        }
    }
    for curve in &params.paper_curves {
        for i in 0..n_pp {
            p.push(curve.xs[i]);
            p.push(curve.ys[i]);
            p.push(curve.ms[i]);
        }
    }
    Ok(p)
}
