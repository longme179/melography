//! Plain-data parameter types for the WGSL spatial kernels (A2.3: grain
//! gather + halation) — always compiled (no `wgpu` feature needed) so
//! `melography-film` can build GPU parameter packs without pulling the
//! graphics stack (the `film_pixel_types` pattern).
//!
//! Everything the kernels need is **resolved on the CPU in f64**
//! (calibrated `a_norm`, the Cholesky factor, the u32 presence
//! threshold, the supersampling factor `k`) and shipped as f32/u32 — the
//! GPU never re-derives policy, only evaluates the same formulas. The
//! integer quantities travel as `f32::from_bits(u32)` and are recovered
//! with `bitcast<u32>` in WGSL (exact for any u32, unlike a value
//! cast).

/// Parameters for the grain-gather kernel — the **full** FILM_MODEL §G
/// field (jittered lattice, per-cell Bernoulli presence, normal radii,
/// Cholesky-correlated layer amplitudes, pixel-area supersampling)
/// evaluated per pixel on the GPU. The u32 hash stream is bit-portable,
/// so presence decisions and jitter positions match the CPU exactly;
//! only the transcendental evaluation (Box–Muller, bump weights) runs
/// in f32 — covered by the parity tolerance, not bit-equality.
#[derive(Debug, Clone)]
pub struct GrainGpuParams {
    /// Grain stage off → the kernel copies `src` to `dst` (the CPU
    /// `apply` does the same pass-through).
    pub enabled: bool,
    /// 1 active layer (replicated to all three) vs 3.
    pub layers_mono: bool,
    pub mm_per_pixel: f32,
    pub cells_per_mm: f32,
    pub cell_mm: f32,
    pub mu_r: f32,
    pub sigma_r: f32,
    /// Maximum grain radius (mm) — `mu_r + 4·sigma_r`; the gather reach
    /// is `r_max + cell_mm`.
    pub r_max_mm: f32,
    /// Cholesky factor of the layer correlation matrix (row-major 3×3).
    pub chol: [f32; 9],
    /// Per-layer calibrated amplitude normalization.
    pub a_norm: [f32; 3],
    pub seed: u32,
    /// Presence threshold: a cell is present iff `draw < presence_thr`.
    /// Exact u32 form of the CPU's f64 comparison
    /// `f64(draw) < presence_p · 2^32` ⇔ `draw < ceil(p·2^32)` —
    /// bit-matching presence decisions without f64 on the GPU.
    pub presence_thr: u32,
    /// `p·2^32 ≥ 2^32`: every draw passes (packed threshold unused).
    pub presence_always: bool,
    pub w: u32,
    pub h: u32,
    /// Pixel-area supersampling factor (CPU rule:
    /// `clamp(round(mmpp / 6 µm), 1, 4)`, computed in f64 at pack time).
    pub k: u32,
    /// σ(D) curve, parallel arrays (piecewise-linear, flat ends).
    pub curve_density: Vec<f32>,
    pub curve_sigma: Vec<f32>,
}

/// Parameters for the halation kernel (FILM_MODEL §C2) — separable
/// h+v gaussian mixture per channel with per-channel gain α. A channel
/// with `radius == 0` is inactive (pass-through), matching the CPU
/// `apply_halation` skip for `alpha ≤ 0` or identity kernels.
#[derive(Debug, Clone)]
pub struct HalationGpuParams {
    pub w: u32,
    pub h: u32,
    /// Per-channel gain α (inactive channels pack 0).
    pub alpha: [f32; 3],
    /// Per-channel truncation radius in pixels (0 = inactive).
    pub radius: [u32; 3],
    /// Flat tap array; channel c's `2·radius[c]+1` unit-DC taps start at
    /// `tap_offset[c]`.
    pub tap_offset: [u32; 3],
    pub taps: Vec<f32>,
}

/// Packed f32 layout for the grain kernel (documented; the WGSL side
/// decodes the same offsets):
///
/// ```text
/// [0]  mm_per_pixel
/// [1]  cells_per_mm
/// [2]  cell_mm
/// [3]  mu_r
/// [4]  sigma_r
/// [5]  r_max_mm
/// [6..15)   chol (row-major 3×3)
/// [15..18)  a_norm
/// [18] seed          (u32 bits)
/// [19] presence_thr  (u32 bits)
/// [20] flags         (u32 bits): 1 = enabled, 2 = mono, 4 = presence_always
/// [21] w             (u32 bits)
/// [22] h             (u32 bits)
/// [23] k             (u32 bits)
/// [24] curve_n       (u32 bits)
/// [25..)  curve pairs: n × (density, sigma)
/// ```
#[allow(dead_code)]
pub(crate) fn pack_grain(params: &GrainGpuParams) -> Result<Vec<f32>, String> {
    if params.curve_density.len() != params.curve_sigma.len() {
        return Err(format!(
            "grain kernel: σ(D) curve arrays must be parallel ({} vs {})",
            params.curve_density.len(),
            params.curve_sigma.len()
        ));
    }
    let mut flags = 0u32;
    if params.enabled {
        flags |= 1;
    }
    if params.layers_mono {
        flags |= 2;
    }
    if params.presence_always {
        flags |= 4;
    }
    let mut v: Vec<f32> = Vec::with_capacity(25 + 2 * params.curve_density.len());
    v.push(params.mm_per_pixel);
    v.push(params.cells_per_mm);
    v.push(params.cell_mm);
    v.push(params.mu_r);
    v.push(params.sigma_r);
    v.push(params.r_max_mm);
    v.extend_from_slice(&params.chol);
    v.extend_from_slice(&params.a_norm);
    v.push(f32::from_bits(params.seed));
    v.push(f32::from_bits(params.presence_thr));
    v.push(f32::from_bits(flags));
    v.push(f32::from_bits(params.w));
    v.push(f32::from_bits(params.h));
    v.push(f32::from_bits(params.k));
    v.push(f32::from_bits(params.curve_density.len() as u32));
    for (d, s) in params.curve_density.iter().zip(&params.curve_sigma) {
        v.push(*d);
        v.push(*s);
    }
    Ok(v)
}

/// Packed f32 layout for the halation kernels:
///
/// ```text
/// [0]   flags (u32 bits): unused (reserved; 0)
/// [1]   w    (u32 bits)
/// [2]   h    (u32 bits)
/// [3..6)    radius[c]     (u32 bits; 0 = inactive)
/// [6..9)    tap_offset[c] (u32 bits)
/// [9..12)   alpha[c]
/// [12..)    taps (flat)
/// ```
#[allow(dead_code)]
pub(crate) fn pack_halation(params: &HalationGpuParams) -> Result<Vec<f32>, String> {
    let mut v: Vec<f32> = Vec::with_capacity(12 + params.taps.len());
    v.push(f32::from_bits(0));
    v.push(f32::from_bits(params.w));
    v.push(f32::from_bits(params.h));
    for c in 0..3 {
        v.push(f32::from_bits(params.radius[c]));
    }
    for c in 0..3 {
        v.push(f32::from_bits(params.tap_offset[c]));
    }
    v.extend_from_slice(&params.alpha);
    v.extend_from_slice(&params.taps);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grain() -> GrainGpuParams {
        GrainGpuParams {
            enabled: true,
            layers_mono: false,
            mm_per_pixel: 0.017578,
            cells_per_mm: 46.0,
            cell_mm: 1.0 / 46.0,
            mu_r: 0.006,
            sigma_r: 0.0028,
            r_max_mm: 0.0172,
            chol: [1.0, 0.0, 0.5, 0.0, 1.0, 0.25, 0.5, 0.25, 1.0],
            a_norm: [0.9, 1.0, 1.1],
            seed: 0xDEAD_BEEF,
            presence_thr: 3_000_000_000,
            presence_always: false,
            w: 320,
            h: 240,
            k: 3,
            curve_density: vec![0.0, 0.5, 1.2, 2.0],
            curve_sigma: vec![0.012, 0.032, 0.04, 0.022],
        }
    }

    #[test]
    fn grain_pack_layout() {
        let p = pack_grain(&grain()).unwrap();
        assert_eq!(p.len(), 25 + 8, "fixed header + 4 curve pairs");
        assert_eq!(p[0], 0.017578);
        assert_eq!(p[6], 1.0, "chol[0]");
        assert_eq!(p[15], 0.9, "a_norm[0]");
        assert_eq!(p[18].to_bits(), 0xDEAD_BEEF, "seed bits");
        assert_eq!(p[19].to_bits(), 3_000_000_000, "presence threshold bits");
        assert_eq!(p[20].to_bits(), 1, "flags: enabled only");
        assert_eq!(p[21].to_bits(), 320, "w bits");
        assert_eq!(p[22].to_bits(), 240, "h bits");
        assert_eq!(p[23].to_bits(), 3, "k bits");
        assert_eq!(p[24].to_bits(), 4, "curve_n bits");
        assert_eq!(p[25], 0.0, "curve density[0]");
        assert_eq!(p[26], 0.012, "curve sigma[0]");
        // Mismatched curve arrays are an honest error, not a wrong pack.
        let mut bad = grain();
        bad.curve_sigma.pop();
        assert!(pack_grain(&bad).is_err());
    }

    #[test]
    fn grain_pack_flags_roundtrip() {
        let mut g = grain();
        g.layers_mono = true;
        g.presence_always = true;
        g.enabled = false;
        let p = pack_grain(&g).unwrap();
        assert_eq!(p[20].to_bits(), 2 | 4, "mono | always, disabled");
    }

    fn halation() -> HalationGpuParams {
        HalationGpuParams {
            w: 64,
            h: 48,
            alpha: [0.045, 0.008, 0.0],
            radius: [10, 5, 0],
            tap_offset: [0, 21, 0],
            taps: (0..31).map(|i| i as f32 * 0.01).collect(),
        }
    }

    #[test]
    fn halation_pack_layout() {
        let p = pack_halation(&halation()).unwrap();
        assert_eq!(p.len(), 12 + 31);
        assert_eq!(p[1].to_bits(), 64, "w bits");
        assert_eq!(p[2].to_bits(), 48, "h bits");
        assert_eq!(p[3].to_bits(), 10, "radius[0] bits");
        assert_eq!(p[4].to_bits(), 5, "radius[1] bits");
        assert_eq!(p[5].to_bits(), 0, "radius[2] inactive");
        assert_eq!(p[6].to_bits(), 0, "tap_offset[0]");
        assert_eq!(p[7].to_bits(), 21, "tap_offset[1]");
        assert_eq!(p[9], 0.045, "alpha[0]");
        assert_eq!(p[12], 0.0, "taps[0]");
        assert_eq!(p[12 + 30], 0.30, "taps[30]");
    }
}
