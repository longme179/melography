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
/// only the transcendental evaluation (Box–Muller, bump weights) runs
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
    /// `mmpp / cell_mm` (cells per pixel) — only used for the SMALL
    /// sub-sample offsets inside a pixel (≤ m cells); single f32 is
    /// ample there.
    pub m: f32,
    /// Per-COLUMN sample positions in cell units as exact
    /// (integer + fraction) pairs, computed on the CPU in f64 from the
    /// SAME `pixel_to_mm` formula the reference uses: `[qi bits, qf]`
    /// per column, so the shader never forms a large coordinate. This
    /// is the CI round 3–7 lesson: absolute film coordinates are ~18 mm
    /// where f32 eps is a third of a grain radius, compensated (hi+lo)
    /// arithmetic is vulnerable to backend fast-math/contraction (Metal
    /// vs WARP diverged 5× on the same WGSL), and (integer, fraction)
    /// LUTs leave only small-number math in the shader — immune by
    /// construction.
    pub u_pos: Vec<f32>,
    /// Per-ROW positions, same `[qi bits, qf]` layout.
    pub v_pos: Vec<f32>,
    /// Integer cell-index ranges where the cell-centre density read is
    /// INSIDE the plane — the exact f64 decisions of the CPU's bilinear
    /// guards, direct-evaluated and corrected at pack time. A f32 guard
    /// in the shader could flip at the image edge and jump the read by
    /// the full edge density (a σ discontinuity, not a smooth error).
    pub guard_ix_min: i32,
    pub guard_ix_max: i32,
    pub guard_iy_min: i32,
    pub guard_iy_max: i32,
    /// σ(D) curve, parallel arrays (piecewise-linear, flat ends).
    pub curve_density: Vec<f32>,
    pub curve_sigma: Vec<f32>,
}

/// Parameters for the halation kernel (FILM_MODEL §C2) — separable
/// h+v gaussian mixture per channel with per-channel gain α. A channel
/// with `radius == 0` is inactive (pass-through), matching the CPU
/// `apply_halation` skip for `alpha ≤ 0` or identity kernels.
///
/// A2.4 — `blend` selects the veiling §B mix on the SAME h+v dispatch
/// structure: `true` computes `(1−α)·src + α·G₂D(src)` (the CPU
/// `apply_veiling_mix` formula) instead of the additive
/// `src + α·G₂D(src)`. The unit-DC taps are identical either way; only
/// the vertical-pass combine differs (params slot [0], one bit per
/// channel). Halation packs `blend = [false; 3]` — bit-compatible with
/// every pre-A2.4 packed payload.
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
    /// Per-channel combine mode: `false` = additive halation
    /// (`src + α·G`), `true` = veiling blend (`(1−α)·src + α·G`).
    pub blend: [bool; 3],
    pub taps: Vec<f32>,
}

/// Parameters for the vignette kernel (FILM_MODEL §B) — the lens
/// falloff `e · cos(r_n·half_fov)^k` with `r_n` from per-column and
/// per-row offsets. Following the A2.3 lesson, positions are shipped
/// as CPU-resolved (f64 → f32) per-column `u−c_u` and per-row `v−c_v`
/// LUTs, so the shader only ever does small-number math — no absolute
/// film coordinates, no accumulated error.
#[derive(Debug, Clone)]
pub struct VignetteGpuParams {
    pub w: u32,
    pub h: u32,
    /// Half diagonal of the frame in mm (the r_n normalizer).
    pub half_diag_mm: f32,
    /// Half the lens field of view, radians.
    pub half_fov_rad: f32,
    /// Falloff exponent `k = 4·vignette_amount` (> 0; `k ≤ 0` is the
    /// exact identity on both sides and the orchestrator skips the
    /// dispatch entirely).
    pub k: f32,
    /// Per-column `u − frame_centre_u` in mm (length `w`).
    pub du: Vec<f32>,
    /// Per-row `v − frame_centre_v` in mm (length `h`).
    pub dv: Vec<f32>,
}

/// Packed f32 layout for the grain kernel (documented; the WGSL side
/// decodes the same offsets). The per-column/row position LUTs travel
/// as SEPARATE storage buffers (`u_pos`/`v_pos`, `[qi bits, qf]` per
/// index) — see `run_grain`'s bind group.
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
/// [25] m             (mmpp/cell_mm, for the sub-sample offsets)
/// [26] guard_ix_min  (i32 bits) [27] guard_ix_max (i32 bits)
/// [28] guard_iy_min  (i32 bits) [29] guard_iy_max (i32 bits)
/// [30..)  curve pairs: n × (density, sigma)
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
    let expected_u = params.w as usize * 2;
    if params.u_pos.len() != expected_u {
        return Err(format!(
            "grain kernel: u_pos must hold w×2 entries ({} vs {expected_u})",
            params.u_pos.len()
        ));
    }
    let expected_v = params.h as usize * 2;
    if params.v_pos.len() != expected_v {
        return Err(format!(
            "grain kernel: v_pos must hold h×2 entries ({} vs {expected_v})",
            params.v_pos.len()
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
    let mut v: Vec<f32> = Vec::with_capacity(30 + 2 * params.curve_density.len());
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
    v.push(params.m);
    v.push(f32::from_bits(params.guard_ix_min as u32));
    v.push(f32::from_bits(params.guard_ix_max as u32));
    v.push(f32::from_bits(params.guard_iy_min as u32));
    v.push(f32::from_bits(params.guard_iy_max as u32));
    for (d, s) in params.curve_density.iter().zip(&params.curve_sigma) {
        v.push(*d);
        v.push(*s);
    }
    Ok(v)
}

/// Packed f32 layout for the halation kernels:
///
/// ```text
/// [0]   flags (u32 bits): bit c = blend mode for channel c
///       (0 = additive halation, 1 = veiling blend); halation = 0
/// [1]   w    (u32 bits)
/// [2]   h    (u32 bits)
/// [3..6)    radius[c]     (u32 bits; 0 = inactive)
/// [6..9)    tap_offset[c] (u32 bits)
/// [9..12)   alpha[c]
/// [12..)    taps (flat)
/// ```
#[allow(dead_code)]
pub(crate) fn pack_halation(params: &HalationGpuParams) -> Result<Vec<f32>, String> {
    let mut blend_flags = 0u32;
    for c in 0..3 {
        if params.blend[c] {
            blend_flags |= 1 << c;
        }
    }
    let mut v: Vec<f32> = Vec::with_capacity(12 + params.taps.len());
    v.push(f32::from_bits(blend_flags));
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

/// Packed f32 layout for the vignette kernel:
///
/// ```text
/// [0]      flags (u32 bits): reserved; 0
/// [1]      w    (u32 bits)
/// [2]      h    (u32 bits)
/// [3]      half_diag_mm
/// [4]      half_fov_rad
/// [5]      k
/// [6..6+w)      du per column (mm)
/// [6+w..6+w+h)  dv per row (mm)
/// ```
#[allow(dead_code)]
pub(crate) fn pack_vignette(params: &VignetteGpuParams) -> Result<Vec<f32>, String> {
    if params.du.len() != params.w as usize || params.dv.len() != params.h as usize {
        return Err(format!(
            "vignette kernel: LUT lengths ({}, {}) do not match w×h ({}, {})",
            params.du.len(),
            params.dv.len(),
            params.w,
            params.h
        ));
    }
    let mut v: Vec<f32> = Vec::with_capacity(6 + params.du.len() + params.dv.len());
    v.push(f32::from_bits(0));
    v.push(f32::from_bits(params.w));
    v.push(f32::from_bits(params.h));
    v.push(params.half_diag_mm);
    v.push(params.half_fov_rad);
    v.push(params.k);
    v.extend_from_slice(&params.du);
    v.extend_from_slice(&params.dv);
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
            w: 4,
            h: 3,
            k: 3,
            m: 0.276,
            // Two example columns/rows of the (integer, fraction) LUT:
            // column 0 at 821.25 cells, column 1 at 821.526 cells, …
            u_pos: vec![
                f32::from_bits(821i32 as u32),
                0.25,
                f32::from_bits(821i32 as u32),
                0.526,
                f32::from_bits(822i32 as u32),
                0.115,
                f32::from_bits(822i32 as u32),
                0.391,
            ],
            v_pos: vec![
                f32::from_bits(469i32 as u32),
                0.5,
                f32::from_bits(469i32 as u32),
                0.776,
                f32::from_bits(470i32 as u32),
                0.052,
            ],
            guard_ix_min: -3,
            guard_ix_max: 831,
            guard_iy_min: 469,
            guard_iy_max: 634,
            curve_density: vec![0.0, 0.5, 1.2, 2.0],
            curve_sigma: vec![0.012, 0.032, 0.04, 0.022],
        }
    }

    #[test]
    fn grain_pack_layout() {
        let p = pack_grain(&grain()).unwrap();
        assert_eq!(p.len(), 30 + 8, "fixed header + 4 curve pairs");
        assert_eq!(p[0], 0.017578);
        assert_eq!(p[6], 1.0, "chol[0]");
        assert_eq!(p[15], 0.9, "a_norm[0]");
        assert_eq!(p[18].to_bits(), 0xDEAD_BEEF, "seed bits");
        assert_eq!(p[19].to_bits(), 3_000_000_000, "presence threshold bits");
        assert_eq!(p[20].to_bits(), 1, "flags: enabled only");
        assert_eq!(p[21].to_bits(), 4, "w bits");
        assert_eq!(p[22].to_bits(), 3, "h bits");
        assert_eq!(p[23].to_bits(), 3, "k bits");
        assert_eq!(p[24].to_bits(), 4, "curve_n bits");
        assert_eq!(p[25], 0.276, "m");
        assert_eq!(p[26].to_bits(), (-3i32) as u32, "guard_ix_min bits");
        assert_eq!(p[27].to_bits(), 831, "guard_ix_max bits");
        assert_eq!(p[28].to_bits(), 469, "guard_iy_min bits");
        assert_eq!(p[29].to_bits(), 634, "guard_iy_max bits");
        assert_eq!(p[30], 0.0, "curve density[0]");
        assert_eq!(p[31], 0.012, "curve sigma[0]");
        // LUT length mismatches are honest errors, not wrong packs.
        let mut bad = grain();
        bad.curve_sigma.pop();
        assert!(pack_grain(&bad).is_err());
        let mut bad = grain();
        bad.u_pos.pop();
        assert!(pack_grain(&bad).is_err());
        let mut bad = grain();
        bad.v_pos.clear();
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
            blend: [false; 3],
            // Power-of-two step: every value exactly representable in
            // f32 (the layout test compares bits, not tolerances).
            taps: (0..31).map(|i| i as f32 * 0.125).collect(),
        }
    }

    #[test]
    fn halation_pack_layout() {
        let p = pack_halation(&halation()).unwrap();
        assert_eq!(p.len(), 12 + 31);
        assert_eq!(p[0].to_bits(), 0, "halation: no blend flags");
        assert_eq!(p[1].to_bits(), 64, "w bits");
        assert_eq!(p[2].to_bits(), 48, "h bits");
        assert_eq!(p[3].to_bits(), 10, "radius[0] bits");
        assert_eq!(p[4].to_bits(), 5, "radius[1] bits");
        assert_eq!(p[5].to_bits(), 0, "radius[2] inactive");
        assert_eq!(p[6].to_bits(), 0, "tap_offset[0]");
        assert_eq!(p[7].to_bits(), 21, "tap_offset[1]");
        assert_eq!(p[9], 0.045, "alpha[0]");
        assert_eq!(p[12], 0.0, "taps[0]");
        assert_eq!(p[12 + 30], 3.75, "taps[30]");
    }

    #[test]
    fn halation_blend_flags_pack() {
        // Veiling (A2.4): all three channels blend → bits 0|1|2 = 7.
        let mut h = halation();
        h.blend = [true, true, true];
        let p = pack_halation(&h).unwrap();
        assert_eq!(p[0].to_bits(), 0b111, "veiling: blend on every channel");
        // Mixed packs stay per-channel.
        h.blend = [false, true, false];
        let p = pack_halation(&h).unwrap();
        assert_eq!(p[0].to_bits(), 0b010);
    }

    #[test]
    fn vignette_pack_layout() {
        let v = VignetteGpuParams {
            w: 4,
            h: 3,
            half_diag_mm: 21.633_308,
            half_fov_rad: 0.34906585,
            k: 1.0,
            du: vec![-18.0, -6.0, 6.0, 18.0],
            dv: vec![-12.0, 0.0, 12.0],
        };
        let p = pack_vignette(&v).unwrap();
        assert_eq!(p.len(), 6 + 4 + 3);
        assert_eq!(p[0].to_bits(), 0, "flags reserved");
        assert_eq!(p[1].to_bits(), 4, "w bits");
        assert_eq!(p[2].to_bits(), 3, "h bits");
        assert_eq!(p[3], 21.633_308, "half_diag");
        assert_eq!(p[4], 0.34906585, "half_fov");
        assert_eq!(p[5], 1.0, "k");
        assert_eq!(p[6], -18.0, "du[0]");
        assert_eq!(p[6 + 4], -12.0, "dv[0]");
        assert_eq!(p[6 + 4 + 2], 12.0, "dv[2]");
        // Length contract: du must match w, dv must match h.
        let bad = VignetteGpuParams {
            dv: vec![0.0],
            ..v.clone()
        };
        assert!(pack_vignette(&bad).is_err());
    }
}

