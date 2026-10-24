//! Stage G — deterministic, resolution-independent film grain
//! (FILM_MODEL §G, docs/research/grain.md).
//!
//! Model (Newson et al. formulation, jittered-lattice v1):
//! * the film plane is tiled by a lattice of cells (`cells_per_mm`);
//! * each cell hashes (u32, WGSL-portable) to a jittered grain center,
//!   a Bernoulli presence draw (`1 − e^−λ`), a normal radius
//!   `N(μ_r, σ_r)`, and a 3-vector of Cholesky-correlated unit normals;
//! * per-grain contribution is a compact radial bump
//!   `ρ(t) = (1 − t²)², t < 1` scaled by
//!   `A_norm_c · σ(D_center) · (L·g)_c`;
//! * the field is a function of continuous film coordinates + seed only —
//!   zoom/tiling independent by construction (grain.md §3).
//!
//! **Amplitude calibration:** `σ_curve(D)` is the *48 µm-aperture* RMS
//! target (physical convention, grain.md §4). Because the aperture
//! statistic is not analytic for the jittered lattice (presence
//! saturation, border effects), the field self-calibrates at resolve
//! time: render one synthetic constant-density patch through the exact
//! evaluation kernel, measure the aperture statistic, and rescale
//! `A_norm` — exact in one step because the field is *linear* in the
//! amplitudes (same seed ⇒ same positions/radii/presence).
//!
//! Placement (spec constant): after density formation, before print
//! exposure — the grain is *in the negative*; the print stage
//! re-photographs it.

use crate::frame::FilmFrame;
use crate::hash::RandStream;
use crate::optics::Plane;
use crate::profile::{GrainParams, LayerLayout, SigmaCurve, StockProfile};

/// Standard aperture for RMS granularity (mm) — grain.md §4.
pub const APERTURE_MM: f64 = 0.048;

/// Compact radial bump profile: `ρ(t) = (1 − t²)²` for `t < 1`, C¹ at the
/// boundary, no transcendentals (WGSL-portable, bit-exact CPU).
#[inline]
fn bump(t: f64) -> f64 {
    if t < 1.0 {
        let u = 1.0 - t * t;
        u * u
    } else {
        0.0
    }
}

/// A resolved, calibrated grain field.
pub struct GrainField {
    cells_per_mm: f64,
    cell_mm: f64,
    /// Bernoulli presence probability per cell.
    presence_p: f64,
    mu_r: f64,
    sigma_r: f64,
    sigma_curve: SigmaCurve,
    /// Cholesky factor of the layer correlation matrix (row-major 3×3).
    chol: [f64; 9],
    seed: u32,
    /// Per-layer amplitude normalization (calibrated).
    a_norm: [f64; 3],
    /// Maximum grain radius (mm) — drives the halo margin.
    pub r_max_mm: f64,
    layers: LayerLayout,
    /// Grain disabled (intensity 0 or degenerate params)?
    pub enabled: bool,
}

impl GrainField {
    /// Resolve + calibrate from a validated profile and adjustments.
    ///
    /// `intensity` scales σ_curve (recipe `grain_intensity`); `seed`
    /// overrides the stock default.
    pub fn resolve(profile: &StockProfile, intensity: f64, seed: Option<u32>) -> Self {
        let g = &profile.grain;
        let sigma_curve = SigmaCurve {
            density: g.sigma_curve.density.clone(),
            sigma: g.sigma_curve.sigma.iter().map(|s| s * intensity).collect(),
        };
        let presence_p = 1.0 - (-g.lambda).exp();
        let r_max = g.mu_r_mm + 4.0 * g.sigma_r_mm;
        let layers = profile.layers;
        let seed = seed.unwrap_or(g.seed);
        let sigma_at_cal = sigma_at(&sigma_curve, D_CAL);
        let mut field = Self {
            cells_per_mm: g.cells_per_mm,
            cell_mm: 1.0 / g.cells_per_mm,
            presence_p,
            mu_r: g.mu_r_mm,
            sigma_r: g.sigma_r_mm,
            sigma_curve,
            chol: cholesky3(&g.rho),
            seed,
            a_norm: [1.0, 1.0, 1.0],
            r_max_mm: r_max,
            layers,
            enabled: sigma_at_cal > 0.0 && intensity > 0.0,
        };
        if field.enabled {
            field.calibrate();
        }
        field
    }

    /// Halo margin (input rows) the grain stage needs at a render scale.
    pub fn margin_px(&self, mm_per_pixel: f64) -> u32 {
        // Grain centers may sit up to r_max + one cell away, and their
        // density read needs ±1 px more.
        let mm = self.r_max_mm + self.cell_mm;
        (mm / mm_per_pixel).ceil() as u32 + 1
    }

    /// One-shot exact amplitude calibration: measure the aperture statistic
    /// of the synthetic field at `D_CAL`, rescale `a_norm`.
    fn calibrate(&mut self) {
        let target = sigma_at(&self.sigma_curve, D_CAL);
        if target <= 0.0 {
            self.enabled = false;
            return;
        }
        let measured = self.aperture_sigma_on_uniform(D_CAL);
        for (a, &m) in self.a_norm.iter_mut().zip(&measured) {
            if m > 1e-12 {
                *a *= target / m;
            }
        }
    }

    /// Measure the 48 µm-aperture σ per layer on a synthetic uniform
    /// density `d` patch (the same statistic the §L gate measures).
    ///
    /// Implementation: forward-splat — iterate each lattice cell once and
    /// add its bump to the sample points it covers (gather would revisit
    /// every cell per sample; splat is ~1000× faster for the same field).
    /// The accumulated field is mathematically identical to the gather
    /// path; only float summation order differs, which is irrelevant at
    /// resolve time (render determinism is unaffected — `a_norm` is fixed
    /// after resolve).
    ///
    /// Patch: 320×320 samples at 6 µm spacing; aperture = 8×8 box.
    fn aperture_sigma_on_uniform(&self, d: f64) -> [f64; 3] {
        let spacing = 0.006; // mm — native-scan-like scale
        let n = 320usize;
        let box_n = 8usize;
        let n_win = n / box_n;
        let sigma = sigma_at(&self.sigma_curve, d);
        let mut field = vec![0.0f64; n * n * 3];
        // Cells covering the patch (plus reach).
        let extent = n as f64 * spacing;
        let reach = self.r_max_mm + self.cell_mm;
        let ix_lo = (-reach * self.cells_per_mm).floor() as i32;
        let ix_hi = ((extent + reach) * self.cells_per_mm).floor() as i32;
        for iy in ix_lo..=ix_hi {
            for ix in ix_lo..=ix_hi {
                let cell = self.cell(ix, iy, sigma);
                if !cell.present {
                    continue;
                }
                let sx_lo = (((cell.cx - cell.r) / spacing).ceil() as usize).min(n);
                let sx_hi = (((cell.cx + cell.r) / spacing).floor() as usize).min(n - 1);
                let sy_lo = (((cell.cy - cell.r) / spacing).ceil() as usize).min(n);
                let sy_hi = (((cell.cy + cell.r) / spacing).floor() as usize).min(n - 1);
                for sy in sy_lo..=sy_hi {
                    let vy = sy as f64 * spacing;
                    for sx in sx_lo..=sx_hi {
                        let vx = sx as f64 * spacing;
                        let du = vx - cell.cx;
                        let dv = vy - cell.cy;
                        let dist2 = du * du + dv * dv;
                        if dist2 >= cell.r * cell.r {
                            continue;
                        }
                        let t = dist2.sqrt() / cell.r;
                        let w = bump(t);
                        let i = (sy * n + sx) * 3;
                        field[i] += w * cell.amp[0];
                        field[i + 1] += w * cell.amp[1];
                        field[i + 2] += w * cell.amp[2];
                    }
                }
            }
        }
        // Aperture box statistics.
        let mut sums = [0.0f64; 3];
        let mut sums2 = [0.0f64; 3];
        let count = (n_win * n_win) as f64;
        for wy in 0..n_win {
            for wx in 0..n_win {
                let mut acc = [0.0f64; 3];
                for by in 0..box_n {
                    for bx in 0..box_n {
                        let i = ((wy * box_n + by) * n + wx * box_n + bx) * 3;
                        acc[0] += field[i];
                        acc[1] += field[i + 1];
                        acc[2] += field[i + 2];
                    }
                }
                for c in 0..3 {
                    let m = acc[c] / (box_n * box_n) as f64;
                    sums[c] += m;
                    sums2[c] += m * m;
                }
            }
        }
        let mut out = [0.0f64; 3];
        for (o, (&su, &su2)) in out.iter_mut().zip(sums.iter().zip(&sums2)) {
            let mean = su / count;
            *o = (su2 / count - mean * mean).max(0.0).sqrt();
        }
        out
    }

    /// Raw field value (density perturbation) at film coordinates
    /// `(u, v)` mm with *uniform* density `d` (calibration/determinism
    /// test path).
    #[cfg_attr(not(test), allow(dead_code))]
    fn eval_raw(&self, u: f64, v: f64, d: f64) -> [f64; 3] {
        let s = self.cells_per_mm;
        let reach = self.r_max_mm + self.cell_mm;
        let ix_lo = ((u - reach) * s).floor() as i32;
        let ix_hi = ((u + reach) * s).floor() as i32;
        let iy_lo = ((v - reach) * s).floor() as i32;
        let iy_hi = ((v + reach) * s).floor() as i32;
        let sigma = sigma_at(&self.sigma_curve, d);
        let mut out = [0.0f64; 3];
        for iy in iy_lo..=iy_hi {
            for ix in ix_lo..=ix_hi {
                let cell = self.cell(ix, iy, sigma);
                if !cell.present {
                    continue;
                }
                let du = u - cell.cx;
                let dv = v - cell.cy;
                let dist2 = du * du + dv * dv;
                if dist2 >= cell.r * cell.r {
                    continue;
                }
                let t = dist2.sqrt() / cell.r;
                let w = bump(t);
                out[0] += w * cell.amp[0];
                out[1] += w * cell.amp[1];
                out[2] += w * cell.amp[2];
            }
        }
        out
    }

    /// Hash-derived grain cell at lattice indices `(ix, iy)` with density
    /// `d` (at the grain center) and pre-looked-up σ(D).
    fn cell(&self, ix: i32, iy: i32, sigma: f64) -> GrainCell {
        let mut rs = RandStream::new(ix as u32, iy as u32, self.seed, 0x6A11);
        let jit_u = rs.next_f64();
        let jit_v = rs.next_f64();
        let present = f64::from(rs.next_u32()) < self.presence_p * 4_294_967_296.0;
        if !present {
            return GrainCell {
                present: false,
                cx: 0.0,
                cy: 0.0,
                r: 0.0,
                amp: [0.0; 3],
            };
        }
        let radius = (self.mu_r + self.sigma_r * rs.next_normal_f64()).max(1e-6);
        // 3 correlated unit normals via 2 Box–Muller pairs.
        let (n0, n1) = normal_pair(&mut rs);
        let (n2, _) = normal_pair(&mut rs);
        let g = [n0, n1, n2];
        let mut amp = [0.0f64; 3];
        let rows: usize = match self.layers {
            LayerLayout::Color => 3,
            LayerLayout::Monochrome => 1,
        };
        for (c, amp_c) in amp.iter_mut().enumerate().take(rows) {
            let mut v = 0.0;
            for (j, &g_j) in g.iter().enumerate().take(rows) {
                v += self.chol[3 * c + j] * g_j;
            }
            *amp_c = self.a_norm[c] * sigma * v;
        }
        if rows == 1 {
            amp = [amp[0]; 3];
        }
        GrainCell {
            present: true,
            cx: (ix as f64 + jit_u) * self.cell_mm,
            cy: (iy as f64 + jit_v) * self.cell_mm,
            r: radius,
            amp,
        }
    }

    /// Apply grain to rows `[y0, y1)` (local coords) of a strip-local
    /// density plane whose local row 0 is absolute image row `y_origin`.
    /// `image_h` is the absolute image height (film-coordinate inverse
    /// mapping). Requires `margin_px(mm_per_pixel)` valid density rows
    /// above and below the strip (grain centers outside the strip still
    /// read local density — grain.md §3 tile contract).
    ///
    /// Performance: grain cells are hashed once into a strip-local cache
    /// (lattice indices → jitter/presence/radius/amplitudes) and gathered
    /// per pixel — the per-cell hash/trig work is amortized over the ~
    /// `(cell/mmpp)²` pixels each grain covers.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &self,
        src: &Plane,
        dst: &mut Plane,
        y0: u32,
        y1: u32,
        frame: &FilmFrame,
        y_origin: u32,
        image_h: u32,
    ) {
        if !self.enabled {
            for y in y0..y1 {
                for x in 0..src.w {
                    dst.set3(x, y, src.px64(x, y));
                }
            }
            return;
        }
        let s = self.cells_per_mm;
        let reach = self.r_max_mm + self.cell_mm;
        let mmpp = frame.mm_per_pixel;
        // -- Build the cell cache for the strip's film-area window -------
        // Cell index range covering all sample points in rows [y0, y1)
        // expanded by the reach (cell centers may sit one cell beyond).
        let v_lo = frame.pixel_to_mm(0, y0 + y_origin)[1] - reach;
        let v_hi = frame.pixel_to_mm(0, (y1 - 1) + y_origin)[1] + reach;
        let iy_lo = (v_lo * s).floor() as i32;
        let iy_hi = (v_hi * s).floor() as i32;
        let u_lo = frame.pixel_to_mm(0, 0)[0] - reach;
        let u_hi = frame.pixel_to_mm(src.w - 1, 0)[0] + reach;
        let ix_lo = (u_lo * s).floor() as i32;
        let ix_hi = (u_hi * s).floor() as i32;
        // Supersampled sub-positions sit up to half a pixel beyond the
        // pixel centers — widen the cache so their windows are covered
        // (missing cells here would show up as strip-edge seams).
        let extra = (0.5 * mmpp * s).ceil() as i32;
        let ix_lo = ix_lo - extra;
        let ix_hi = ix_hi + extra;
        let iy_lo = iy_lo - extra;
        let iy_hi = iy_hi + extra;
        let cols = (ix_hi - ix_lo + 1) as usize;
        let rows = (iy_hi - iy_lo + 1) as usize;
        let absent = GrainCell {
            present: false,
            cx: 0.0,
            cy: 0.0,
            r: 0.0,
            amp: [0.0; 3],
        };
        let mut cache = vec![absent; cols * rows];
        for (k, iy) in (iy_lo..=iy_hi).enumerate() {
            for (j, ix) in (ix_lo..=ix_hi).enumerate() {
                let cu = (ix as f64 + 0.5) * self.cell_mm;
                let cv = (iy as f64 + 0.5) * self.cell_mm;
                let fx = (cu - crate::frame::FRAME_LONG_MM / 2.0) / mmpp + f64::from(src.w) / 2.0;
                let fy_abs =
                    (cv - crate::frame::FRAME_SHORT_MM / 2.0) / mmpp + f64::from(image_h) / 2.0;
                // Grain centers outside the image frame read zero density
                // — identical in monolithic and tiled renders.
                let d_lum = if fy_abs < 0.0 || fy_abs > f64::from(image_h) - 1.0 {
                    0.0
                } else {
                    let fy = fy_abs - f64::from(y_origin);
                    let d = src.bilinear(fx, fy);
                    0.25 * d[0] + 0.5 * d[1] + 0.25 * d[2]
                };
                let sigma = sigma_at(&self.sigma_curve, d_lum);
                if sigma <= 0.0 {
                    continue; // absent in cache ⇒ skipped at gather too
                }
                cache[k * cols + j] = self.cell(ix, iy, sigma);
            }
        }
        // -- Gather per pixel ---------------------------------------------
        // Pixel-area supersampling: a pixel is the *area average* of the
        // continuous field (that is what a sensor/scanner pixel sees).
        // Sub-sampling step ≈ the calibration scale (6 µm) so coarse
        // previews reproduce the aperture statistics of native renders
        // (grain.md §3 zoom-consistency); k = 1 at native scale — no extra
        // cost for exports. Cap k = 4 to bound worst-case preview cost
        // (interactive fast path is the spec'd answer beyond that).
        let k = ((mmpp / 0.006).round() as u32).clamp(1, 4);
        let sub_step = mmpp / f64::from(k);
        let (cix_lo, ciy_lo) = (ix_lo, iy_lo);
        for y in y0..y1 {
            for x in 0..src.w {
                let [uc, vc] = frame.pixel_to_mm(x, y + y_origin);
                let mut out = [0.0f64; 3];
                for sj in 0..k {
                    for si in 0..k {
                        let u = uc + (f64::from(si) + 0.5) * sub_step - 0.5 * mmpp;
                        let v = vc + (f64::from(sj) + 0.5) * sub_step - 0.5 * mmpp;
                        let pix_ix_lo = ((u - reach) * s).floor() as i32;
                        let pix_ix_hi = ((u + reach) * s).floor() as i32;
                        let pix_iy_lo = ((v - reach) * s).floor() as i32;
                        let pix_iy_hi = ((v + reach) * s).floor() as i32;
                        for iy in pix_iy_lo.max(ciy_lo)..=pix_iy_hi {
                            let kr = (iy - ciy_lo) as usize;
                            if kr >= rows {
                                continue;
                            }
                            for ix in pix_ix_lo.max(cix_lo)..=pix_ix_hi {
                                let jr = (ix - cix_lo) as usize;
                                if jr >= cols {
                                    continue;
                                }
                                let cell = &cache[kr * cols + jr];
                                if !cell.present {
                                    continue;
                                }
                                let du = u - cell.cx;
                                let dv = v - cell.cy;
                                let dist2 = du * du + dv * dv;
                                if dist2 >= cell.r * cell.r {
                                    continue;
                                }
                                let t = dist2.sqrt() / cell.r;
                                let w = bump(t);
                                out[0] += w * cell.amp[0];
                                out[1] += w * cell.amp[1];
                                out[2] += w * cell.amp[2];
                            }
                        }
                    }
                }
                let sub_n = f64::from(k * k);
                let mut d = src.px64(x, y);
                for c in 0..3 {
                    d[c] += out[c] / sub_n;
                    if d[c] < 0.0 {
                        d[c] = 0.0; // never-negative densities (§D)
                    }
                }
                dst.set3(x, y, d);
            }
        }
    }
}

/// Calibration anchor density (documented v1 convention: middle of the
/// working density range).
pub const D_CAL: f64 = 1.0;

#[derive(Clone)]
struct GrainCell {
    present: bool,
    cx: f64,
    cy: f64,
    r: f64,
    amp: [f64; 3],
}

/// σ(D) lookup with flat extrapolation at the ends.
fn sigma_at(curve: &SigmaCurve, d: f64) -> f64 {
    let xs = &curve.density;
    let ys = &curve.sigma;
    if xs.is_empty() {
        return 0.0;
    }
    if d <= xs[0] {
        return ys[0];
    }
    if d >= xs[xs.len() - 1] {
        return ys[ys.len() - 1];
    }
    // Linear scan is fine (curves are short and this is per-grain-center).
    for i in 0..xs.len() - 1 {
        if d < xs[i + 1] {
            let t = (d - xs[i]) / (xs[i + 1] - xs[i]);
            return ys[i] * (1.0 - t) + ys[i + 1] * t;
        }
    }
    ys[ys.len() - 1]
}

/// Box–Muller pair (cos, sin) — both normals used.
fn normal_pair(rs: &mut RandStream) -> (f64, f64) {
    let u1 = rs.next_f64().max(2.0e-9);
    let u2 = rs.next_f64();
    let r = (-2.0 * u1.ln()).sqrt();
    let a = core::f64::consts::TAU * u2;
    (r * a.cos(), r * a.sin())
}

/// Cholesky factor of a 3×3 symmetric PSD matrix (row-major, lower
/// triangular: `L[i][k] = out[3i+k]`, `A = L·Lᵀ`).
fn cholesky3(m: &crate::profile::Mat3) -> [f64; 9] {
    let a = m.0;
    let mut l = [0.0f64; 9];
    // Row 0.
    l[0] = a[0].max(0.0).sqrt();
    // Row 1: L10 = A10 / L00, L11 = sqrt(A11 − L10²).
    if l[0] > 0.0 {
        l[3] = a[3] / l[0];
    }
    l[4] = (a[4] - l[3] * l[3]).max(0.0).sqrt();
    // Row 2: L20 = A20 / L00, L21 = (A21 − L20·L10)/L11,
    // L22 = sqrt(A22 − L20² − L21²).
    if l[0] > 0.0 {
        l[6] = a[6] / l[0];
    }
    if l[4] > 0.0 {
        l[7] = (a[7] - l[6] * l[3]) / l[4];
    }
    l[8] = (a[8] - l[6] * l[6] - l[7] * l[7]).max(0.0).sqrt();
    l
}

impl GrainParams {
    /// Sanity: σ(D) curve must be non-negative (validated in the profile);
    /// helper for tooling.
    pub fn sigma_at(&self, d: f64) -> f64 {
        sigma_at(&self.sigma_curve, d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::testing::test_profile;

    #[test]
    fn cholesky_reconstructs_correlation() {
        let rho = test_profile().grain.rho;
        let l = cholesky3(&rho);
        // L·Lᵀ == rho.
        for i in 0..3 {
            for j in 0..3 {
                let mut v = 0.0;
                for k in 0..3 {
                    v += l[3 * i + k] * l[3 * j + k];
                }
                assert!(
                    (v - rho.0[3 * i + j]).abs() < 1e-12,
                    "({i},{j}): {v} vs {}",
                    rho.0[3 * i + j]
                );
            }
        }
    }

    #[test]
    fn calibration_hits_target_sigma() {
        // After resolve, the aperture σ at D_CAL must match σ_curve(D_CAL)
        // exactly (one-step exact rescale — linearity).
        let p = test_profile();
        let f = GrainField::resolve(&p, 1.0, None);
        assert!(f.enabled);
        let target = sigma_at(&p.grain.sigma_curve, D_CAL);
        let measured = f.aperture_sigma_on_uniform(D_CAL);
        for c in 0..3 {
            assert!(
                (measured[c] - target).abs() / target < 1e-9,
                "layer {c}: {measured:?} vs {target}"
            );
        }
    }

    #[test]
    fn grain_is_deterministic_same_seed() {
        let p = test_profile();
        let f1 = GrainField::resolve(&p, 1.0, Some(42));
        let f2 = GrainField::resolve(&p, 1.0, Some(42));
        let f3 = GrainField::resolve(&p, 1.0, Some(43));
        let mut nonzero_1 = 0usize;
        let mut differing = 0usize;
        // A grid of sample points: determinism must hold everywhere, and
        // the two seeds must produce genuinely different fields somewhere.
        for gy in 0..24 {
            for gx in 0..24 {
                let u = 3.0 + gx as f64 * 0.037;
                let v = 2.0 + gy as f64 * 0.053;
                let a = f1.eval_raw(u, v, 1.0);
                let b = f2.eval_raw(u, v, 1.0);
                let c = f3.eval_raw(u, v, 1.0);
                assert_eq!(a, b, "same seed must be bit-exact at ({u},{v})");
                if a != [0.0, 0.0, 0.0] {
                    nonzero_1 += 1;
                }
                if a != c {
                    differing += 1;
                }
            }
        }
        assert!(nonzero_1 > 24, "field must be non-empty (got {nonzero_1})");
        assert!(
            differing > 24,
            "different seed must give a different field ({differing}/576 points differ)"
        );
    }

    #[test]
    fn grain_applies_without_seams() {
        // Strip application must equal full application (tile contract).
        let p = test_profile();
        let f = GrainField::resolve(&p, 1.0, Some(7));
        let w = 96u32;
        let h = 64u32;
        let frame = FilmFrame::from_dims(w, h, crate::frame::CropRule::FitWidth);
        let src = Plane {
            w,
            h,
            data: vec![1.0; (w * h * 3) as usize],
        };
        let mut full = Plane::new(w, h);
        f.apply(&src, &mut full, 0, h, &frame, 0, h);
        let mut tiled = Plane::new(w, h);
        for y0 in (0..h).step_by(8) {
            let y1 = (y0 + 8).min(h);
            f.apply(&src, &mut tiled, y0, y1, &frame, 0, h);
        }
        for i in 0..(w * h * 3) as usize {
            assert_eq!(full.data[i], tiled.data[i], "index {i} differs");
        }
    }

    #[test]
    fn intensity_zero_disables() {
        let p = test_profile();
        let f = GrainField::resolve(&p, 0.0, None);
        assert!(!f.enabled);
    }
}
