//! Stage B — lens & capture optics (FILM_MODEL §B).
//!
//! Light passes the lens **before** the emulsion, so these stages run
//! first on scene-referred exposure `E` (AP1-linear f32 in, f32 out).
//!
//! Shipped v1: vignetting (cos^k falloff) and veiling glare (broadband
//! gaussian bloom — *distinct* from halation: broadband, lens-stage,
//! vs red-biased, emulsion-stage).
//!
//! Declared but honestly unimplemented v1: chromatic aberration and
//! distortion (`[ART, default off]` in the spec) — enabling them is a
//! clear parameter error, never a silent no-op.

use crate::frame::FilmFrame;
use serde::{Deserialize, Serialize};

/// User-facing lens parameters (recipe-level, not stock-level — the lens
/// is not part of the emulsion).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LensParams {
    /// Vignetting amount 0..1 (0 = off). Maps to the cos exponent
    /// `k = 4·amount`.
    pub vignette_amount: f64,
    /// Horizontal field of view (degrees) shaping the falloff angle.
    /// Default 54° ≈ a 35 mm lens on full frame.
    pub lens_fov_deg: f64,
    /// Veiling glare amount 0..1 (0 = off). Mixes a wide gaussian bloom
    /// into the exposure: `E ← (1−s)·E + s·(G * E)` with `s = 0.3·amount`.
    pub veiling_amount: f64,
    /// Bloom gaussian σ on the film plane (mm).
    pub veiling_sigma_mm: f64,
    /// Chromatic aberration `[ART, default off]` — declared, rejected if
    /// enabled (honest unimplemented).
    pub chromatic_aberration: f64,
    /// Distortion `[ART, default off]` — declared, rejected if enabled.
    pub distortion: f64,
}

impl Default for LensParams {
    fn default() -> Self {
        Self {
            vignette_amount: 0.0,
            lens_fov_deg: 54.0,
            veiling_amount: 0.0,
            veiling_sigma_mm: 1.5,
            chromatic_aberration: 0.0,
            distortion: 0.0,
        }
    }
}

impl LensParams {
    /// Reject enabled-but-unimplemented `[ART]` features loudly.
    pub fn validate(&self) -> Result<(), String> {
        for (name, v, limit) in [
            ("vignette_amount", self.vignette_amount, 1.0),
            ("veiling_amount", self.veiling_amount, 1.0),
        ] {
            if !(v.is_finite() && (0.0..=limit).contains(&v)) {
                return Err(format!("lens.{name} must be within [0, {limit}]"));
            }
        }
        if !(self.lens_fov_deg.is_finite() && (20.0..=120.0).contains(&self.lens_fov_deg)) {
            return Err("lens.lens_fov_deg must be within [20, 120] degrees".into());
        }
        if !(self.veiling_sigma_mm.is_finite() && self.veiling_sigma_mm > 0.0) {
            return Err("lens.veiling_sigma_mm must be > 0".into());
        }
        if self.chromatic_aberration != 0.0 {
            return Err(
                "lens.chromatic_aberration is declared but not implemented in v1 — \
                 refuse to silently no-op (FILM_MODEL §B [ART, default off])"
                    .into(),
            );
        }
        if self.distortion != 0.0 {
            return Err("lens.distortion is declared but not implemented in v1 — \
                 refuse to silently no-op (FILM_MODEL §B [ART, default off])"
                .into());
        }
        Ok(())
    }
}

/// Separable gaussian convolution kernel, unit DC gain (FILM_MODEL §C2's
/// energy-accounting convention: a constant field maps to itself, so the
/// stage gain is exactly `α`).
///
/// Determinism: taps are f64, computed in a fixed order, truncated at
/// `ceil(3σ)` — bit-identical across platforms (S2 discipline).
#[derive(Debug, Clone, PartialEq)]
pub struct GaussianKernel {
    pub(crate) taps: Vec<f64>,
    /// Truncation radius in pixels (the halo margin this kernel needs).
    pub radius_px: u32,
}

impl GaussianKernel {
    /// Build a 1D unit-DC-gain gaussian of `sigma_px` (σ < 0.05 px is the
    /// identity — no work, no margin).
    pub fn new(sigma_px: f64) -> Self {
        if !(sigma_px.is_finite() && sigma_px >= 0.05) {
            return Self {
                taps: vec![1.0],
                radius_px: 0,
            };
        }
        let radius = (3.0 * sigma_px).ceil() as i64;
        let mut taps = Vec::with_capacity((2 * radius + 1) as usize);
        let two_sig2 = 2.0 * sigma_px * sigma_px;
        for i in -radius..=radius {
            let d = i as f64;
            taps.push((-(d * d) / two_sig2).exp());
        }
        let sum: f64 = taps.iter().sum();
        for t in &mut taps {
            *t /= sum;
        }
        Self {
            taps,
            radius_px: radius as u32,
        }
    }

    /// Build from precomputed (already unit-DC-normalized) taps — used by
    /// the halation mixture kernels (emulsion stage).
    pub(crate) fn from_taps(taps: Vec<f64>, radius_px: u32) -> Self {
        Self { taps, radius_px }
    }

    /// The identity kernel?
    pub fn is_identity(&self) -> bool {
        self.radius_px == 0
    }

    /// Horizontal pass: `out[y][x] = Σ_i taps[i] · in[y][x+i−radius]`
    /// (zero padding outside the frame — no data exists there; honest
    /// and tile-safe). Only rows `[y0, y1)` of `out` are written.
    pub fn conv_h(&self, src: &Plane, dst: &mut Plane, y0: u32, y1: u32) {
        let r = self.radius_px as i64;
        let w = src.w as i64;
        for y in y0..y1 {
            for x in 0..src.w {
                let mut acc = [0.0f64; 3];
                for (ti, t) in self.taps.iter().enumerate() {
                    let sx = x as i64 + ti as i64 - r;
                    if sx < 0 || sx >= w {
                        continue; // zero padding
                    }
                    let p = src.px(sx as u32, y);
                    acc[0] += t * p[0] as f64;
                    acc[1] += t * p[1] as f64;
                    acc[2] += t * p[2] as f64;
                }
                dst.set3(x, y, acc);
            }
        }
    }

    /// Vertical pass (same contract; margins from the horizontal pass must
    /// already be materialized in `src`).
    pub fn conv_v(&self, src: &Plane, dst: &mut Plane, y0: u32, y1: u32) {
        let r = self.radius_px as i64;
        let h = src.h as i64;
        for y in y0..y1 {
            for x in 0..src.w {
                let mut acc = [0.0f64; 3];
                for (ti, t) in self.taps.iter().enumerate() {
                    let sy = y as i64 + ti as i64 - r;
                    if sy < 0 || sy >= h {
                        continue;
                    }
                    let p = src.px(x, sy as u32);
                    acc[0] += t * p[0] as f64;
                    acc[1] += t * p[1] as f64;
                    acc[2] += t * p[2] as f64;
                }
                dst.set3(x, y, acc);
            }
        }
    }
}

/// A 3-channel f32 image plane with f64-accumulation setters
/// (storage f32, compute f64 — the M1 lut-crate discipline).
#[derive(Debug, Clone)]
pub struct Plane {
    pub w: u32,
    pub h: u32,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            data: vec![0.0; (w as usize) * (h as usize) * 3],
        }
    }

    #[inline]
    pub fn px(&self, x: u32, y: u32) -> [f32; 3] {
        let i = ((y as usize) * (self.w as usize) + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Read as f64 triple (compute precision).
    #[inline]
    pub fn px64(&self, x: u32, y: u32) -> [f64; 3] {
        let p = self.px(x, y);
        [p[0] as f64, p[1] as f64, p[2] as f64]
    }

    #[inline]
    pub fn set3(&mut self, x: u32, y: u32, v: [f64; 3]) {
        let i = ((y as usize) * (self.w as usize) + x as usize) * 3;
        self.data[i] = v[0] as f32;
        self.data[i + 1] = v[1] as f32;
        self.data[i + 2] = v[2] as f32;
    }

    /// Bilinear sample in corner coordinates (used by the grain lattice to
    /// read density at off-pixel grain centers). Zero outside the plane.
    pub fn bilinear(&self, fx: f64, fy: f64) -> [f64; 3] {
        if !(fx >= 0.0 && fy >= 0.0 && fx <= self.w as f64 - 1.0 && fy <= self.h as f64 - 1.0) {
            return [0.0; 3];
        }
        let x0 = fx.floor() as u32;
        let y0 = fy.floor() as u32;
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let tx = fx - f64::from(x0);
        let ty = fy - f64::from(y0);
        let p00 = self.px64(x0, y0);
        let p10 = self.px64(x1, y0);
        let p01 = self.px64(x0, y1);
        let p11 = self.px64(x1, y1);
        let mut out = [0.0f64; 3];
        for c in 0..3 {
            let top = p00[c] * (1.0 - tx) + p10[c] * tx;
            let bot = p01[c] * (1.0 - tx) + p11[c] * tx;
            out[c] = top * (1.0 - ty) + bot * ty;
        }
        out
    }
}

/// Apply vignetting to rows `[y0, y1)` (per-pixel, no margin). In-place
/// variant over a strip-local plane: `y_origin` is the absolute image row
/// of the plane's local row 0 (film coordinates stay absolute).
pub fn apply_vignette(
    src: &Plane,
    dst: &mut Plane,
    y0: u32,
    y1: u32,
    y_origin: u32,
    lens: &LensParams,
    frame: &FilmFrame,
) {
    let half_diag_mm = (crate::frame::FRAME_LONG_MM * crate::frame::FRAME_LONG_MM / 4.0
        + crate::frame::FRAME_SHORT_MM * crate::frame::FRAME_SHORT_MM / 4.0)
        .sqrt();
    let half_fov_rad = lens.lens_fov_deg.to_radians() / 2.0;
    let k = 4.0 * lens.vignette_amount;
    for y in y0..y1 {
        for x in 0..src.w {
            let e = src.px64(x, y);
            // cos^k of the view angle at this film position.
            let [u, v] = frame.pixel_to_mm(x, y + y_origin);
            let du = u - crate::frame::FRAME_LONG_MM / 2.0;
            let dv = v - crate::frame::FRAME_SHORT_MM / 2.0;
            let r_n = (du * du + dv * dv).sqrt() / half_diag_mm; // 0..~1
            let theta = r_n * half_fov_rad;
            let falloff = theta.cos().powf(k); // 1 at center
            dst.set3(x, y, [e[0] * falloff, e[1] * falloff, e[2] * falloff]);
        }
    }
}

/// In-place vignette (per-pixel read-then-write on the same plane).
pub fn apply_vignette_inplace(
    plane: &mut Plane,
    y0: u32,
    y1: u32,
    y_origin: u32,
    lens: &LensParams,
    frame: &FilmFrame,
) {
    let half_diag_mm = (crate::frame::FRAME_LONG_MM * crate::frame::FRAME_LONG_MM / 4.0
        + crate::frame::FRAME_SHORT_MM * crate::frame::FRAME_SHORT_MM / 4.0)
        .sqrt();
    let half_fov_rad = lens.lens_fov_deg.to_radians() / 2.0;
    let k = 4.0 * lens.vignette_amount;
    for y in y0..y1 {
        for x in 0..plane.w {
            let e = plane.px64(x, y);
            let [u, v] = frame.pixel_to_mm(x, y + y_origin);
            let du = u - crate::frame::FRAME_LONG_MM / 2.0;
            let dv = v - crate::frame::FRAME_SHORT_MM / 2.0;
            let r_n = (du * du + dv * dv).sqrt() / half_diag_mm;
            let theta = r_n * half_fov_rad;
            let falloff = theta.cos().powf(k);
            plane.set3(x, y, [e[0] * falloff, e[1] * falloff, e[2] * falloff]);
        }
    }
}

/// Mix the veiling bloom back onto the sharp exposure (per-pixel, no
/// margin): `E ← (1−s)·E_sharp + s·E_bloom`. The runner materializes the
/// bloom (gaussian blur of the vignetted exposure) with proper margins
/// before calling this.
pub fn apply_veiling_mix(
    sharp: &Plane,
    bloom: &Plane,
    dst: &mut Plane,
    y0: u32,
    y1: u32,
    veiling_amount: f64,
) {
    let s = 0.3 * veiling_amount;
    for y in y0..y1 {
        for x in 0..sharp.w {
            let e = sharp.px64(x, y);
            let b = bloom.px64(x, y);
            dst.set3(
                x,
                y,
                [
                    (1.0 - s) * e[0] + s * b[0],
                    (1.0 - s) * e[1] + s * b[1],
                    (1.0 - s) * e[2] + s * b[2],
                ],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_unit_dc_gain() {
        for sigma in [0.5f64, 1.7, 4.2] {
            let k = GaussianKernel::new(sigma);
            let sum: f64 = k.taps.iter().sum();
            assert!((sum - 1.0).abs() < 1e-12, "σ={sigma}: sum={sum}");
        }
    }

    #[test]
    fn tiny_sigma_is_identity() {
        let k = GaussianKernel::new(0.01);
        assert!(k.is_identity());
        assert_eq!(k.taps, vec![1.0]);
    }

    #[test]
    fn convolution_preserves_constant_field() {
        // Unit DC gain: a constant plane maps to itself (energy accounting).
        let w = 33;
        let src = Plane {
            w,
            h: 33,
            data: vec![0.5; (w * w * 3) as usize],
        };
        let mut tmp = Plane::new(w, 33);
        let mut dst = Plane::new(w, 33);
        let k = GaussianKernel::new(2.0); // radius 6
        k.conv_h(&src, &mut tmp, 0, 33);
        k.conv_v(&tmp, &mut dst, 0, 33);
        // Interior only: beyond the zero-padded border (radius 6).
        for y in 8..25 {
            for x in 8..25 {
                let p = dst.px(x, y);
                for c in p {
                    assert!((c - 0.5).abs() < 1e-6, "interior must stay 0.5");
                }
            }
        }
    }

    #[test]
    fn convolution_impulse_energy_is_one() {
        // Σ taps = 1 ⇒ impulse response sums to 1 (per channel).
        let w = 32;
        let mut src = Plane::new(w, w);
        src.set3(16, 16, [1.0, 1.0, 1.0]);
        let mut tmp = Plane::new(w, w);
        let mut dst = Plane::new(w, w);
        let k = GaussianKernel::new(1.5);
        k.conv_h(&src, &mut tmp, 0, w);
        k.conv_v(&tmp, &mut dst, 0, w);
        let mut sum = [0.0f64; 3];
        for y in 0..w {
            for x in 0..w {
                let p = dst.px64(x, y);
                sum[0] += p[0];
                sum[1] += p[1];
                sum[2] += p[2];
            }
        }
        for c in sum {
            assert!((c - 1.0).abs() < 1e-9, "impulse energy {c}");
        }
    }

    #[test]
    fn rejects_enabled_unimplemented_art() {
        let mut p = LensParams::default();
        assert!(p.validate().is_ok());
        p.chromatic_aberration = 0.2;
        assert!(p.validate().is_err());
        p.chromatic_aberration = 0.0;
        p.distortion = 0.1;
        assert!(p.validate().is_err());
    }

    #[test]
    fn vignette_darkens_corners_not_center() {
        let frame = FilmFrame::from_dims(64, 64, crate::frame::CropRule::FitWidth);
        let src = Plane {
            w: 64,
            h: 64,
            data: vec![1.0; 64 * 64 * 3],
        };
        let mut dst = Plane::new(64, 64);
        let lens = LensParams {
            vignette_amount: 1.0,
            ..Default::default()
        };
        apply_vignette(&src, &mut dst, 0, 64, 0, &lens, &frame);
        let center = dst.px(32, 32);
        let corner = dst.px(1, 1);
        assert!(center[0] > 0.99);
        assert!(corner[0] < center[0], "corner must be darker");
    }

    #[test]
    fn veiling_mix_blends_bloom() {
        let sharp = Plane {
            w: 4,
            h: 4,
            data: vec![1.0; 4 * 4 * 3],
        };
        let bloom = Plane {
            w: 4,
            h: 4,
            data: vec![0.0; 4 * 4 * 3],
        };
        let mut dst = Plane::new(4, 4);
        apply_veiling_mix(&sharp, &bloom, &mut dst, 0, 4, 1.0);
        let p = dst.px(2, 2);
        // s = 0.3 → 0.7·1 + 0.3·0 = 0.7
        assert!((p[0] - 0.7).abs() < 1e-12);
    }
}
