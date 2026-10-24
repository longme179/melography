//! Film chain composition (stages A–I) and the strip executor.
//!
//! Stage order (FILM_MODEL §A–§I):
//!
//! ```text
//! input (AP1-linear RGB f32)
//!   → A/B   vignette + veiling bloom        (per-pixel / gaussian margin)
//!   → C1,C3 excitation M_exc + EV + anchor  (per-pixel)
//!   → C2    halation                         (gaussian margin)
//!   → D/E   H&D response + chemistry         (per-pixel)
//!   → G     grain                            (lattice margin)
//!   → F     print/scan inversion             (per-pixel)
//!   → AP1-linear output (I: export stage owns the final transform)
//! ```
//!
//! **Tiling model.** For an output strip `[y, y1)` the row regions each
//! stage must cover are computed top-down from declared margins:
//!
//! ```text
//! r1 = expand(strip, m_grain)        // density rows (response output)
//! r2 = expand(r1,    m_halation)     // excitation rows
//! r3 = expand(r2,    m_veiling)      // vignetted rows
//! ```
//!
//! Every stage then runs over exactly its region with the same fixed tap
//! windows and summation order as any other tiling — byte-identical to
//! the monolithic render by construction (verified by the §L gate).
//!
//! **Memory.** Three strip-local planes (A/B/T) sized
//! `(strip_h + 2·m_total)` rows — ~44 MB each at 6000 px width — plus the
//! input/output images. Peak ≈ input + output + 3 planes, comfortably
//! inside the ≤ 2 GiB @ 50 MP budget (R-REN-2).

use crate::emulsion::{self, Adjustments, Emulsion};
use crate::frame::FilmFrame;
use crate::grain::GrainField;
use crate::optics::{self, GaussianKernel, LensParams, Plane};
use crate::print::Print;
use crate::profile::StockProfile;
use melography_core::{Channels, ImageF32};

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("film chain: {0}")]
    BadParams(String),
    #[error("film chain: input must be RGB, got {0:?}")]
    NotRgb(Channels),
}

/// Everything a render needs: validated profile + adjustments + lens.
#[derive(Clone)]
pub struct FilmChainParams {
    pub profile: StockProfile,
    pub adjustments: Adjustments,
    pub lens: LensParams,
}

impl FilmChainParams {
    pub fn new(profile: StockProfile) -> Result<Self, ChainError> {
        profile
            .validate()
            .map_err(|e| ChainError::BadParams(e.to_string()))?;
        let p = Self {
            profile,
            adjustments: Adjustments::default(),
            lens: LensParams::default(),
        };
        p.validate()?;
        Ok(p)
    }

    pub fn with(mut self, adjustments: Adjustments) -> Result<Self, ChainError> {
        self.adjustments = adjustments;
        self.validate()?;
        Ok(self)
    }

    /// Full recipe-level parameterization (adjustments + lens).
    pub fn with_adjustments(
        mut self,
        adjustments: Adjustments,
        lens: LensParams,
    ) -> Result<Self, ChainError> {
        self.adjustments = adjustments;
        self.lens = lens;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ChainError> {
        self.profile
            .validate()
            .map_err(|e| ChainError::BadParams(e.to_string()))?;
        self.adjustments.validate().map_err(ChainError::BadParams)?;
        self.lens.validate().map_err(ChainError::BadParams)?;
        Ok(())
    }
}

/// Render the film chain monolithically (single strip).
pub fn render(img: &ImageF32, params: &FilmChainParams) -> Result<ImageF32, ChainError> {
    render_tiled(img, params, img.height())
}

/// Render in horizontal strips of `strip_h` output rows (byte-identical to
/// `render` for any `strip_h ≥ 1` — the tiled-vs-monolithic gate verifies).
pub fn render_tiled(
    img: &ImageF32,
    params: &FilmChainParams,
    strip_h: u32,
) -> Result<ImageF32, ChainError> {
    params.validate()?;
    if img.channels() != Channels::Rgb {
        return Err(ChainError::NotRgb(img.channels()));
    }
    if !params.adjustments.enabled {
        return Ok(img.clone());
    }
    let (w, h) = (img.width(), img.height());
    let frame = FilmFrame::from_dims(w, h, params.profile.crop_rule);
    let mmpp = frame.mm_per_pixel;

    // Resolve stages once per render.
    let em: Emulsion = emulsion::resolve_emulsion(&params.profile, &params.adjustments, mmpp);
    let grain = GrainField::resolve(
        &params.profile,
        params.adjustments.grain_intensity,
        params.adjustments.grain_seed,
    );
    let print = Print::resolve(
        &params.profile,
        params.adjustments.inversion_path,
        params.adjustments.print_exposure_stops,
    );
    let veiling_on = params.lens.veiling_amount > 0.0;
    let veiling = GaussianKernel::new(params.lens.veiling_sigma_mm / mmpp);
    let veiling_on = veiling_on && !veiling.is_identity();

    let m_veiling = if veiling_on { veiling.radius_px } else { 0 };
    let m_halation = em.input_margin();
    let m_grain = grain.margin_px(mmpp);
    let m_total = m_veiling + m_halation + m_grain;

    let strip_h = strip_h.clamp(1, h);
    let buf_rows = (strip_h + 2 * m_total).clamp(1, h);
    let mut plane_a = Plane::new(w, buf_rows);
    let mut plane_b = Plane::new(w, buf_rows);
    let mut plane_t = Plane::new(w, buf_rows);

    let mut out_img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);

    let mut strip_y = 0u32;
    while strip_y < h {
        let strip_y1 = (strip_y + strip_h).min(h);
        // Row regions (absolute, clamped): each stage's coverage derives
        // top-down from the margins of the stages above it.
        let r1 = Region::expand(strip_y, strip_y1, m_grain, h); // density
        let r2 = Region::expand(r1.y0, r1.y1, m_halation, h); // excitation
        let r3 = Region::expand(r2.y0, r2.y1, m_veiling, h); // vignetted
        let y3 = r3.y0; // local row 0 of every strip plane

        // -- Vignette (B) over r3 into plane_a (in place) ---------------------
        for (ly, ay) in (r3.y0..r3.y1).enumerate() {
            for x in 0..w {
                let p = img.pixel(x, ay);
                plane_a.set3(
                    x,
                    ly as u32,
                    [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
                );
            }
        }
        optics::apply_vignette_inplace(&mut plane_a, 0, r3.len(), r3.y0, &params.lens, &frame);

        // -- Veiling (B) over r2 ------------------------------------------------
        if veiling_on {
            // Horizontal pass over all r3 rows into plane_t, vertical pass
            // over the r2 window into plane_b, then mix into plane_t and
            // move the result back to plane_a.
            veiling.conv_h(&plane_a, &mut plane_t, 0, r3.len());
            veiling.conv_v(&plane_t, &mut plane_b, r2.lo(y3), r2.hi(y3));
            optics::apply_veiling_mix(
                &plane_a,
                &plane_b,
                &mut plane_t,
                r2.lo(y3),
                r2.hi(y3),
                params.lens.veiling_amount,
            );
            copy_rows(&plane_t, &mut plane_a, r2.lo(y3), r2.hi(y3));
        }

        // -- Excitation C1/C3 + A (EV, pre-desat) over r2 (in-place) -----------
        emulsion::apply_excitation_inplace(
            &mut plane_a,
            r2.lo(y3),
            r2.hi(y3),
            &em,
            params.adjustments.ev,
            params.adjustments.pre_desaturate,
        );

        // -- Halation C2 over r1 (reads the r2 window) → plane_b ---------------
        emulsion::apply_halation(
            &plane_a,
            &mut plane_b,
            r1.lo(y3),
            r1.hi(y3),
            &em,
            (r2.lo(y3), r2.hi(y3)),
        );

        // -- Response D + chemistry E over r1 (in-place per-pixel) -------------
        emulsion::apply_response_inplace(&mut plane_a, r1.lo(y3), r1.hi(y3), &em);

        // -- Grain G over the strip (reads the r1 window) → plane_b ------------
        grain.apply(
            &plane_a,
            &mut plane_b,
            strip_y - y3,
            strip_y1 - y3,
            &frame,
            y3,
            h,
        );

        // -- Print/scan F over the strip → plane_a, then into the output ------
        print.apply(&plane_b, &mut plane_a, strip_y - y3, strip_y1 - y3);
        {
            let off = y3;
            for (ly, ay) in (strip_y..strip_y1).enumerate() {
                for x in 0..w {
                    let v = plane_a.px64(x, ly as u32 + (strip_y - off));
                    let p = out_img.pixel_mut(x, ay);
                    p[0] = v[0] as f32;
                    p[1] = v[1] as f32;
                    p[2] = v[2] as f32;
                }
            }
        }

        strip_y = strip_y1;
    }
    Ok(out_img)
}

/// Clamped absolute row region.
struct Region {
    y0: u32,
    y1: u32,
}

impl Region {
    fn expand(y0: u32, y1: u32, margin: u32, h: u32) -> Self {
        Self {
            y0: y0.saturating_sub(margin),
            y1: (y1 + margin).min(h),
        }
    }
    fn len(&self) -> u32 {
        self.y1 - self.y0
    }
    /// Local (plane-relative) start row, given the plane's absolute origin.
    fn lo(&self, origin: u32) -> u32 {
        self.y0 - origin
    }
    fn hi(&self, origin: u32) -> u32 {
        self.y1 - origin
    }
}

fn copy_rows(src: &Plane, dst: &mut Plane, y0: u32, y1: u32) {
    for y in y0..y1 {
        for x in 0..src.w {
            dst.set3(x, y, src.px64(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::testing::test_profile;
    use melography_core::ImageF32;

    /// A deterministic synthetic photo: gradients + a bright disc (gives
    /// every stage something to do — halation blooms around the disc,
    /// grain sees a density range, curves get a exposure sweep).
    fn synthetic_photo(w: u32, h: u32) -> ImageF32 {
        let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w - 1);
                let fy = f64::from(y) / f64::from(h - 1);
                let dx = fx - 0.62;
                let dy = fy - 0.35;
                let disc = (-((dx * dx + dy * dy) / 0.004).exp() * 3.0).exp();
                let base = 0.02 + 0.5 * fx * fy + 0.1 * fx;
                let v = (base + 2.0 * disc).min(4.0);
                let p = img.pixel_mut(x, y);
                p[0] = (v * 1.05) as f32;
                p[1] = (v * 0.95) as f32;
                p[2] = (v * 0.85) as f32;
            }
        }
        img
    }

    fn params() -> FilmChainParams {
        FilmChainParams::new(test_profile()).unwrap()
    }

    #[allow(dead_code)]
    pub(crate) fn synthetic_photo_for_debug(w: u32, h: u32) -> ImageF32 {
        synthetic_photo(w, h)
    }

    #[test]
    fn chain_renders_finite_sane_output() {
        let img = synthetic_photo(96, 64);
        let out = render(&img, &params()).unwrap();
        let mut max = 0.0f32;
        for v in out.data() {
            assert!(v.is_finite());
            max = max.max(*v);
        }
        // Real tonal action: not all black, not all blown.
        assert!(max > 0.01, "output must have brightness, max={max}");
        let non_zero = out.data().iter().filter(|v| **v > 0.001).count();
        assert!(
            non_zero > out.data().len() / 10,
            "output must not be near-black ({non_zero}/{})",
            out.data().len()
        );
    }

    #[test]
    fn chain_is_deterministic() {
        let img = synthetic_photo(80, 60);
        let p = params();
        let a = render(&img, &p).unwrap();
        let b = render(&img, &p).unwrap();
        assert_eq!(a.data(), b.data(), "same params must be bit-exact");
    }

    #[test]
    fn tiled_equals_monolithic_bytes() {
        // The §L gate: tiled render == monolithic render, byte for byte,
        // for several strip heights — including 1 (pathological tiling).
        let img = synthetic_photo(96, 64);
        let p = params();
        let mono = render(&img, &p).unwrap();
        for strip in [1u32, 3, 7, 16, 64] {
            let tiled = render_tiled(&img, &p, strip).unwrap();
            assert_eq!(
                mono.data(),
                tiled.data(),
                "strip_h={strip}: tiled render differs from monolithic"
            );
        }
    }

    #[test]
    fn disabled_chain_is_identity() {
        let img = synthetic_photo(16, 16);
        let p = params()
            .with(Adjustments {
                enabled: false,
                ..Default::default()
            })
            .unwrap();
        let out = render(&img, &p).unwrap();
        assert_eq!(out.data(), img.data());
    }

    #[test]
    fn brighter_input_prints_brighter() {
        // End-to-end tone polarity through the whole chain.
        let p = params();
        let dark = ImageF32::from_interleaved(4, 4, Channels::Rgb, vec![0.05; 4 * 4 * 3]);
        let bright = ImageF32::from_interleaved(4, 4, Channels::Rgb, vec![0.6; 4 * 4 * 3]);
        let o_dark = render(&dark, &p).unwrap();
        let o_bright = render(&bright, &p).unwrap();
        let m_dark: f64 = o_dark.data().iter().map(|v| f64::from(*v)).sum::<f64>() / 48.0;
        let m_bright: f64 = o_bright.data().iter().map(|v| f64::from(*v)).sum::<f64>() / 48.0;
        assert!(
            m_bright > m_dark,
            "brighter input must print brighter: {m_bright} vs {m_dark}"
        );
    }
}
