//! Preview composition — the pixel path behind the editor viewport
//! (issue #8, UX_SPEC §4, ADR-0001 hard constraints).
//!
//! All math lives here in Rust; the webview only blits the RGBA bytes it
//! receives (`tauri::ipc::Response` → ArrayBuffer → `ImageData`). The
//! wire format is a typed binary frame — never JSON-embedded pixel arrays
//! and never giant base64 (ARCHITECTURE §6).
//!
//! **Preview vs export correctness:** with no film leg active the LUT
//! stack applies at *full resolution* on the AP1 working image (export-
//! parity filtering), is box-averaged down to the preview bound (linear
//! AP1 — correct filtering space) **once**, and the *bounded* result is
//! cached per (parameter-set, `max_dim`); with a film leg the chain
//! renders at preview scale after the linear downscale (film-plane
//! coordinates, grain.md §3). At 100% zoom the preview pixels are the
//! export pixels within published tolerance (UX_SPEC §4 viewport rule);
//! zoomed-out previews are proxy renders like every other editor's
//! (documented, not hidden).
//!
//! **Preview transport bounds (A1):** the IPC commands resolve `max_dim`
//! through [`resolve_max_dim`] — `0` falls back to
//! `Settings.preview_max_dim`, `256..=2560` passes, anything else is the
//! typed `render.max_dim_out_of_range`. The internals (`compose_preview`,
//! `compose_original`, `downscale_ap1`) stay permissive: trusted callers
//! compose thumbnails at small bounds (48–256 px) through the same code.

use melography_color as mcolor;
use melography_core::{Channels, ImageF32};
use melography_film::chain::{self as film_chain, FilmChainParams};
use melography_film::Adjustments;
use melography_lut::stage::{InsertionPoint, LutApply};
use melography_lut::{CubeLut, LutError};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Preview-transport bound range (A1): 256 px is a postage stamp,
/// 2560 px is a 5K-class panel long edge. Bounded by construction — a
/// 2560-px long edge caps the RGBA8 wire frame at ≈ 26 MB no matter the
/// source resolution (a 24 MP source used to ship ~100 MB frames).
pub const MAX_DIM_MIN: u32 = 256;
pub const MAX_DIM_MAX: u32 = 2560;

/// Resolve a UI-requested `max_dim` at the command boundary (A1).
///
/// * `0` — the legacy "unbounded" sentinel: fall back to
///   `Settings.preview_max_dim` (itself clamped at load *and* at
///   `set_settings`; clamped again here as defense in depth).
/// * `MAX_DIM_MIN..=MAX_DIM_MAX` — passes through.
/// * anything else — the typed `render.max_dim_out_of_range` error:
///   a malformed request must never degrade into a full-res render
///   (the A1 defect class), it must fail loudly and locally.
pub fn resolve_max_dim(fallback: u32, max_dim: u32) -> Result<u32, crate::error::AppError> {
    match max_dim {
        0 => Ok(fallback.clamp(MAX_DIM_MIN, MAX_DIM_MAX)),
        MAX_DIM_MIN..=MAX_DIM_MAX => Ok(max_dim),
        other => Err(crate::error::AppError::new(
            "render.max_dim_out_of_range",
            format!(
                "max_dim {other} outside {MAX_DIM_MIN}..={MAX_DIM_MAX} (0 = use Settings.preview_max_dim)"
            ),
        )),
    }
}

/// Film leg input for preview composition: the resolved stock + knobs
/// (`None` = identity). The preview applies the film chain **at preview
/// scale after the linear downscale** — resolution-independent by
/// construction (film-plane coordinates; grain.md §3), so preview pixels
/// carry the physically-correct statistics and the export (full-res) is
/// the same field sampled natively. The zoom-consistency §L gate pins
/// this equivalence.
pub struct FilmLeg {
    pub params: FilmChainParams,
    /// Cheap cache key component (stock id + serialized knobs).
    pub key: u64,
}

pub fn film_leg_key(stock_id: &str, adjustments: &Adjustments) -> u64 {
    let mut h = DefaultHasher::new();
    stock_id.hash(&mut h);
    serde_json::to_string(adjustments)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

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

/// The bounded preview image + the cache key it was built for (A1: the
/// slot caches the *preview-scale* artifact — long edge ≤ the `max_dim`
/// it was composed for — never a full-resolution render).
pub struct RenderCache {
    pub key: u64,
    /// Preview-resolution result (AP1): film path = chain output at
    /// preview scale; no-film path = full-res LUT output downscaled once.
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

/// Cache key for the tone leg (M3): the bit pattern of every knob —
/// two param sets differing in any field produce different keys.
#[must_use]
pub fn tone_key(p: &melography_color::tone::ToneParams) -> u64 {
    use std::hash::Hasher;
    let mut h = DefaultHasher::new();
    for v in [
        p.ev,
        p.highlights,
        p.shadows,
        p.wb.map_or(1.0f32, |w| w.r),
        p.wb.map_or(1.0, |w| w.g),
        p.wb.map_or(1.0, |w| w.b),
        p.black_source.unwrap_or(0.0),
        p.black_target.unwrap_or(0.0),
        p.white_source.unwrap_or(0.0),
        p.white_target.unwrap_or(0.0),
    ] {
        h.write_u32(v.to_bits());
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

/// The cache key `compose_preview` keys on: base + LUT stack, the film
/// leg, the tone leg (identity tone = no leg) AND the preview bound
/// `max_dim` (A1). The cached artifact is the bounded preview image, so
/// every input that changes its pixels changes the key: two viewport
/// sizes never share a slot, and a differently-scaled frame can never be
/// served to a bounded request (the A1 cross-size poisoning defect).
#[must_use]
pub fn preview_key(
    base: &ImageF32,
    stack: &[ResolvedLut],
    film: Option<&FilmLeg>,
    tone: Option<&melography_color::tone::ToneParams>,
    max_dim: u32,
) -> u64 {
    let mut key = stack_key(base, stack);
    if let Some(f) = film {
        key ^= f.key.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    if let Some(t) = tone.filter(|t| !t.is_identity()) {
        key ^= tone_key(t).wrapping_mul(0x5851_F42D_4C95_7F2D);
    }
    key ^ u64::from(max_dim).wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Compose one preview frame from the (possibly cached) bounded render.
/// Caching (#25 + A1): `cache` is the budgeted slot — hits/misses are
/// counted, the slot stores the *bounded* preview image (never
/// full-resolution), and a bounded image larger than
/// `cache.budget_bytes` is NOT stored (honest degradation: re-render per
/// interaction, surfaced via `cache_stats` — never a silent memory
/// overrun). The hit path is a blit: no re-filtering, no re-downscale.
#[allow(clippy::too_many_arguments)]
pub fn compose_preview(
    base: &ImageF32,
    cache: &mut crate::cache::PreviewCacheState,
    stack: &[ResolvedLut],
    film: Option<&FilmLeg>,
    tone: Option<&melography_color::tone::ToneParams>,
    max_dim: u32,
) -> Result<PreviewFrame, LutError> {
    let start = std::time::Instant::now();
    // Identity tone params are exactly "no tone leg" — same key, same path.
    let tone = tone.filter(|t| !t.is_identity());
    let key = preview_key(base, stack, film, tone, max_dim);
    let (small, clamped) = match cache.slot.as_ref().filter(|c| c.key == key) {
        Some(c) => {
            cache.hits += 1;
            (c.image.clone(), c.clamped_pixels)
        }
        None => {
            cache.misses += 1;
            let (rendered, clamped) = render_pipeline(base, stack, film, tone, max_dim)?;
            // The slot caches the BOUNDED preview (A1): with a film leg
            // the pipeline already rendered at preview scale; without one
            // the LUT stack ran at full resolution (export-parity
            // filtering) and is downscaled here — once — so the slot
            // never retains a full-resolution image and later hits are
            // blits, not re-filters.
            let small = if film.is_some() {
                rendered
            } else {
                downscale_ap1(&rendered, max_dim)
            };
            if (small.byte_len() as u64) <= cache.budget_bytes {
                cache.slot = Some(RenderCache {
                    key,
                    image: small.clone(),
                    clamped_pixels: clamped,
                });
            } else {
                // Budget enforcement (#25): too large to retain.
                cache.slot = None;
            }
            (small, clamped)
        }
    };
    let rgba = ap1_to_rgba8_display(&small);
    Ok(PreviewFrame {
        width: small.width(),
        height: small.height(),
        rgba,
        render_ms: start.elapsed().as_secs_f64() * 1000.0,
        clamped_pixels: clamped,
    })
}

/// Preview-scale pipeline: tone (full-res, scene-referred — same domain as
/// export) → linear downscale → film chain (scale-free physics) → LUT
/// stack (finishing semantics). With a film leg the returned image is the
/// final preview-resolution result; without one it is the full-resolution
/// LUT'd image — `compose_preview` bounds it before caching (A1).
fn render_pipeline(
    base: &ImageF32,
    stack: &[ResolvedLut],
    film: Option<&FilmLeg>,
    tone: Option<&melography_color::tone::ToneParams>,
    max_dim: u32,
) -> Result<(ImageF32, u64), LutError> {
    // Tone first, at full resolution (export parity: the CLI/export path
    // applies tone to the full-res working image before LUT/film).
    let toned = match tone {
        Some(t) => melography_color::tone::apply(base, t),
        None => base.clone(),
    };
    let mut img = if film.is_some() {
        downscale_ap1(&toned, max_dim)
    } else {
        toned
    };
    if let Some(f) = film {
        // A2.1 — strip threading: byte-identical to the monolithic chain
        // (chunks are just another strip partition; the §L gate pins it
        // for every worker count), one chunk per logical core, scratch
        // capped at 2× the monolithic footprint. Export keeps the
        // sequential CLI path (memory-first, R-REN-2).
        img = film_chain::render_threaded(&img, &f.params, 0).map_err(|e| LutError::Malformed {
            detail: format!("film chain: {e}"),
        })?;
    }
    // LUT stack (full-res path unchanged when no film leg is active).
    if film.is_some() {
        let small = img;
        let (out, clamped) = render_full(&small, stack)?;
        Ok((out, clamped))
    } else {
        render_full(&img, stack)
    }
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

/// WB eyedropper math (UX_SPEC §6, session 08): mean AP1-linear RGB over
/// a (2r+1)×(2r+1) window (edge-clamped — picks near borders still see a
/// full sample) → gray-target cone gains via `analysis_gains_for_gray`,
/// the SAME function auto-lighting uses. One WB path for suggestions
/// and hand picks; pure, deterministic, testable.
pub fn wb_gains_for_region(
    ap1: &ImageF32,
    cx: u32,
    cy: u32,
    r: u32,
) -> melography_color::tone::WbGains {
    let (w, h) = (ap1.width().max(1), ap1.height().max(1));
    let x0 = cx.saturating_sub(r).min(w - 1);
    let x1 = cx.saturating_add(r).min(w - 1);
    let y0 = cy.saturating_sub(r).min(h - 1);
    let y1 = cy.saturating_add(r).min(h - 1);
    let (mut sr, mut sg, mut sb, mut n) = (0f64, 0f64, 0f64, 0u64);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = ap1.pixel(x, y);
            sr += f64::from(p[0]);
            sg += f64::from(p[1]);
            sb += f64::from(p[2]);
            n += 1;
        }
    }
    let n = f64::from(n.max(1) as u32);
    let gray = [(sr / n) as f32, (sg / n) as f32, (sb / n) as f32];
    melography_color::tone::analysis_gains_for_gray(gray)
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

        let mut cache = crate::cache::PreviewCacheState::new(u64::MAX);
        let f1 = compose_preview(&base, &mut cache, &stack, None, None, 16).unwrap();
        let f2 = compose_preview(&base, &mut cache, &stack, None, None, 16).unwrap();
        assert_eq!(f1.rgba, f2.rgba, "cached path returns identical pixels");
        // A1: the slot key includes max_dim — same request, same key.
        assert!(cache.slot.as_ref().unwrap().key == preview_key(&base, &stack, None, None, 16));
        assert_eq!(cache.hits, 1, "second compose hit");
        assert_eq!(cache.misses, 1, "first compose missed");

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
        let f3 = compose_preview(&base, &mut cache, &stack2, None, None, 16).unwrap();
        assert_ne!(f1.rgba, f3.rgba, "intensity 0 vs 1 must differ");
    }

    #[test]
    fn preview_cache_budget_is_enforced() {
        // #25: an image bigger than the budget renders fine but is never
        // retained — the honest degradation contract.
        let base = gradient(32, 32);
        let stack: Vec<ResolvedLut> = Vec::new();
        let mut cache = crate::cache::PreviewCacheState::new(64); // 64 bytes!
        let f1 = compose_preview(&base, &mut cache, &stack, None, None, 16).unwrap();
        assert!(cache.slot.is_none(), "oversize render not retained");
        assert_eq!(cache.misses, 1);
        // Second compose: still a miss (nothing cached), same pixels.
        let f2 = compose_preview(&base, &mut cache, &stack, None, None, 16).unwrap();
        assert_eq!(f1.rgba, f2.rgba);
        assert_eq!(cache.misses, 2, "no hit possible when uncached");
        assert_eq!(cache.hits, 0);
    }

    #[test]
    fn tone_leg_changes_preview_and_cache_key() {
        use melography_color::tone::ToneParams;
        let base = gradient(32, 32);
        let stack: Vec<ResolvedLut> = Vec::new();
        let mut cache = crate::cache::PreviewCacheState::new(u64::MAX);
        let f1 = compose_preview(&base, &mut cache, &stack, None, None, 16).unwrap();
        let key_before = cache.slot.as_ref().unwrap().key;

        // Identity tone = same key, same pixels.
        let f_id = compose_preview(
            &base,
            &mut cache,
            &stack,
            None,
            Some(&ToneParams::default()),
            16,
        )
        .unwrap();
        assert_eq!(key_before, cache.slot.as_ref().unwrap().key);
        assert_eq!(f1.rgba, f_id.rgba);

        // A real correction → new key → visibly brighter pixels (mean).
        let tone = ToneParams {
            ev: 1.0,
            ..ToneParams::default()
        };
        let f_ev = compose_preview(&base, &mut cache, &stack, None, Some(&tone), 16).unwrap();
        assert_ne!(key_before, cache.slot.as_ref().unwrap().key);
        let mean = |f: &PreviewFrame| {
            f.rgba.iter().map(|&c| u32::from(c)).sum::<u32>() / f.rgba.len() as u32
        };
        assert!(
            mean(&f_ev) > mean(&f1),
            "+1 EV must brighten the preview: {} !> {}",
            mean(&f_ev),
            mean(&f1)
        );
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

    // ── WB eyedropper (session 08) ────────────────────────────────────

    fn flat(w: u32, h: u32, rgb: [f32; 3]) -> ImageF32 {
        let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
        for y in 0..h {
            for x in 0..w {
                let p = img.pixel_mut(x, y);
                p.copy_from_slice(&rgb);
            }
        }
        img
    }

    #[test]
    fn wb_pick_on_neutral_gray_is_neutral() {
        let img = flat(9, 9, [0.18, 0.18, 0.18]); // equal-energy gray
        let g = wb_gains_for_region(&img, 4, 4, 2);
        assert!(
            g.is_neutral(),
            "equal-energy gray needs no correction: {g:?}"
        );
        assert!(g.is_valid());
    }

    #[test]
    fn wb_pick_direction_compensates_the_cast() {
        // Warm cast (red-dominant): to neutralize, the RED gain must drop
        // below the BLUE gain — the inverse of the measured ratios.
        let img = flat(9, 9, [0.30, 0.18, 0.10]);
        let g = wb_gains_for_region(&img, 4, 4, 2);
        assert!(g.r < g.b, "warm cast must yield r-gain < b-gain: {g:?}");
        assert!(g.is_valid());
    }

    #[test]
    fn wb_pick_window_is_edge_clamped() {
        // Pick at the very corner: the window clamps instead of panicking,
        // and a flat image still yields neutral gains.
        let img = flat(6, 6, [0.2, 0.2, 0.2]);
        let g = wb_gains_for_region(&img, 0, 0, 2);
        assert!(g.is_neutral(), "corner pick on flat gray: {g:?}");
    }

    #[test]
    fn wb_pick_averages_noise_in_the_window() {
        // A single bright outlier inside the window must not dominate a
        // small (5×5) sample — the mean smooths pixel noise (that is why
        // the pick is a REGION, not one pixel).
        let mut img = flat(9, 9, [0.18, 0.18, 0.18]);
        let p = img.pixel_mut(4, 4);
        p.copy_from_slice(&[1.0, 1.0, 1.0]);
        let g = wb_gains_for_region(&img, 4, 4, 2);
        // 24 neutral + 1 white pixel: chromatically neutral. Exact 1.0
        // is not guaranteed through the f32 cone math (observed 1 ulp),
        // so the tolerance is one part in a million — still "neutral"
        // by any photographic measure.
        for v in [g.r, g.g, g.b] {
            assert!(
                (v - 1.0).abs() < 1e-6,
                "chromatic mean over the window: {g:?}"
            );
        }
        assert!(g.is_valid());
    }

    // ── A1 · Preview transport regressions (M5-A) ────────────────────

    fn film_leg_fixture() -> FilmLeg {
        let profile = melography_film::profile::testing::test_profile();
        let params = FilmChainParams::new(profile).expect("test profile is valid");
        FilmLeg {
            params,
            key: film_leg_key("test-stock", &Adjustments::default()),
        }
    }

    #[test]
    fn max_dim_resolves_at_the_command_boundary() {
        // 0 = the legacy "unbounded" sentinel → the Settings fallback
        // (proved with a non-default value: the fallback is READ, not
        // hardcoded).
        assert_eq!(resolve_max_dim(1024, 0).unwrap(), 1024);
        // In-range passes through; both bounds are inclusive.
        assert_eq!(resolve_max_dim(1024, MAX_DIM_MIN).unwrap(), 256);
        assert_eq!(resolve_max_dim(1024, MAX_DIM_MAX).unwrap(), 2560);
        // Out of range → the typed error, never a silent full-res render.
        for bad in [1u32, 255, 2561, 99_999, u32::MAX] {
            let e = resolve_max_dim(1024, bad).unwrap_err();
            assert_eq!(e.code, "render.max_dim_out_of_range", "max_dim={bad}");
        }
        // Defense in depth: even a drifted fallback clamps in range.
        assert_eq!(resolve_max_dim(u32::MAX, 0).unwrap(), MAX_DIM_MAX);
        assert_eq!(resolve_max_dim(1, 0).unwrap(), MAX_DIM_MIN);
    }

    #[test]
    fn preview_cache_key_separates_viewport_sizes() {
        // A1 regression (defect: the key omitted max_dim): same params,
        // different max_dim → different keys, on BOTH legs — a bounded
        // request can never hit a differently-scaled slot.
        let base = gradient(64, 48);
        let stack: Vec<ResolvedLut> = Vec::new();
        assert_ne!(
            preview_key(&base, &stack, None, None, 16),
            preview_key(&base, &stack, None, None, 32)
        );
        let leg = film_leg_fixture();
        assert_ne!(
            preview_key(&base, &stack, Some(&leg), None, 16),
            preview_key(&base, &stack, Some(&leg), None, 32)
        );
        // Identity tone folds out of the key (unchanged semantic).
        use melography_color::tone::ToneParams;
        assert_eq!(
            preview_key(&base, &stack, None, None, 16),
            preview_key(&base, &stack, None, Some(&ToneParams::default()), 16)
        );
    }

    #[test]
    fn bounded_request_never_receives_a_larger_cached_frame() {
        // A1 regression (the slot-poisoning defect): after a large
        // compose, a smaller request must NOT be served the stale larger
        // frame from the slot — and the slot must hold the bounded image,
        // never the full-resolution render.
        let base = gradient(512, 384);
        let stack: Vec<ResolvedLut> = Vec::new();
        let mut cache = crate::cache::PreviewCacheState::new(u64::MAX);

        let big = compose_preview(&base, &mut cache, &stack, None, None, 512).unwrap();
        assert_eq!(
            (big.width, big.height),
            (512, 384),
            "source fits — identity"
        );
        let slot = cache.slot.as_ref().expect("512-bounded render retained");
        assert!(slot.image.width().max(slot.image.height()) <= 512);

        let small = compose_preview(&base, &mut cache, &stack, None, None, 256).unwrap();
        assert_eq!(
            (small.width, small.height),
            (256, 192),
            "bounded by the REQUEST, not by what the slot happens to hold"
        );
        assert_eq!(
            cache.misses, 2,
            "different max_dim = different key = miss, never a stale hit"
        );
        let slot = cache.slot.as_ref().unwrap();
        assert!(
            slot.image.width().max(slot.image.height()) <= 256,
            "slot replaced with the 256-bounded artifact, not the 512 one"
        );
    }

    #[test]
    fn film_leg_compose_is_bounded_by_max_dim() {
        // A1 regression (the reported "Phòng phim" defect): with a film
        // leg active the preview renders at preview scale — a max_dim of
        // 0 used to mean "identity downscale" → a full-res film-chain
        // render on every slider commit, shipped over IPC and discarded.
        // The command boundary resolves 0 away; this pins the
        // compose-level bound: frame and slot are both max_dim-bounded
        // even when the source is much larger.
        let base = gradient(600, 400);
        let leg = film_leg_fixture();
        let stack: Vec<ResolvedLut> = Vec::new();
        let mut cache = crate::cache::PreviewCacheState::new(u64::MAX);

        let f = compose_preview(&base, &mut cache, &stack, Some(&leg), None, 256).unwrap();
        assert_eq!((f.width, f.height), (256, 171), "600×400 → 256-bounded");
        assert_eq!(f.rgba.len(), 256 * 171 * 4, "wire frame matches geometry");
        let slot = cache.slot.as_ref().expect("film render retained");
        assert_eq!((slot.image.width(), slot.image.height()), (256, 171));

        // Same leg at a different bound → miss + re-render at that scale
        // (grain statistics are scale-correct, never a stretched stale
        // frame).
        let f2 = compose_preview(&base, &mut cache, &stack, Some(&leg), None, 128).unwrap();
        assert_eq!((f2.width, f2.height), (128, 85));
        assert_eq!(cache.misses, 2);
    }
}
