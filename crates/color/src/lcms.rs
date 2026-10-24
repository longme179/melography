//! lcms2 integration (vendored-static build — see crate docs).
//!
//! M0 surface (Spike S2): programmatic profile construction (sRGB, Display-P3,
//! AP1 as linear matrix-shaper), ICC-bytes loading, float32 RGB transforms,
//! and the lcms2 version report used by the cross-OS determinism gate.
//!
//! **Display-profile discovery (S4 machinery) is M1** — this module first
//! proves transform correctness headlessly; OS integration arrives with the
//! editor shell.

use crate::spaces;
use lcms2::{CIExyY, Intent, PixelFormat, Profile, ToneCurve, Transform};
use melography_core::{Channels, ColorEncoding, ImageF32};

/// lcms2 library version as an integer, e.g. `2190` ⇒ 2.19.
///
/// Because `melography-color` pins the `static` (vendored) build of lcms2,
/// every OS compiles the *same* library version; asserting this value in CI
/// on all three operating systems is the S2 "same library everywhere" gate.
#[must_use]
pub fn lcms2_version() -> u32 {
    lcms2::version()
}

/// Programmatic AP1 profile: AP1 primaries, ACES white, linear TRCs
/// (matrix-shaper, v2 ICC semantics — the common denominator every CMM
/// handles identically).
#[must_use]
pub fn ap1_profile() -> Profile {
    let trc = ToneCurve::new(1.0); // linear
    let white = CIExyY {
        x: spaces::white::ACES.x,
        y: spaces::white::ACES.y,
        Y: 1.0,
    };
    let (r, g, b) = spaces::ap1_primaries(); // derived from the authoritative matrix
    let primaries = lcms2::CIExyYTRIPLE {
        Red: xy_to_cie(r),
        Green: xy_to_cie(g),
        Blue: xy_to_cie(b),
    };
    Profile::new_rgb(&white, &primaries, &[&trc, &trc, &trc])
        .expect("AP1 profile construction is total for valid primaries")
}

/// Programmatic Display-P3 profile (D65, sRGB parametric TRC — IEC 61966-2-1
/// curve, lcms2 parametric type 4).
#[must_use]
pub fn p3_profile() -> Profile {
    let trc = ToneCurve::new_parametric(4, &[2.4, 1.055, 0.055, 1.0 / 12.92, 0.040_45])
        .expect("sRGB parametric curve parameters are valid");
    let white = CIExyY {
        x: spaces::white::D65.x,
        y: spaces::white::D65.y,
        Y: 1.0,
    };
    let primaries = lcms2::CIExyYTRIPLE {
        Red: xy_to_cie(spaces::p3_primaries::R),
        Green: xy_to_cie(spaces::p3_primaries::G),
        Blue: xy_to_cie(spaces::p3_primaries::B),
    };
    Profile::new_rgb(&white, &primaries, &[&trc, &trc, &trc])
        .expect("P3 profile construction is total for valid primaries")
}

/// Built-in sRGB profile.
#[must_use]
pub fn srgb_profile() -> Profile {
    Profile::new_srgb()
}

/// Profile from raw ICC bytes (embedded profiles, S0 decode path).
pub fn profile_from_icc(bytes: &[u8]) -> Result<Profile, ColorError> {
    Profile::new_icc(bytes).map_err(|e| ColorError::Lcms(format!("invalid ICC profile: {e:?}")))
}

fn xy_to_cie(c: spaces::Chromaticity) -> CIExyY {
    CIExyY {
        x: c.x,
        y: c.y,
        Y: 1.0,
    }
}

/// Transform an interleaved RGB(A) f32 image between two profiles.
/// Intent: relative colorimetric (photographic default; COLOR_PIPELINE §6).
///
/// NaN/Inf inputs are rejected (`ColorError::InvalidPixels`) — the pipeline
/// never smuggles non-finite values through the CMM.
pub fn transform_image_f32(
    image: &ImageF32,
    from: &Profile,
    to: &Profile,
    intent: Intent,
) -> Result<ImageF32, ColorError> {
    if image.data().iter().any(|v| !v.is_finite()) {
        return Err(ColorError::InvalidPixels);
    }
    match image.channels() {
        Channels::Rgb => {
            let xform: Transform<[f32; 3], [f32; 3]> =
                Transform::new(from, PixelFormat::RGB_FLT, to, PixelFormat::RGB_FLT, intent)
                    .map_err(|e| ColorError::Lcms(format!("transform creation failed: {e:?}")))?;
            let src: &[[f32; 3]] = bytemuck::cast_slice(image.data());
            let mut dst = vec![[0.0_f32; 3]; src.len()];
            xform.transform_pixels(src, &mut dst);
            let flat = bytemuck::cast_slice(&dst);
            Ok(ImageF32::from_interleaved(
                image.width(),
                image.height(),
                Channels::Rgb,
                flat.to_vec(),
            ))
        }
        Channels::Rgba => {
            let xform: Transform<[f32; 4], [f32; 4]> = Transform::new(
                from,
                PixelFormat::RGBA_FLT,
                to,
                PixelFormat::RGBA_FLT,
                intent,
            )
            .map_err(|e| ColorError::Lcms(format!("transform creation failed: {e:?}")))?;
            let src: &[[f32; 4]] = bytemuck::cast_slice(image.data());
            let mut dst = vec![[0.0_f32; 4]; src.len()];
            xform.transform_pixels(src, &mut dst);
            let flat = bytemuck::cast_slice(&dst);
            Ok(ImageF32::from_interleaved(
                image.width(),
                image.height(),
                Channels::Rgba,
                flat.to_vec(),
            ))
        }
    }
}

/// Errors from color management operations.
#[derive(Debug, thiserror::Error)]
pub enum ColorError {
    #[error("lcms2 error: {0}")]
    Lcms(String),
    #[error("non-finite pixel values rejected by the CMM path")]
    InvalidPixels,
    #[error("unsupported encoding for this operation: {0}")]
    Unsupported(String),
}

/// Map a `ColorEncoding` to a concrete lcms2 profile (S1 input stage).
/// `ColorEncoding::Untagged` maps to sRGB with the *caller* responsible for
/// surfacing the UI warning (COLOR_PIPELINE §2 priority order).
pub fn profile_for_encoding(enc: &ColorEncoding) -> Result<Profile, ColorError> {
    match enc {
        ColorEncoding::Srgb { .. } | ColorEncoding::Untagged => Ok(srgb_profile()),
        ColorEncoding::DisplayP3 { .. } => Ok(p3_profile()),
        ColorEncoding::Ap1Linear => Ok(ap1_profile()),
        ColorEncoding::Icc { profile } => profile_from_icc(profile),
    }
}
