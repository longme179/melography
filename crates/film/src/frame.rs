//! Film-plane coordinate frame (FILM_MODEL §0).
//!
//! All spatial film stages (halation, grain, optics) live in physical film
//! coordinates: a 36×24 mm full-frame equivalent rectangle. The mapping from
//! pixels to millimetres is derived from the **processed image dimensions**
//! (not the original file), which makes the field resolution-independent by
//! construction: a full-resolution render and a fit-to-window preview cover
//! the same film area, so evaluating a stage at a pixel's `(u, v)` millimetre
//! position samples the same continuous field.
//!
//! Aspect ratios ≠ 3:2 pick a crop rule (profile-declared, default
//! `fit-width`): the film frame's long/short edge is aligned to the image
//! width/height and the frame extends conceptually beyond the image in the
//! other axis. Because grain/halation fields are defined over unbounded
//! film coordinates, "extending" only affects reported coordinates, never
//! evaluation.

use serde::{Deserialize, Serialize};

/// Long edge of the reference film frame (mm) — 35 mm stills.
pub const FRAME_LONG_MM: f64 = 36.0;
/// Short edge of the reference film frame (mm).
pub const FRAME_SHORT_MM: f64 = 24.0;

/// Crop rule for aspect ratios ≠ 3:2 (FILM_MODEL §0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CropRule {
    /// Film width (36 mm) maps to image width (default).
    #[default]
    FitWidth,
    /// Film height (24 mm) maps to image height.
    FitHeight,
}

/// Pixel ↔ film-millimetre mapping for one render.
///
/// `mm_per_pixel` is the physical scale; the image is anchored so its center
/// sits at the film-frame center (`anchor-center` semantics of FILM_MODEL
/// §0). All math is affine f64 — bit-identical across platforms (S2
/// discipline: f64 core, no transcendentals).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilmFrame {
    /// Millimetres per pixel (derived from the crop rule).
    pub mm_per_pixel: f64,
    /// Image half-extent in pixels (captured at construction for the
    /// center anchor; private to keep the value coherent with the rule).
    half_px: [f64; 2],
    crop: CropRule,
}

impl FilmFrame {
    /// Derive the frame for an image of `width × height` pixels under the
    /// given crop rule.
    pub fn from_dims(width: u32, height: u32, crop: CropRule) -> Self {
        let w = f64::from(width.max(1));
        let h = f64::from(height.max(1));
        let mm_per_pixel = match crop {
            CropRule::FitWidth => FRAME_LONG_MM / w,
            CropRule::FitHeight => FRAME_SHORT_MM / h,
        };
        Self {
            mm_per_pixel,
            half_px: [w / 2.0, h / 2.0],
            crop,
        }
    }

    /// Crop rule this frame was built with.
    #[inline]
    pub fn crop_rule(&self) -> CropRule {
        self.crop
    }

    /// Film-plane position (mm) of the center of pixel `(x, y)`.
    #[inline]
    pub fn pixel_to_mm(&self, x: u32, y: u32) -> [f64; 2] {
        self.corner_to_mm(f64::from(x) + 0.5, f64::from(y) + 0.5)
    }

    /// Film-plane position (mm) of a **corner-coordinate** point
    /// `(fx, fy)` (pixel index as a continuous quantity — e.g. `fx = 2i+1`
    /// is the center of the 2-pixel region `[2i, 2i+2)`). This is the
    /// exact-correspondence primitive for downscaled previews.
    #[inline]
    pub fn corner_to_mm(&self, fx: f64, fy: f64) -> [f64; 2] {
        [
            FRAME_LONG_MM / 2.0 + (fx - self.half_px[0]) * self.mm_per_pixel,
            FRAME_SHORT_MM / 2.0 + (fy - self.half_px[1]) * self.mm_per_pixel,
        ]
    }

    /// Convert a length in millimetres to pixels at this scale (ceil —
    /// halo margins must round *out*).
    #[inline]
    pub fn mm_to_px_ceil(&self, mm: f64) -> u32 {
        (mm / self.mm_per_pixel).ceil().max(0.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_width_maps_36mm_to_width() {
        let f = FilmFrame::from_dims(6000, 4000, CropRule::FitWidth);
        assert!((f.mm_per_pixel - 36.0 / 6000.0).abs() < 1e-12); // 6 µm/px
    }

    #[test]
    fn fit_height_maps_24mm_to_height() {
        let f = FilmFrame::from_dims(4000, 6000, CropRule::FitHeight);
        assert!((f.mm_per_pixel - 24.0 / 6000.0).abs() < 1e-12);
    }

    #[test]
    fn center_pixel_maps_to_frame_center() {
        let f = FilmFrame::from_dims(101, 51, CropRule::FitWidth);
        let [u, v] = f.pixel_to_mm(50, 25);
        assert!((u - 18.0).abs() < 1e-9);
        assert!((v - 12.0).abs() < 1e-9);
    }

    #[test]
    fn scale_is_resolution_invariant_per_film_area() {
        // A 2× downscale covers the same film area: mm/px doubles, and a
        // half-res pixel center corresponds exactly to the center of its
        // 2×2 source region (corner coords 2i+1, 2j+1).
        let full = FilmFrame::from_dims(6000, 4000, CropRule::FitWidth);
        let half = FilmFrame::from_dims(3000, 2000, CropRule::FitWidth);
        assert!((half.mm_per_pixel - 2.0 * full.mm_per_pixel).abs() < 1e-12);
        let [u1, v1] = full.corner_to_mm(3001.0, 2001.0);
        let [u2, v2] = half.pixel_to_mm(1500, 1000);
        assert!((u1 - u2).abs() < 1e-9 && (v1 - v2).abs() < 1e-9);
    }

    #[test]
    fn mm_to_px_ceil_rounds_out() {
        let f = FilmFrame::from_dims(3600, 2400, CropRule::FitWidth); // 0.01 mm/px
        assert_eq!(f.mm_to_px_ceil(0.05), 5);
        assert_eq!(f.mm_to_px_ceil(0.051), 6); // rounds out
        assert_eq!(f.mm_to_px_ceil(0.0), 0);
    }
}
