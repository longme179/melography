//! The M0 pipeline: S1 input transform (→AP1 linear) and S5 export transform
//! (AP1 → sRGB), fast-path and lcms2 variants, plus ordered dithering for
//! 8-bit quantization (COLOR_PIPELINE §1 stage table, §6 export).

use crate::lcms::{
    ap1_profile, profile_for_encoding, srgb_profile, transform_image_f32, ColorError,
};
use crate::spaces;
use crate::srgb_tf::{linear_to_srgb, srgb_to_linear};
use lcms2::Intent;
use melography_core::{Channels, ColorEncoding, ImageF32};

/// S1 fast path: encoded sRGB f32 (0..1 code-value convention) → AP1 linear.
/// Pure scalar f32; bit-deterministic across OSes (S2 gate).
#[must_use]
pub fn srgb_to_ap1_fast(image: &ImageF32) -> ImageF32 {
    debug_assert_eq!(image.channels(), Channels::Rgb);
    let m = spaces::srgb_linear_to_ap1().to_f32();
    let mut out = Vec::with_capacity(image.data().len());
    for px in image.data().as_chunks::<3>().0 {
        let lin = [
            srgb_to_linear(px[0]),
            srgb_to_linear(px[1]),
            srgb_to_linear(px[2]),
        ];
        out.extend_from_slice(&mat3_f32_apply(&m, lin));
    }
    ImageF32::from_interleaved(image.width(), image.height(), Channels::Rgb, out)
}

/// S1 fast path: linear Display-P3 f32 → AP1 linear.
#[must_use]
pub fn p3_linear_to_ap1_fast(image: &ImageF32) -> ImageF32 {
    debug_assert_eq!(image.channels(), Channels::Rgb);
    let m = spaces::p3_linear_to_ap1().to_f32();
    let mut out = Vec::with_capacity(image.data().len());
    for px in image.data().as_chunks::<3>().0 {
        out.extend_from_slice(&mat3_f32_apply(&m, [px[0], px[1], px[2]]));
    }
    ImageF32::from_interleaved(image.width(), image.height(), Channels::Rgb, out)
}

/// S1 via lcms2 (arbitrary ICC / verification path).
pub fn to_ap1_lcms(image: &ImageF32, encoding: &ColorEncoding) -> Result<ImageF32, ColorError> {
    let src = profile_for_encoding(encoding)?;
    let dst = ap1_profile();
    transform_image_f32(image, &src, &dst, Intent::RelativeColorimetric)
}

/// S5 fast path: AP1 linear → sRGB 16-bit code values with clamping at the
/// documented boundary (export is a clamp boundary per COLOR_PIPELINE §1).
/// No dither for 16-bit targets (dithering is an 8-bit convenience, §6).
#[must_use]
pub fn ap1_to_srgb_u16_fast(image: &ImageF32) -> Vec<u16> {
    let m = spaces::ap1_to_srgb_linear().to_f32();
    let mut out = Vec::with_capacity(image.data().len());
    for px in image.data().as_chunks::<3>().0 {
        let lin = mat3_f32_apply(&m, [px[0], px[1], px[2]]);
        for c in lin {
            out.push(clamp_to_u16(linear_to_srgb(c)));
        }
    }
    out
}

/// S5 fast path with **ordered (4×4 Bayer) dithering** for 8-bit outputs —
/// the near-black banding gate (COLOR_PIPELINE §8 row 5) applies to this path.
#[must_use]
pub fn ap1_to_srgb_u8_dither(image: &ImageF32) -> Vec<u8> {
    const BAYER: [[f32; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    let m = spaces::ap1_to_srgb_linear().to_f32();
    let mut out = Vec::with_capacity(image.data().len());
    for (i, px) in image.data().as_chunks::<3>().0.iter().enumerate() {
        let x = i % image.width() as usize;
        let y = i / image.width() as usize;
        let dither = (BAYER[y % 4][x % 4] + 0.5) / 16.0 - 0.5; // centered in [-0.5, 0.5)
        let lin = mat3_f32_apply(&m, [px[0], px[1], px[2]]);
        for c in lin {
            let code = linear_to_srgb(c) + dither / 255.0;
            out.push(clamp_to_u8(code));
        }
    }
    out
}

fn mat3_f32_apply(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn clamp_to_u16(c: f32) -> u16 {
    (c.clamp(0.0, 1.0) * 65_535.0).round() as u16
}

fn clamp_to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// S5 via lcms2: AP1 → destination profile, output as f32 (caller quantizes).
pub fn from_ap1_lcms(
    image: &ImageF32,
    dest: &ColorEncoding,
    intent: Intent,
) -> Result<ImageF32, ColorError> {
    let src = ap1_profile();
    let dst = profile_for_encoding(dest)?;
    transform_image_f32(image, &src, &dst, intent)
}

/// Convenience: the default export destination encoding (sRGB, COLOR_PIPELINE
/// §6) as a `ColorEncoding`.
#[must_use]
pub fn default_export_encoding() -> ColorEncoding {
    ColorEncoding::Srgb { linear: false }
}

/// Reference bytes for the S2 double-transform detection harness: the
/// expected sRGB 8-bit rendering of a known AP1 pattern through
/// `lcms2(AP1→sRGB, relative colorimetric)`. The GUI harness (spike S4 app /
/// owner runbook) captures the actual canvas and compares against these
/// bytes; a webview-applied extra transform shifts them beyond tolerance.
#[must_use]
pub fn ap1_pattern_display_reference(pattern: &ImageF32) -> Vec<u8> {
    let m = spaces::ap1_to_srgb_linear().to_f32();
    pattern
        .data()
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|px| {
            let lin = mat3_f32_apply(&m, [px[0], px[1], px[2]]);
            lin.iter()
                .map(|&c| (linear_to_srgb(c).clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect::<Vec<u8>>()
        })
        .collect()
}

/// Re-export for the CLI/tests: transform via lcms2 keeping the sRGB
/// destination (roundtrip verification path).
pub fn ap1_to_srgb_lcms(image: &ImageF32) -> Result<ImageF32, ColorError> {
    transform_image_f32(
        image,
        &ap1_profile(),
        &srgb_profile(),
        Intent::RelativeColorimetric,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_image(w: u32, h: u32, data: &[f32]) -> ImageF32 {
        ImageF32::from_interleaved(w, h, Channels::Rgb, data.to_vec())
    }

    #[test]
    fn fast_path_roundtrip_midgray() {
        // sRGB code 0.5 → linear → AP1 → back; error must be far below the
        // COLOR_PIPELINE §8 threshold (0.5/255 in 8-bit scale).
        let c = 0.5_f32;
        let img = rgb_image(1, 1, &[c, c, c]);
        let ap1 = srgb_to_ap1_fast(&img);
        let u16 = ap1_to_srgb_u16_fast(&ap1);
        let expected = (c * 65_535.0).round() as u16;
        for v in &u16 {
            assert!(
                (i32::from(*v) - i32::from(expected)).abs() <= 128,
                "16-bit roundtrip error exceeds 0.5/255 (16-bit scale 128): {v} vs {expected}"
            );
        }
    }

    #[test]
    fn out_of_gamut_negative_survives_s1() {
        // Negative sRGB code (out-of-gamut proxy) must pass through S1
        // unbounded — the clamp boundary is export, not the working space.
        let img = rgb_image(1, 1, &[-0.1, 1.2, 0.0]);
        let ap1 = srgb_to_ap1_fast(&img);
        assert!(ap1.data().iter().any(|v| *v < 0.0 || *v > 1.0));
    }

    #[test]
    fn dither_keeps_extremes_exact() {
        // Black and white must stay 0/255 — dither is centered and clamped.
        let black = rgb_image(4, 4, &[0.0; 48]);
        assert!(ap1_to_srgb_u8_dither(&black).iter().all(|&v| v == 0));
        let ap1_white = rgb_image(4, 4, &[1.0; 48]); // AP1 linear white
        assert!(ap1_to_srgb_u8_dither(&ap1_white).iter().all(|&v| v == 255));
    }
}
