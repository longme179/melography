//! Preview composition — the pixel path behind the editor viewport
//! (issue #8, UX_SPEC §4, ADR-0001 hard constraints).
//!
//! All math lives here in Rust; the webview only blits the RGBA bytes it
//! receives (`tauri::ipc::Response` → ArrayBuffer → `ImageData`). The
//! wire format is a typed binary frame — never JSON-embedded pixel arrays
//! and never giant base64 (ARCHITECTURE §6).
//!
//! **Preview vs export correctness:** the LUT stack applies at *full
//! resolution* on the AP1 working image, and the result is cached per
//! parameter-set; the viewport then downscales (box-average in linear AP1
//! — correct filtering space) and display-transforms. At 100% zoom the
//! preview pixels are the export pixels within published tolerance
//! (UX_SPEC §4 viewport rule); zoomed-out previews are proxy renders like
//! every other editor's (documented, not hidden).

use melography_color as mcolor;
use melography_core::{Channels, ImageF32};
use melography_lut::stage::{InsertionPoint, LutApply};
use melography_lut::{CubeLut, LutError};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Display-convention RGBA8 frame + render telemetry (S1-leg data).
#[derive(Debug, Clone)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, straight alpha, sRGB display convention.
    pub rgba: Vec<u8>,
    pub render_ms: f64,
    /// Aggregate "LUT domain clipping" badge input across the stack.
    pub clamped_pixels: u64,
}

/// Binary wire format (little-endian; the UI parses the same layout):
/// `magic u32 = 0x4D475046 ("MGPF") | version u32 = 1 | width u32 |
/// height u32 | render_ms f64 | clamped u64 | rgba (w·h·4 bytes)`.
pub const WIRE_MAGIC: u32 = 0x4D47_5046;
pub const WIRE_VERSION: u32 = 1;
pub const WIRE_HEADER_LEN: usize = 4 + 4 + 4 + 4 + 8 + 8;

impl PreviewFrame {
    #[must_use]
    pub fn to_wire(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(WIRE_HEADER_LEN + self.rgba.len());
        out.extend_from_slice(&WIRE_MAGIC.to_le_bytes());
        out.extend_from_slice(&WIRE_VERSION.to_le_bytes());
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.render_ms.to_le_bytes());
        out.extend_from_slice(&self.clamped_pixels.to_le_bytes());
        out.extend_from_slice(&self.rgba);
        out
    }
}

/// One resolved LUT stack entry (stage params + the loaded, hash-verified
/// LUT — resolution happens once at stack mutation, not per frame).
#[derive(Debug)]
pub struct ResolvedLut {
    pub apply: LutApply,
    pub lut: Arc<CubeLut>,
}

/// Full-resolution LUT'd working image + the cache key it was built for.
pub struct RenderCache {
    pub key: u64,
    /// LUT-stack output at full resolution (AP1).
    pub image: ImageF32,
    pub clamped_pixels: u64,
}

/// Hash the parameters that invalidate the full-res render.
#[must_use]
pub fn stack_key(base: &ImageF32, stack: &[ResolvedLut]) -> u64 {
    let mut h = DefaultHasher::new();
    // Base identity: geometry + content hash of the decoded source.
    h.write_u32(base.width());
    h.write_u32(base.height());
    melography_color::hash_image(base).hash(&mut h);
    for r in stack {
        r.apply.lut_hash.hash(&mut h);
        r.apply.intensity.to_bits().hash(&mut h);
        r.apply.insertion.order().hash(&mut h);
        r.apply.declared_input.as_str().hash(&mut h);
        r.apply.declared_output.as_str().hash(&mut h);
        r.apply.enabled.hash(&mut h);
    }
    h.finish()
}

/// Apply the LUT stack at full resolution (identity chain around it is
/// implied — M1 film chain is identity; M2 inserts real stages here).
pub fn render_full(base: &ImageF32, stack: &[ResolvedLut]) -> Result<(ImageF32, u64), LutError> {
    let mut img = base.clone();
    let mut clamped_total = 0u64;
    // Canonical insertion order (stage.rs apply_chain semantics, inlined
    // here so the cache sees exactly one pass).
    let mut ordered: Vec<&ResolvedLut> = stack.iter().filter(|r| r.apply.enabled).collect();
    ordered.sort_by_key(|r| r.apply.insertion.order());
    for r in ordered {
        let (out, report) = r.apply.apply(&r.lut, &img)?;
        img = out;
        clamped_total += report.clamped_pixels;
    }
    Ok((img, clamped_total))
}

/// Box-average downscale in linear AP1 (correct filtering space — CP §1:
/// never filter transfer-encoded pixels). `max_dim` bounds the longer
/// edge; identity when the image already fits.
#[must_use]
pub fn downscale_ap1(img: &ImageF32, max_dim: u32) -> ImageF32 {
    let (w, h) = (img.width(), img.height());
    let long = w.max(h);
    if long <= max_dim || max_dim == 0 {
        return img.clone();
    }
    let scale = f64::from(max_dim) / f64::from(long);
    let nw = ((f64::from(w) * scale).round() as u32).max(1);
    let nh = ((f64::from(h) * scale).round() as u32).max(1);
    let ch = img.channels().count();
    let mut out = ImageF32::alloc(nw, nh, img.channels(), 0.0);
    for y in 0..nh {
        // Source row span (inclusive edges; last cell absorbs remainder).
        let sy0 = (u64::from(y) * u64::from(h)) / u64::from(nh);
        let sy1 = (((u64::from(y) + 1) * u64::from(h)) / u64::from(nh)).max(sy0 + 1);
        for x in 0..nw {
            let sx0 = (u64::from(x) * u64::from(w)) / u64::from(nw);
            let sx1 = (((u64::from(x) + 1) * u64::from(w)) / u64::from(nw)).max(sx0 + 1);
            let mut acc = [0f64; 4];
            let mut n = 0u64;
            for sy in sy0..sy1.min(u64::from(h)) {
                for sx in sx0..sx1.min(u64::from(w)) {
                    let px = img.pixel(sx as u32, sy as u32);
                    for c in 0..ch {
                        acc[c] += f64::from(px[c]);
                    }
                    n += 1;
                }
            }
            let dst = out.pixel_mut(x, y);
            for c in 0..ch {
                dst[c] = (acc[c] / n as f64) as f32;
            }
        }
    }
    out
}

/// AP1-linear → sRGB-display RGBA8 with ordered dither (CP §6: dither
/// default-on for 8-bit outputs — kills quantization banding).
#[must_use]
pub fn ap1_to_rgba8_display(img: &ImageF32) -> Vec<u8> {
    let ch = img.channels().count();
    let n = img.width() as usize * img.height() as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    // Reuse the color crate's dithered sRGB path per pixel row chunk: it
    // operates on ImageF32 → (Vec<u8> RGB); we interleave alpha here.
    let rgb = mcolor::pipeline::ap1_to_srgb_u8_dither(img);
    for i in 0..n {
        rgba.push(rgb[i * 3]);
        rgba.push(rgb[i * 3 + 1]);
        rgba.push(rgb[i * 3 + 2]);
        rgba.push(if ch == 4 {
            (img.data()[i * 4 + 3].clamp(0.0, 1.0) * 255.0).round() as u8
        } else {
            255
        });
    }
    rgba
}

/// Compose one preview frame from the (possibly cached) full-res render.
pub fn compose_preview(
    base: &ImageF32,
    cache: &mut Option<RenderCache>,
    stack: &[ResolvedLut],
    max_dim: u32,
) -> Result<PreviewFrame, LutError> {
    let start = std::time::Instant::now();
    let key = stack_key(base, stack);
    let (full, clamped) = match cache {
        Some(c) if c.key == key => (c.image.clone(), c.clamped_pixels),
        _ => {
            let (img, clamped) = render_full(base, stack)?;
            *cache = Some(RenderCache {
                key,
                image: img.clone(),
                clamped_pixels: clamped,
            });
            (img, clamped)
        }
    };
    let small = downscale_ap1(&full, max_dim);
    let rgba = ap1_to_rgba8_display(&small);
    Ok(PreviewFrame {
        width: small.width(),
        height: small.height(),
        rgba,
        render_ms: start.elapsed().as_secs_f64() * 1000.0,
        clamped_pixels: clamped,
    })
}

/// The compare "original": base downscale + display transform (no LUT).
pub fn compose_original(base: &ImageF32, max_dim: u32) -> PreviewFrame {
    let start = std::time::Instant::now();
    let small = downscale_ap1(base, max_dim);
    let rgba = ap1_to_rgba8_display(&small);
    PreviewFrame {
        width: small.width(),
        height: small.height(),
        rgba,
        render_ms: start.elapsed().as_secs_f64() * 1000.0,
        clamped_pixels: 0,
    }
}

/// Parse a UI-supplied insertion point string (fixed vocabulary).
pub fn parse_insertion(s: &str) -> Result<InsertionPoint, String> {
    match s {
        "pre-film" => Ok(InsertionPoint::PreFilm),
        "film-input" => Ok(InsertionPoint::FilmInput),
        "print-output" => Ok(InsertionPoint::PrintOutput),
        "finishing" => Ok(InsertionPoint::Finishing),
        other => Err(format!(
            "unknown insertion point {other:?} (fixed list: pre-film, film-input, print-output, finishing)"
        )),
    }
}

// keep a typed anchor: Channels is part of the buffer contract
#[allow(dead_code)]
fn _ch_anchor(c: Channels) -> usize {
    c.count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use melography_lut::{cube::parse_cube, LutEncoding};

    fn gradient(w: u32, h: u32) -> ImageF32 {
        let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
        for y in 0..h {
            for x in 0..w {
                let px = img.pixel_mut(x, y);
                px[0] = x as f32 / (w - 1) as f32;
                px[1] = y as f32 / (h - 1) as f32;
                px[2] = 0.25;
            }
        }
        img
    }

    fn constant_lut(value: [f32; 3]) -> Arc<CubeLut> {
        let text = format!(
            "LUT_3D_SIZE 2\n{} {} {} \n{} {} {} \n{} {} {} \n{} {} {} \n{} {} {} \n{} {} {} \n{} {} {} \n{} {} {}\n",
            value[0], value[1], value[2], value[0], value[1], value[2],
            value[0], value[1], value[2], value[0], value[1], value[2],
            value[0], value[1], value[2], value[0], value[1], value[2],
            value[0], value[1], value[2], value[0], value[1], value[2]
        );
        Arc::new(parse_cube(&text).expect("constant lut parses"))
    }

    #[test]
    fn wire_roundtrip_layout() {
        let f = PreviewFrame {
            width: 3,
            height: 2,
            rgba: vec![7; 24],
            render_ms: 1.5,
            clamped_pixels: 9,
        };
        let wire = f.to_wire();
        assert_eq!(wire.len(), WIRE_HEADER_LEN + 24);
        assert_eq!(
            u32::from_le_bytes(wire[0..4].try_into().unwrap()),
            WIRE_MAGIC
        );
        assert_eq!(
            u32::from_le_bytes(wire[4..8].try_into().unwrap()),
            WIRE_VERSION
        );
        assert_eq!(u32::from_le_bytes(wire[8..12].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(wire[12..16].try_into().unwrap()), 2);
        assert_eq!(f64::from_le_bytes(wire[16..24].try_into().unwrap()), 1.5);
        assert_eq!(u64::from_le_bytes(wire[24..32].try_into().unwrap()), 9);
        assert_eq!(&wire[32..], &[7u8; 24]);
    }

    #[test]
    fn downscale_preserves_extrema_and_dimensions() {
        let img = gradient(64, 32);
        let small = downscale_ap1(&img, 16);
        assert_eq!((small.width(), small.height()), (16, 8));
        // Corners survive box averaging on a linear gradient.
        let tl = small.pixel(0, 0);
        let br = small.pixel(15, 7);
        assert!(tl[0] < 0.05, "{tl:?}");
        assert!(br[0] > 0.95, "{br:?}");
        // No-op when it fits.
        let same = downscale_ap1(&img, 64);
        assert_eq!(same.data(), img.data());
    }

    #[test]
    fn preview_cache_hits_and_invalidates() {
        let base = gradient(32, 32);
        let lut = constant_lut([0.8, 0.1, 0.1]);
        let stack = vec![ResolvedLut {
            apply: LutApply::new(
                "hash1",
                InsertionPoint::Finishing,
                1.0,
                LutEncoding::Rec709DisplayLinear,
                LutEncoding::Rec709DisplayLinear,
            ),
            lut: lut.clone(),
        }];

        let mut cache = None;
        let f1 = compose_preview(&base, &mut cache, &stack, 16).unwrap();
        let f2 = compose_preview(&base, &mut cache, &stack, 16).unwrap();
        assert_eq!(f1.rgba, f2.rgba, "cached path returns identical pixels");
        assert!(cache.as_ref().unwrap().key == stack_key(&base, &stack));

        // Intensity change → new key → different pixels.
        let stack2 = vec![ResolvedLut {
            apply: LutApply::new(
                "hash1",
                InsertionPoint::Finishing,
                0.0,
                LutEncoding::Rec709DisplayLinear,
                LutEncoding::Rec709DisplayLinear,
            ),
            lut,
        }];
        let f3 = compose_preview(&base, &mut cache, &stack2, 16).unwrap();
        assert_ne!(f1.rgba, f3.rgba, "intensity 0 vs 1 must differ");
    }

    #[test]
    fn full_render_reports_clamping() {
        let base = gradient(4, 4); // values up to 1.0; wrap pushes beyond domain
        let lut = constant_lut([0.5, 0.5, 0.5]);
        let stack = vec![ResolvedLut {
            apply: LutApply::new(
                "h",
                InsertionPoint::Finishing,
                1.0,
                LutEncoding::Rec709DisplayLinear,
                LutEncoding::Rec709DisplayLinear,
            ),
            lut,
        }];
        let (img, clamped) = render_full(&base, &stack).unwrap();
        assert!(clamped > 0, "saturated AP1 pixels clamp through the wrap");
        // Constant LUT at intensity 1: every pixel unwraps to the same AP1.
        let a = img.pixel(0, 0);
        let b = img.pixel(3, 3);
        assert!((a[0] - b[0]).abs() < 1e-6, "{a:?} {b:?}");
    }

    #[test]
    fn display_transform_produces_valid_rgba() {
        let img = gradient(8, 8);
        let rgba = ap1_to_rgba8_display(&img);
        assert_eq!(rgba.len(), 8 * 8 * 4);
        // Alpha opaque; black corner ~0; near-white corner high.
        assert_eq!(rgba[3], 255);
        assert!(rgba[0] <= 2, "{}", rgba[0]);
        let last = &rgba[(8 * 8 - 1) * 4..];
        assert!(last[1] > 200, "{last:?}");
    }
}
