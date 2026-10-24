//! Scene-referred tone operator — the M3 "Tone" pipeline stage
//! (AI_MODELS §2/§7, ADR-0004 §6).
//!
//! Everything here is **deterministic, explainable, CPU-first math** on AP1
//! linear pixels — it is the *landing zone* for auto-lighting suggestions:
//! suggestions become [`ToneParams`] values (editable, never baked pixels),
//! recorded in the sidecar with provenance (ADR-0004: "reproducible forever
//! after, even if the model is deleted").
//!
//! ## Semantics (apply order)
//!
//! 1. **White balance** — von Kries cone gains (Bradford), relative to the
//!    AP1 white; `[1,1,1]` is the exact identity. v1 stores *gains*, not
//!    Kelvin: gains round-trip exactly through analysis↔application, while
//!    a CCT parameterization would need a Planckian-locus table (documented
//!    follow-up). A McCamy CCT *estimate* is provided for display only.
//! 2. **Exposure** — `RGB ← RGB · 2^ev` (scene-linear stops).
//! 3. **Highlights / shadows** — luma-masked multiplicative shaping
//!    (±0.75 stops at ±100) plus a small additive floor lift for shadows
//!    (pure blacks stay reachable: multiplicative lift cannot raise y = 0).
//! 4. **Black/white percentile shaping** — monotone piecewise curve in
//!    log2 space through recorded knots `(0.18→0.18, source→target)`;
//!    sources are *measured percentiles pinned at suggestion time* so
//!    preview (proxy) and export (full-res) apply the identical mapping.

use crate::matrix::Mat3;
use crate::spaces::{ap1_white_cones, cct_from_chromaticity, wb_gains_to_ap1};
use melography_core::ImageF32;
use serde::{Deserialize, Serialize};

/// AP1 (ACEScg) luma weights — S-2014-004.
pub const AP1_LUMA: [f32; 3] = [0.272_228_7, 0.674_081_8, 0.053_689_5];

/// Midtone anchor for the percentile-shaping curve (18 % gray).
pub const MID_GRAY: f32 = 0.18;

// ── parameters ──────────────────────────────────────────────────────────────

/// White-balance correction as Bradford cone gains relative to the AP1
/// white. `[1,1,1]` = neutral. Stored/applied exactly; a CCT estimate for
/// display can be derived via [`estimate_cct_k`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WbGains {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl WbGains {
    pub const NEUTRAL: WbGains = WbGains {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    };

    #[must_use]
    pub fn is_neutral(&self) -> bool {
        self.r == 1.0 && self.g == 1.0 && self.b == 1.0
    }

    /// All components finite and positive (a von Kries gain must be).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.r.is_finite()
            && self.g.is_finite()
            && self.b.is_finite()
            && self.r > 0.0
            && self.g > 0.0
            && self.b > 0.0
    }

    /// Dampen toward neutral by `strength` ∈ [0,1] (0 = keep, 1 = neutral).
    /// Applied in log-gain space so it composes with repeated damping.
    #[must_use]
    pub fn dampen(&self, strength: f32) -> WbGains {
        let s = strength.clamp(0.0, 1.0);
        let d = |g: f32| (g.ln() * (1.0 - s)).exp();
        WbGains {
            r: d(self.r),
            g: d(self.g),
            b: d(self.b),
        }
    }
}

/// Tone-stage parameters — every field is a user-editable recipe parameter.
/// [`Default`] is the exact identity transform.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToneParams {
    /// Scene exposure in stops. Range [-5, 5].
    pub ev: f32,
    /// Highlight shaping, -100 (recover) ..= 100 (boost). ±0.75 stops max.
    pub highlights: f32,
    /// Shadow shaping, -100 (crush) ..= 100 (lift). ±0.75 stops max.
    pub shadows: f32,
    /// White-balance cone gains (`None` = as-shot / neutral).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wb: Option<WbGains>,
    /// Percentile-shaping knot: measured black percentile (source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub black_source: Option<f32>,
    /// Percentile-shaping knot: target value for `black_source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub black_target: Option<f32>,
    /// Percentile-shaping knot: measured white percentile (source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_source: Option<f32>,
    /// Percentile-shaping knot: target value for `white_source`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub white_target: Option<f32>,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            ev: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            wb: None,
            black_source: None,
            black_target: None,
            white_source: None,
            white_target: None,
        }
    }
}

impl ToneParams {
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self == &Self::default()
            || (self.ev == 0.0
                && self.highlights == 0.0
                && self.shadows == 0.0
                && self.wb.is_none_or(|w| w.is_neutral())
                && self.black_source.is_none()
                && self.black_target.is_none()
                && self.white_source.is_none()
                && self.white_target.is_none())
    }

    /// Clamp every knob into its documented range and drop invalid WB /
    /// degenerate knots (defensive: params arrive from sidecars and IPC).
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.ev = self.ev.clamp(-5.0, 5.0);
        self.highlights = self.highlights.clamp(-100.0, 100.0);
        self.shadows = self.shadows.clamp(-100.0, 100.0);
        if let Some(wb) = self.wb {
            self.wb = if wb.is_valid() { Some(wb) } else { None };
        }
        // Knot pairs must be present, ordered, and inside (0, 1) with the
        // midtone anchor preserved: 0 < black < 0.18 < white < 1.
        let knot_ok = |s: Option<f32>, t: Option<f32>, lo: f32, hi: f32| match (s, t) {
            (Some(s), Some(t)) => (lo..hi).contains(&s) && (lo..hi).contains(&t),
            (None, None) => true,
            _ => false,
        };
        if !knot_ok(self.black_source, self.black_target, 0.0, MID_GRAY) {
            self.black_source = None;
            self.black_target = None;
        }
        if !knot_ok(self.white_source, self.white_target, MID_GRAY, 1.0) {
            self.white_source = None;
            self.white_target = None;
        }
        self
    }
}

// ── display-only CCT estimate ───────────────────────────────────────────────

/// McCamy CCT estimate of the *scene illuminant implied by a correction*
/// (display only — the applied parameters stay gains). Under the von Kries
/// model the illuminant's cone response is the neutral white divided by the
/// correction gains.
#[must_use]
pub fn estimate_cct_k(gains: &WbGains) -> Option<f32> {
    if !gains.is_valid() {
        return None;
    }
    let w = ap1_white_cones();
    // Illuminant cones ∝ white / correction.
    let ill = [
        w[0] / f64::from(gains.r),
        w[1] / f64::from(gains.g),
        w[2] / f64::from(gains.b),
    ];
    // Cone response → XYZ (Bradford inverse) → chromaticity.
    // Reuse wb_gains_to_ap1's internals via its inverse relationship: the
    // cone→XYZ map is the Bradford matrix inverse applied to cone values.
    let m = crate::spaces::bradford_cone_to_xyz();
    let xyz = m.apply(ill);
    let sum = xyz[0] + xyz[1] + xyz[2];
    if sum <= 1e-12 || xyz[1] <= 1e-12 {
        return None;
    }
    cct_from_chromaticity(crate::spaces::Chromaticity {
        x: xyz[0] / sum,
        y: xyz[1] / sum,
    })
    .map(|c| c as f32)
}

/// Cone-space gains that neutralize a measured scene gray (AP1 linear
/// RGB): `gains_i = white_cone_i / observed_cone_i` — the exact von Kries
/// inverse of [`estimate_cct_k`]'s illuminant model. This is the *analysis*
/// half of white balance; the auto-lighting baseline (AI_MODELS §2) calls
/// it with robust (trimmed-mean) gray statistics and applies the skin-tone
/// guard on the result.
#[must_use]
pub fn analysis_gains_for_gray(gray: [f32; 3]) -> WbGains {
    let g = crate::spaces::wb_gains_for_ap1_gray(gray);
    // Normalize out the luminance factor: raw gains map the measured gray
    // onto the *unit-luma* white axis, so they carry a global scale
    // (~1/luma(gray)) that belongs to exposure (EV), not white balance.
    // The geometric mean removes it, leaving a pure chromatic correction.
    let geo = (g[0] * g[1] * g[2]).cbrt();
    let normalized = if geo.is_finite() && geo > 1e-9 {
        [g[0] / geo, g[1] / geo, g[2] / geo]
    } else {
        [1.0, 1.0, 1.0]
    };
    // Defensive clamps: degenerate statistics must never produce inf/NaN
    // gains; a sane *chromatic* correction lives within ±3 stops per cone.
    let clamp = |v: f64| (v as f32).clamp(2f32.powi(-3), 2f32.powi(3));
    WbGains {
        r: clamp(normalized[0]),
        g: clamp(normalized[1]),
        b: clamp(normalized[2]),
    }
}

// ── the operator ────────────────────────────────────────────────────────────

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Log2-domain monotone curve through the knots
/// `(ε,ε), (black_source→black_target)?, (MID_GRAY→MID_GRAY), (white_source→white_target)?, (1,1)`.
/// Piecewise-linear in log2 — the standard photographic "curve" domain.
struct ShapeCurve {
    /// (source, target) log2 pairs, sorted by source.
    knots: Vec<(f32, f32)>,
}

const CURVE_EPS: f32 = 1.0 / 65_536.0; // one 16-bit code value below black

impl ShapeCurve {
    fn new(p: &ToneParams) -> Self {
        let lg = |v: f32| v.max(CURVE_EPS).log2();
        // Base knots: ε→ε (blacks reachable), mid-gray pinned (contrast
        // must not shift midtones), 1→1 (log2 1 = 0).
        let mut knots: Vec<(f32, f32)> = vec![
            (lg(CURVE_EPS), lg(CURVE_EPS)),
            (lg(MID_GRAY), lg(MID_GRAY)),
            (0.0, 0.0),
        ];
        if let (Some(s), Some(t)) = (p.black_source, p.black_target) {
            knots.push((lg(s), lg(t)));
        }
        if let (Some(s), Some(t)) = (p.white_source, p.white_target) {
            knots.push((lg(s), lg(t)));
        }
        // Sort by source; keep only strictly increasing sources
        // (equal-source duplicates drop deterministically).
        knots.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut cleaned: Vec<(f32, f32)> = Vec::with_capacity(knots.len());
        for k in knots {
            if cleaned.is_empty() || k.0 > cleaned.last().unwrap().0 {
                cleaned.push(k);
            }
        }
        Self { knots: cleaned }
    }

    /// Map x ∈ [0,1] through the curve (identity outside knot span by
    /// construction — first/last knots pin ε→ε and 1→1).
    fn apply(&self, x: f32) -> f32 {
        if x <= CURVE_EPS {
            return x;
        }
        let lx = x.log2();
        let ks = &self.knots;
        debug_assert!(ks.len() >= 2);
        // Find segment.
        let mut i = 0;
        while i + 1 < ks.len() && ks[i + 1].0 < lx {
            i += 1;
        }
        if i + 1 >= ks.len() {
            return x; // above the top knot (== 1.0): identity
        }
        let (x0, y0) = ks[i];
        let (x1, y1) = ks[i + 1];
        if x1 <= x0 {
            return x;
        }
        let t = (lx - x0) / (x1 - x0);
        let ly = y0 + t * (y1 - y0);
        2f32.powf(ly).min(1.0)
    }
}

/// Apply the tone stage to an AP1-linear image. Alpha passes through
/// untouched. Deterministic single pass, no allocation beyond the output.
#[must_use]
pub fn apply(img: &ImageF32, p: &ToneParams) -> ImageF32 {
    let p = p.sanitized();
    if p.is_identity() {
        return img.clone();
    }

    let ch = img.channels().count();
    let mut out = img.clone();

    // Precompute the WB matrix once (AP1→AP1).
    let wb_m: Option<Mat3> = p.wb.filter(|w| !w.is_neutral()).map(|w| {
        let m = wb_gains_to_ap1([f64::from(w.r), f64::from(w.g), f64::from(w.b)]);
        // Fold exposure into the same per-pixel scale pass separately below;
        // keep the matrix pure WB.
        m
    });

    let ev_scale = 2f32.powf(p.ev);
    let k_hi = p.highlights / 100.0 * 0.75; // stops at ±100
    let k_sh = p.shadows / 100.0 * 0.75;
    let sh_lift = p.shadows.max(0.0) / 100.0 * 0.04; // additive floor ≤ 0.04
    let curve = ShapeCurve::new(&p);

    let data = img.data();
    let odata = out.data_mut();
    let npix = img.width() as usize * img.height() as usize;
    for i in 0..npix {
        let base = i * ch;
        let (r, g, b) = (data[base], data[base + 1], data[base + 2]);
        // 1. White balance.
        let (mut r, mut g, mut b) = match &wb_m {
            Some(m) => {
                let v = m.apply_f32([r, g, b]);
                (v[0], v[1], v[2])
            }
            None => (r, g, b),
        };
        // 2. Exposure.
        if p.ev != 0.0 {
            r *= ev_scale;
            g *= ev_scale;
            b *= ev_scale;
        }
        // 3. Highlights / shadows + 4. shaping curve — luma-ratio preserving
        //    (hue/saturation kept; only lightness moves).
        let shaping = p.black_source.is_some() || p.white_source.is_some();
        if p.highlights != 0.0 || p.shadows != 0.0 || shaping {
            let y = (AP1_LUMA[0] * r + AP1_LUMA[1] * g + AP1_LUMA[2] * b).max(0.0);
            let mut y2 = y;
            if p.highlights != 0.0 {
                let w = smoothstep(0.4, 1.0, y);
                y2 *= 2f32.powf(k_hi * w);
            }
            if p.shadows != 0.0 {
                let w = 1.0 - smoothstep(0.0, 0.4, y);
                y2 *= 2f32.powf(k_sh * w);
                y2 += sh_lift * w;
            }
            if shaping {
                y2 = curve.apply(y2);
            }
            if y > 1e-6 {
                let ratio = y2 / y;
                r *= ratio;
                g *= ratio;
                b *= ratio;
            } else if y2 > 0.0 {
                // Near-black with an additive lift: add the lift as flat
                // light (slight desaturation of true blacks — documented).
                r += y2;
                g += y2;
                b += y2;
            }
        }
        // Scene-linear: clamp only at exact zero floor (negatives from the
        // WB matrix must not leak into the working space).
        odata[base] = r.max(0.0);
        odata[base + 1] = g.max(0.0);
        odata[base + 2] = b.max(0.0);
    }
    out
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use melography_core::Channels;

    fn gray_img(v: f32) -> ImageF32 {
        ImageF32::alloc(4, 4, Channels::Rgb, v)
    }

    fn ramp() -> ImageF32 {
        // 8×8 luma ramp 0..1 as gray RGB.
        let mut img = ImageF32::alloc(8, 8, Channels::Rgb, 0.0);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let v = (x * 8 + y) as f32 / 63.0;
                let p = img.pixel_mut(x, y);
                p[0] = v;
                p[1] = v;
                p[2] = v;
            }
        }
        img
    }

    #[test]
    fn default_params_are_exact_identity() {
        let img = ramp();
        let out = apply(&img, &ToneParams::default());
        assert_eq!(img.data(), out.data(), "identity must be byte-exact");
    }

    #[test]
    fn neutral_wb_gains_are_exact_identity() {
        let img = ramp();
        let p = ToneParams {
            wb: Some(WbGains::NEUTRAL),
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        assert_eq!(img.data(), out.data());
    }

    #[test]
    fn ev_scales_by_powers_of_two() {
        let img = gray_img(0.1);
        let p = ToneParams {
            ev: 1.0,
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        let expected = 0.2_f32;
        assert!((out.data()[0] - expected).abs() < 1e-6);
        let p = ToneParams {
            ev: -1.0,
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        assert!((out.data()[0] - 0.05).abs() < 1e-6);
    }

    #[test]
    fn ev_is_clamped_to_five_stops() {
        let p = ToneParams {
            ev: 99.0,
            ..ToneParams::default()
        };
        assert_eq!(p.sanitized().ev, 5.0);
        let p = ToneParams {
            ev: -99.0,
            ..ToneParams::default()
        };
        assert_eq!(p.sanitized().ev, -5.0);
    }

    #[test]
    fn highlights_recovery_reduces_top_preserves_bottom() {
        let img = ramp();
        let p = ToneParams {
            highlights: -100.0,
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        let y = |o: &ImageF32, x: u32, y: u32| o.pixel(x, y)[1];
        // Bottom of the ramp (mask ≈ 0) untouched.
        assert!((y(&out, 0, 0) - y(&img, 0, 0)).abs() < 1e-7);
        // Top of the ramp (mask = 1) reduced by 2^(-0.75).
        let top_in = y(&img, 7, 7);
        let top_out = y(&out, 7, 7);
        let expected = top_in * 2f32.powf(-0.75);
        assert!((top_out - expected).abs() < 1e-5);
    }

    #[test]
    fn shadows_lift_raises_floor_but_not_highlights() {
        let img = ramp();
        let p = ToneParams {
            shadows: 100.0,
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        let y = |o: &ImageF32, x: u32, y: u32| o.pixel(x, y)[1];
        // Near-black: additive lift ≈ 0.04 (mask ≈ 1, k_sh=0.75 → mult
        // negligible at tiny y).
        let lifted = y(&out, 0, 0);
        assert!(lifted > 0.03 && lifted < 0.05, "lifted={lifted}");
        // Highlights (mask = 0) untouched.
        assert!((y(&out, 7, 7) - y(&img, 7, 7)).abs() < 1e-7);
    }

    #[test]
    fn shaping_curve_maps_recorded_knots_and_pins_midgray() {
        let img = ramp();
        let p = ToneParams {
            black_source: Some(0.05),
            black_target: Some(0.02),
            white_source: Some(0.8),
            white_target: Some(0.95),
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        // The curve operates on luma after identity HL/SH; gray pixels scale
        // by ratio = curve(y)/y.
        for i in 0..img.data().len() {
            let y_in = img.data()[i];
            let y_out = out.data()[i];
            let expected = curve_value(&p, y_in);
            assert!(
                (y_out - expected).abs() < 1e-5,
                "y_in={y_in} y_out={y_out} expected={expected}"
            );
        }
    }

    fn curve_value(p: &ToneParams, x: f32) -> f32 {
        // Reference implementation (recomputes the same curve directly).
        let curve = ShapeCurve::new(p);
        curve.apply(x)
    }

    #[test]
    fn shaping_is_monotone() {
        let p = ToneParams {
            black_source: Some(0.05),
            black_target: Some(0.01),
            white_source: Some(0.75),
            white_target: Some(0.97),
            ..ToneParams::default()
        };
        let curve = ShapeCurve::new(&p);
        let mut prev = -1.0;
        let mut x = CURVE_EPS;
        while x <= 1.0 {
            let y = curve.apply(x);
            assert!(y >= prev - 1e-7, "non-monotone at x={x}: {y} < {prev}");
            prev = y;
            x += 0.001;
        }
        // Knot fidelity.
        assert!((curve.apply(0.05) - 0.01).abs() < 1e-4);
        assert!((curve.apply(0.75) - 0.97).abs() < 1e-4);
        assert!((curve.apply(MID_GRAY) - MID_GRAY).abs() < 1e-5);
    }

    #[test]
    fn degenerate_knots_are_dropped() {
        let mut p = ToneParams {
            black_source: Some(0.5), // > MID_GRAY — invalid
            black_target: Some(0.02),
            ..ToneParams::default()
        };
        let s = p.sanitized();
        assert_eq!(s.black_source, None);
        assert_eq!(s.black_target, None);
        // Half a pair → dropped entirely.
        p = ToneParams {
            white_source: Some(0.8),
            ..ToneParams::default()
        };
        let s = p.sanitized();
        assert_eq!(s.white_source, None);
        assert_eq!(s.white_target, None);
    }

    #[test]
    fn wb_neutralizes_a_known_cast() {
        // Gray 0.18 under a warm cast: simulate by multiplying channels.
        let mut img = ImageF32::alloc(8, 8, Channels::Rgb, 0.0);
        for i in 0..img.data().len() / 3 {
            let c = &mut img.data_mut()[i * 3..i * 3 + 3];
            c[0] = 0.18 * 1.4; // warm
            c[1] = 0.18;
            c[2] = 0.18 * 0.7;
        }
        // Gains that undo the cast in *cone space* are not simply the
        // reciprocal of the RGB multipliers — derive them with the public
        // analysis helper (the exact path the auto-lighting baseline uses).
        let gains = analysis_gains_for_gray([0.18 * 1.4, 0.18, 0.18 * 0.7]);
        assert!(gains.is_valid());
        let p = ToneParams {
            wb: Some(gains),
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        // After correction the gray must be neutral in AP1: all channels
        // equal within tolerance (von Kries round-trip through the public
        // analysis↔synthesis pair).
        for i in 0..out.data().len() / 3 {
            let c = &out.data()[i * 3..i * 3 + 3];
            let spread = (c[0] - c[1]).abs().max((c[2] - c[1]).abs());
            assert!(spread < 2e-3, "not neutral: {c:?}");
        }
    }

    #[test]
    fn dampen_moves_gains_toward_neutral() {
        let g = WbGains {
            r: 1.5,
            g: 1.0,
            b: 0.6,
        };
        let d = g.dampen(1.0);
        assert!(d.is_neutral());
        let d = g.dampen(0.0);
        assert!((d.r - 1.5).abs() < 1e-6);
    }

    #[test]
    fn invalid_wb_is_dropped_by_sanitize() {
        let p = ToneParams {
            wb: Some(WbGains {
                r: -1.0,
                g: 0.0,
                b: f32::NAN,
            }),
            ..ToneParams::default()
        };
        assert_eq!(p.sanitized().wb, None);
    }

    #[test]
    fn cct_estimate_is_display_sane() {
        // Neutral → the AP1 white (D60-ish) ≈ 5500–6500 K.
        let cct = estimate_cct_k(&WbGains::NEUTRAL).expect("neutral has a CCT");
        assert!((5000.0..=7000.0).contains(&cct), "cct={cct}");
        // A strong blue-boost correction implies a warm illuminant.
        let warm = estimate_cct_k(&WbGains {
            r: 0.8,
            g: 1.0,
            b: 1.3,
        });
        let cool = estimate_cct_k(&WbGains {
            r: 1.3,
            g: 1.0,
            b: 0.8,
        });
        if let (Some(w), Some(c)) = (warm, cool) {
            assert!(w < c, "blue-boost ⇒ warm illuminant: {w} !< {c}");
        }
    }

    #[test]
    fn alpha_passes_through() {
        let mut img = ImageF32::alloc(2, 2, Channels::Rgba, 0.0);
        for i in 0..img.data().len() / 4 {
            let c = &mut img.data_mut()[i * 4..i * 4 + 4];
            c[0] = 0.2;
            c[1] = 0.3;
            c[2] = 0.4;
            c[3] = 0.55;
        }
        let p = ToneParams {
            ev: 1.5,
            ..ToneParams::default()
        };
        let out = apply(&img, &p);
        for i in 0..out.data().len() / 4 {
            assert!((out.data()[i * 4 + 3] - 0.55).abs() < 1e-7);
        }
    }

    #[test]
    fn sanitized_params_serde_roundtrip() {
        let p = ToneParams {
            ev: 0.4,
            highlights: -12.0,
            shadows: 5.0,
            wb: Some(WbGains {
                r: 1.1,
                g: 1.0,
                b: 0.9,
            }),
            black_source: Some(0.05),
            black_target: Some(0.02),
            white_source: Some(0.8),
            white_target: Some(0.95),
        }
        .sanitized();
        let s = serde_json::to_string(&p).unwrap();
        let back: ToneParams = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
    }
}
