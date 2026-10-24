//! Non-ML auto-lighting baseline — the quality bar every model must beat
//! (AI_MODELS §2, ADR-0004 §1a). Deterministic, explainable, model-free:
//! zero downloads, zero size, zero license surface, offline by definition.
//!
//! ```text
//! EV:       argmin_ev | log2( percentile(Y_L, 50) / 0.18 ) + ev |
//!           with highlight guard: percentile(Y_L, 99.5) · 2^ev ≤ 0.95
//! WB:       gray-world on robust (trimmed-mean) statistics;
//!           skin-tone guard damps over-correction of warm scenes
//! Contrast: histogram percentile shaping (p0.1 → target black,
//!           p99 → target white)
//! ```
//!
//! Output is a [`ToneParams`] (the editable landing zone — suggestions are
//! *parameters*, never baked pixels) plus machine-readable notes the UI
//! localizes (AI_MODELS §7: honest, explainable suggestions).
//!
//! All statistics are computed from a single O(n) pass with fixed-bin
//! histograms — deterministic on every platform (no sorting, no threading).

use melography_color as mcolor;
use melography_color::tone::AP1_LUMA;
use melography_color::tone::{analysis_gains_for_gray, ToneParams, MID_GRAY};
use melography_core::ImageF32;

/// Baseline engine id — recorded in sidecar provenance (`SuggestionSource`).
pub const BASELINE_ID: &str = "autolight-baseline-v1";
pub const BASELINE_VERSION: &str = "1.0.0";

/// Percentile targets (AI_MODELS §2 constants, documented knobs).
pub const TARGET_MID_GRAY: f32 = MID_GRAY; // 0.18
pub const CLIP_TARGET: f32 = 0.95;
pub const TARGET_BLACK: f32 = 0.02;
pub const TARGET_WHITE: f32 = 0.95;
/// Suggest a black-point remap only when blacks are clearly lifted.
pub const BLACK_TRIGGER: f32 = 0.05;
/// Suggest a white-point remap only when whites are clearly dull.
pub const WHITE_TRIGGER: f32 = 0.75;
/// Below this per-cone log2 gain a cast is noise, not a suggestion.
pub const WB_CAST_THRESHOLD: f32 = 0.02;

// ── statistics ───────────────────────────────────────────────────────────────

const BINS: usize = 4096;

/// A count+sum histogram over [0, 1] (values clamped into the domain —
/// scene-linear values above 1 land in the top bin, which only matters for
/// the top percentiles where they belong anyway).
struct SumHist {
    count: [u64; BINS],
    sum: [f64; BINS],
    total: u64,
}

impl SumHist {
    fn new() -> Self {
        Self {
            count: [0; BINS],
            sum: [0.0; BINS],
            total: 0,
        }
    }

    fn push(&mut self, v: f32) {
        let c = v.clamp(0.0, 1.0);
        let bin = ((c * (BINS - 1) as f32).round()) as usize;
        self.count[bin] += 1;
        self.sum[bin] += f64::from(v.clamp(0.0, 4.0));
        self.total += 1;
    }

    /// q ∈ [0, 1] → percentile with linear interpolation inside the bin.
    fn percentile(&self, q: f32) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        let target = ((q.clamp(0.0, 1.0) * self.total as f32).ceil().max(1.0)) as u64;
        let mut acc = 0u64;
        for bin in 0..BINS {
            acc += self.count[bin];
            if acc >= target {
                return (bin as f32) / (BINS - 1) as f32;
            }
        }
        1.0
    }

    /// Mean of values inside the [q_lo, q_hi] percentile band (robust
    /// trimmed mean — outlier pixels cannot drag it).
    fn trimmed_mean(&self, q_lo: f32, q_hi: f32) -> f32 {
        let lo = self.percentile(q_lo);
        let hi = self.percentile(q_hi);
        let (mut sum, mut n) = (0.0f64, 0u64);
        for bin in 0..BINS {
            if self.count[bin] == 0 {
                continue;
            }
            let c = (bin as f32) / (BINS - 1) as f32;
            if c >= lo && c <= hi {
                sum += self.sum[bin];
                n += self.count[bin];
            }
        }
        if n == 0 {
            0.0
        } else {
            (sum / n as f64) as f32
        }
    }
}

/// Measured scene statistics — exposed for tests, the UI, and benchmarks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneStats {
    pub p001: f32,
    pub p005: f32,
    pub p50: f32,
    pub p99: f32,
    pub p995: f32,
    /// Robust per-channel means (trimmed 5–95 %).
    pub gray_rgb: [f32; 3],
    /// Fraction of pixels inside the skin-tone guard ellipse.
    pub skin_fraction: f32,
}

fn collect_stats(img: &ImageF32) -> SceneStats {
    let ch = img.channels().count();
    let mut luma = SumHist::new();
    let mut r = SumHist::new();
    let mut g = SumHist::new();
    let mut b = SumHist::new();
    let mut skin = 0u64;
    let mut n = 0u64;

    let ap1_to_709 = mcolor::ap1_to_srgb_linear();
    let data = img.data();
    for px in data.chunks(ch) {
        let (pr, pg, pb) = (px[0], px[1], px[2]);
        luma.push(AP1_LUMA[0] * pr + AP1_LUMA[1] * pg + AP1_LUMA[2] * pb);
        r.push(pr);
        g.push(pg);
        b.push(pb);
        if in_skin_ellipse(&ap1_to_709, [pr, pg, pb]) {
            skin += 1;
        }
        n += 1;
    }

    SceneStats {
        p001: luma.percentile(0.001),
        p005: luma.percentile(0.005),
        p50: luma.percentile(0.5),
        p99: luma.percentile(0.99),
        p995: luma.percentile(0.995),
        gray_rgb: [
            r.trimmed_mean(0.05, 0.95),
            g.trimmed_mean(0.05, 0.95),
            b.trimmed_mean(0.05, 0.95),
        ],
        skin_fraction: if n > 0 { skin as f32 / n as f32 } else { 0.0 },
    }
}

/// Skin-tone guard: pixels inside the elliptical core of the classic YCbCr
/// skin cluster (center Cb=102, Cr=153, semi-axes 25/20 — the ellipse
/// inscribed in the standard 77–127 / 133–173 full-range box; heuristic,
/// tuned to *dampen* rather than block). When a large fraction of the frame
/// is skin, a gray-world correction is fighting the subject, not the light.
fn in_skin_ellipse(ap1_to_709: &mcolor::matrix::Mat3, rgb: [f32; 3]) -> bool {
    // AP1 linear → linear Rec.709 → sRGB OETF → JFIF full-range YCbCr.
    let lin = ap1_to_709.apply_f32(rgb);
    let rp = mcolor::srgb_tf::linear_to_srgb(lin[0].clamp(0.0, 1.0));
    let gp = mcolor::srgb_tf::linear_to_srgb(lin[1].clamp(0.0, 1.0));
    let bp = mcolor::srgb_tf::linear_to_srgb(lin[2].clamp(0.0, 1.0));
    let y = 0.299 * rp + 0.587 * gp + 0.114 * bp;
    if y < 0.1 || y > 0.98 {
        return false; // the cluster is defined for meaningful luma only
    }
    let cb = 128.0 + (bp - y) * 111.0 / 255.0 * 255.0;
    let cr = 128.0 + (rp - y) * 143.0 / 255.0 * 255.0;
    let dcb = (cb - 102.0) / 25.0;
    let dcr = (cr - 153.0) / 20.0;
    dcb * dcb + dcr * dcr <= 1.0
}

// ── suggestion ──────────────────────────────────────────────────────────────

/// Machine-readable explanation notes (the UI maps these to localized
/// strings — AI_MODELS §7 "confidence is disclosed honestly").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    /// EV was reduced so p99.5 stays under the clip target.
    HighlightGuard,
    /// WB correction was damped because many skin-tone pixels were found.
    SkinGuardApplied,
    /// Black point clearly lifted → remap suggested.
    BlackPointLift,
    /// Whites clearly dull → remap suggested.
    WhitePointStretch,
    /// Crushed shadows → lift suggested.
    ShadowsCrushed,
    /// Clipping highlights → recovery suggested.
    HighlightsRecover,
}

/// A complete, explainable auto-lighting suggestion.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The editable parameters (landing zone: sidecar Tone stage).
    pub params: ToneParams,
    /// Display-only CCT estimate for the WB correction (McCamy).
    pub wb_cct_estimate_k: Option<f32>,
    /// Measured statistics (for the UI tooltip + benchmarks).
    pub stats: SceneStats,
    /// Explanation notes (machine-readable; UI localizes).
    pub notes: Vec<Note>,
}

/// Analyze an image (AP1 linear — pass a ≤ 512 px proxy per ADR-0004 §5;
/// [`suggest_proxy`] does the downscale) and produce the baseline
/// suggestion. Deterministic: same input → same suggestion, everywhere.
#[must_use]
pub fn suggest(img: &ImageF32) -> Suggestion {
    let stats = collect_stats(img);
    let mut notes = Vec::new();
    let mut params = ToneParams::default();

    // EV (AI_MODELS §2): match the median to 18 % gray…
    let ev_raw = (TARGET_MID_GRAY / stats.p50.max(1e-6)).log2();
    // …but never push p99.5 past the clip target.
    let ev_cap = (CLIP_TARGET / stats.p995.max(1e-6)).log2();
    let ev = if ev_cap < ev_raw {
        notes.push(Note::HighlightGuard);
        ev_cap
    } else {
        ev_raw
    };
    // Two-decimal quantization: suggestions are user-facing ("+0.4 EV")
    // and bin-quantized statistics must not leak sub-millistop noise —
    // an 18 % gray suggests exactly 0.00.
    params.ev = ((ev.clamp(-3.0, 3.0)) * 100.0).round() / 100.0;

    // WB: robust gray-world via cone gains.
    let gray = stats.gray_rgb;
    let gains = if gray[0] > 1e-6 && gray[1] > 1e-6 && gray[2] > 1e-6 {
        let g = analysis_gains_for_gray(gray);
        let strength = [g.r.log2().abs(), g.g.log2().abs(), g.b.log2().abs()]
            .into_iter()
            .fold(0.0f32, f32::max);
        if strength < WB_CAST_THRESHOLD {
            None // noise-level cast — no WB suggestion (not an event)
        } else if stats.skin_fraction > 0.10 {
            // Dampen up to 70 % at ≥ 25 % skin coverage (heuristic guard —
            // warm scenes keep their character instead of being "fixed").
            let damp = ((stats.skin_fraction - 0.10) / 0.15).clamp(0.0, 1.0) * 0.7;
            let damped = g.dampen(damp);
            notes.push(Note::SkinGuardApplied);
            let strength_after = [
                damped.r.log2().abs(),
                damped.g.log2().abs(),
                damped.b.log2().abs(),
            ]
            .into_iter()
            .fold(0.0f32, f32::max);
            if strength_after < WB_CAST_THRESHOLD {
                None
            } else {
                Some(damped)
            }
        } else {
            Some(g)
        }
    } else {
        None
    };
    let wb_cct_estimate_k = gains.as_ref().and_then(mcolor::tone::estimate_cct_k);
    params.wb = gains;

    // Highlights / shadows after EV.
    let top = stats.p995 * 2f32.powf(params.ev);
    if top > CLIP_TARGET {
        notes.push(Note::HighlightsRecover);
        params.highlights =
            (-100.0 * (((CLIP_TARGET / top).log2()) / 0.75).clamp(0.0, 1.0)).round();
    }
    let bot = stats.p005 * 2f32.powf(params.ev);
    if bot < 0.004 {
        notes.push(Note::ShadowsCrushed);
        params.shadows = (100.0 * (((0.004 / bot.max(1e-6)).log2()) / 2.0).clamp(0.0, 1.0)).round();
    }

    // Percentile shaping (AI_MODELS §2 contrast line).
    if stats.p001 > BLACK_TRIGGER {
        notes.push(Note::BlackPointLift);
        params.black_source = Some((stats.p001 * 1000.0).round() / 1000.0);
        params.black_target = Some(TARGET_BLACK);
    }
    if stats.p99 < WHITE_TRIGGER {
        notes.push(Note::WhitePointStretch);
        params.white_source = Some((stats.p99 * 1000.0).round() / 1000.0);
        params.white_target = Some(TARGET_WHITE);
    }

    Suggestion {
        params: params.sanitized(),
        wb_cct_estimate_k,
        stats,
        notes,
    }
}

/// Deterministic area-average downscale to a proxy (long edge ≤ max_dim;
/// no-op when already smaller), then [`suggest`]. ADR-0004 §5: analysis
/// runs at ≤ 512 px. Box averaging is the deterministic choice (integer
/// accumulation, no resampling filters that vary by platform).
#[must_use]
pub fn suggest_proxy(img: &ImageF32, max_dim: u32) -> Suggestion {
    let proxy = downscale_area(img, max_dim);
    suggest(&proxy)
}

/// Area-average downscale (long edge ≤ max_dim). RGB(A); alpha averaged
/// alongside (it is analysis-only — never rendered).
#[must_use]
pub fn downscale_area(img: &ImageF32, max_dim: u32) -> ImageF32 {
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
    let (sx, sy) = (f64::from(w) / f64::from(nw), f64::from(h) / f64::from(nh));
    let data = img.data();
    let odata = out.data_mut();
    for oy in 0..nh {
        let y0 = (f64::from(oy) * sy).floor() as u32;
        let y1 = ((f64::from(oy + 1) * sy).ceil() as u32).clamp(y0 + 1, h);
        for ox in 0..nw {
            let x0 = (f64::from(ox) * sx).floor() as u32;
            let x1 = ((f64::from(ox + 1) * sx).ceil() as u32).clamp(x0 + 1, w);
            let mut acc = vec![0.0f64; ch];
            let mut n = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let base = ((y * w + x) as usize) * ch;
                    for c in 0..ch {
                        acc[c] += f64::from(data[base + c]);
                    }
                    n += 1;
                }
            }
            let obase = ((oy * nw + ox) as usize) * ch;
            for c in 0..ch {
                odata[obase + c] = (acc[c] / n as f64) as f32;
            }
        }
    }
    out
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use melography_color::tone::WbGains;
    use melography_core::Channels;

    fn uniform(v: f32) -> ImageF32 {
        ImageF32::alloc(16, 16, Channels::Rgb, v)
    }

    fn uniform_cast(rgb: [f32; 3]) -> ImageF32 {
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for px in img.data_mut().chunks_mut(3) {
            px[0] = rgb[0];
            px[1] = rgb[1];
            px[2] = rgb[2];
        }
        img
    }

    #[test]
    fn mid_gray_is_identity() {
        // A *well-exposed* photo-like histogram: 1 % near black, the bulk
        // at 18 % gray, 20 % climbing to (but not past) the clip target.
        // (A uniform gray is maximal haze by definition — the shaping
        // suggestion there is correct, see the haze test.)
        let mut img = ImageF32::alloc(10, 10, Channels::Rgb, 0.0);
        let vals: Vec<f32> = {
            let mut v = vec![0.18; 79];
            v.push(0.01);
            v.extend((0..20).map(|i| 0.50 + i as f32 * 0.0231)); // ..=0.94
            v
        };
        for (i, px) in img.data_mut().chunks_mut(3).enumerate() {
            let x = vals[i % vals.len()];
            px[0] = x;
            px[1] = x;
            px[2] = x;
        }
        let s = suggest(&img);
        assert!((s.params.ev).abs() < 1e-6, "ev={}", s.params.ev);
        assert_eq!(s.params.wb, None);
        assert_eq!(s.params.highlights, 0.0);
        assert_eq!(s.params.shadows, 0.0);
        assert_eq!(s.params.black_source, None);
        assert_eq!(s.params.white_source, None);
        assert!(s.params.is_identity(), "{:?}", s.params);
        assert!(s.notes.is_empty(), "{:?}", s.notes);
    }

    #[test]
    fn dark_image_gets_positive_ev() {
        let s = suggest(&uniform(0.09));
        assert!((s.params.ev - 1.0).abs() < 0.01, "ev={}", s.params.ev);
        // p005 = 0.09 → not crushed; p995 = 0.18 < 0.95 → no recovery.
        assert!(!s.notes.contains(&Note::ShadowsCrushed));
        assert!(!s.notes.contains(&Note::HighlightsRecover));
    }

    #[test]
    fn bright_image_gets_negative_ev() {
        let s = suggest(&uniform(0.72));
        assert!((s.params.ev - (-2.0)).abs() < 0.01, "ev={}", s.params.ev);
    }

    #[test]
    fn highlight_guard_caps_ev() {
        // Median dark (wants +1 stop) but p99.5 clipped at 1.0: the guard
        // caps EV at log2(0.95/1.0) ≈ −0.074 → rounds to −0.07; the small
        // round-up leaves the top a hair over the clip target, so a mild
        // recovery is suggested (the post-rounding safety valve).
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.09);
        for px in img.data_mut().chunks_mut(3).skip(200) {
            px[0] = 1.0;
            px[1] = 1.0;
            px[2] = 1.0;
        }
        let s = suggest(&img);
        assert!(
            (s.params.ev + 0.07).abs() < 0.005,
            "ev={} must be guard-capped",
            s.params.ev
        );
        assert!(s.notes.contains(&Note::HighlightGuard));
        assert!(
            s.params.highlights <= 0.0,
            "recovery or nothing, never boost"
        );
    }

    #[test]
    fn green_cast_gets_compensating_wb() {
        // A green cast (far from the skin ellipse — a warm cast *is*
        // skin-like and the guard damps it by design, see the skin test).
        let cast = [MID_GRAY * 0.75, MID_GRAY * 1.4, MID_GRAY * 0.8];
        let img = uniform_cast(cast);
        let s = suggest(&img);
        assert!(s.stats.skin_fraction < 0.05, "green is not skin");
        let gains = s.params.wb.expect("cast must produce WB gains");
        // Applying the suggestion must neutralize the cast.
        let corrected = mcolor::tone::apply(&img, &s.params);
        let px = &corrected.data()[..3];
        let spread = (px[0] - px[1]).abs().max((px[2] - px[1]).abs());
        assert!(spread < 5e-3, "not neutralized: {px:?} (gains {gains:?})");
        // A CCT estimate is offered for display.
        assert!(s.wb_cct_estimate_k.is_some());
    }

    #[test]
    fn warm_cast_is_skin_guarded_not_neutralized() {
        // A warm flat field is skin-colored; the guard damps the gray-world
        // correction (warm scenes keep their character) — the suggestion
        // is deliberately weaker than full neutralization.
        let cast = [MID_GRAY * 1.4, MID_GRAY, MID_GRAY * 0.7];
        let s = suggest(&uniform_cast(cast));
        assert!(s.stats.skin_fraction > 0.5, "warm gray reads as skin");
        assert!(s.notes.contains(&Note::SkinGuardApplied));
        assert!(s.params.wb.is_some(), "still corrected, just damped");
    }

    #[test]
    fn neutral_image_gets_no_wb() {
        let s = suggest(&uniform(0.3));
        assert!(s.params.wb.is_none());
        assert!(!s.notes.contains(&Note::SkinGuardApplied));
    }

    #[test]
    fn crushed_shadows_get_lift() {
        // 90 % mid-gray + 10 % pure black.
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, MID_GRAY);
        for px in img.data_mut().chunks_mut(3).take(26) {
            px[0] = 0.0;
            px[1] = 0.0;
            px[2] = 0.0;
        }
        let s = suggest(&img);
        assert!(s.params.shadows > 0.0, "shadows={}", s.params.shadows);
        assert!(s.notes.contains(&Note::ShadowsCrushed));
        // Blacks at 0 → below the BLACK_TRIGGER? p001 = 0 → no black remap.
        assert_eq!(s.params.black_source, None);
    }

    #[test]
    fn lifted_blacks_and_dull_whites_get_shaping() {
        // A low-contrast haze: everything between 0.1 and 0.5.
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for (i, px) in img.data_mut().chunks_mut(3).enumerate() {
            let v = 0.1 + (i % 256) as f32 / 256.0 * 0.4;
            px[0] = v;
            px[1] = v;
            px[2] = v;
        }
        let s = suggest(&img);
        assert!(s.notes.contains(&Note::BlackPointLift));
        assert!(s.notes.contains(&Note::WhitePointStretch));
        let bs = s.params.black_source.expect("black knot");
        assert!(bs > 0.05);
        assert_eq!(s.params.black_target, Some(TARGET_BLACK));
        let ws = s.params.white_source.expect("white knot");
        assert!(ws < 0.75);
        assert_eq!(s.params.white_target, Some(TARGET_WHITE));
    }

    #[test]
    fn skin_guard_damps_wb() {
        // Same warm cast, but fill most of the frame with skin-tone pixels
        // (a warm skin RGB in AP1 ≈ [0.45, 0.30, 0.22]).
        let skin_ap1 = {
            let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
            for px in img.data_mut().chunks_mut(3) {
                px[0] = 0.45;
                px[1] = 0.30;
                px[2] = 0.22;
            }
            img
        };
        let stats = collect_stats(&skin_ap1);
        assert!(
            stats.skin_fraction > 0.9,
            "skin ellipse must recognize the sample (frac={})",
            stats.skin_fraction
        );

        let mut mixed = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for (i, px) in mixed.data_mut().chunks_mut(3).enumerate() {
            if i % 3 == 0 {
                // warm-cast gray
                px[0] = MID_GRAY * 1.5;
                px[1] = MID_GRAY;
                px[2] = MID_GRAY * 0.6;
            } else {
                px[0] = 0.45;
                px[1] = 0.30;
                px[2] = 0.22;
            }
        }
        let s_mixed = suggest(&mixed);
        assert!(s_mixed.notes.contains(&Note::SkinGuardApplied));
        // Without skin, the same cast would get the *undamped* gains —
        // compare against a no-skin image with an identical gray cast.
        let mut noskin = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for px in noskin.data_mut().chunks_mut(3) {
            px[0] = MID_GRAY * 1.5;
            px[1] = MID_GRAY;
            px[2] = MID_GRAY * 0.6;
        }
        let s_plain = suggest(&noskin);
        let (m, p) = match (s_mixed.params.wb, s_plain.params.wb) {
            (Some(m), Some(p)) => (m, p),
            (m, p) => panic!("both must suggest WB: mixed={m:?} plain={p:?}"),
        };
        let strength = |g: &WbGains| {
            [g.r.log2().abs(), g.g.log2().abs(), g.b.log2().abs()]
                .into_iter()
                .fold(0.0f32, f32::max)
        };
        assert!(
            strength(&m) < strength(&p),
            "skin guard must damp the correction: mixed={m:?} plain={p:?}"
        );
    }

    #[test]
    fn proxy_matches_full_resolution_suggestion() {
        // Statistics are scale-stable: a 512 px proxy of a big smooth
        // gradient suggests (nearly) the same EV as the full image.
        let mut img = ImageF32::alloc(1024, 1024, Channels::Rgb, 0.0);
        for y in 0..1024u32 {
            for x in 0..1024u32 {
                let v = 0.02 + (x + y) as f32 / 2048.0 * 0.6;
                let px = img.pixel_mut(x, y);
                px[0] = v;
                px[1] = v;
                px[2] = v;
            }
        }
        let full = suggest(&img);
        let proxy = suggest_proxy(&img, 512);
        assert!(
            (full.params.ev - proxy.params.ev).abs() < 0.05,
            "ev full={} proxy={}",
            full.params.ev,
            proxy.params.ev
        );
        assert_eq!(full.params.wb, proxy.params.wb);
    }

    #[test]
    fn downscale_area_preserves_mean_and_bounds() {
        let mut img = ImageF32::alloc(64, 32, Channels::Rgb, 0.0);
        for y in 0..32u32 {
            for x in 0..64u32 {
                let v = x as f32 / 64.0;
                let px = img.pixel_mut(x, y);
                px[0] = v;
                px[1] = v;
                px[2] = v;
            }
        }
        let small = downscale_area(&img, 16);
        assert_eq!(small.width(), 16);
        assert_eq!(small.height(), 8);
        let mean = small.data().iter().sum::<f32>() / small.data().len() as f32;
        assert!((mean - 0.5).abs() < 0.02, "mean={mean}");
    }

    #[test]
    fn suggestion_is_deterministic() {
        let img = uniform_cast([0.2, 0.25, 0.15]);
        let a = suggest(&img);
        let b = suggest(&img);
        assert_eq!(a, b);
    }
}
