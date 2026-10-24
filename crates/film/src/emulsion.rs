//! Stages C–E — emulsion exposure, characteristic response, chemistry
//! (FILM_MODEL §C, §D, §E).
//!
//! Pipeline within the emulsion block (negative stocks):
//!
//! ```text
//! E (AP1-linear exposure, per-pixel)
//!  → C1  X = M_exc · E                    (layer excitation)
//!  → C2  X += H(X)                        (halation, red-biased, margins)
//!  → C3  X *= s                           (18 % gray → mid-density anchor)
//!  → E(pp) X *= 2^(±stops·0.5)            (push/pull exposure shift)
//!  → D   D_c = D_min,c + f_c(log10 X_c)   (H&D monotone splines)
//!  → D   D += C·D_off                     (inter-layer crosstalk)
//!  → E(γ) f mid-pivot contrast            (push/pull + dev contrast)
//!  → E(xp) curve-set remap                (cross-process [ART])
//!  → E(bb) silver blend                   (bleach bypass [ART])
//! ```
//!
//! Reversal stocks: `f_c` is decreasing (validated in the profile), and
//! the print stage skips negative inversion. All math f64 on f32 storage
//! (S2 discipline); the halation convolution runs through the shared
//! gaussian infrastructure with unit-DC-gain kernels, so the energy guard
//! `α ≤ ε` is exact for constant fields and conservative otherwise.

use crate::curves::MonotoneSpline;
use crate::optics::{GaussianKernel, Plane};
use crate::profile::{InversionPath, LayerLayout, Mat3, Polarity, StockProfile};
use serde::{Deserialize, Serialize};

/// Recipe-level chemistry/artistic selections (§E).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CrossProcessMode {
    #[default]
    None,
    /// E6 stock in C41 chemistry: harder red curve, lifted D_min,
    /// g/b curve swap — "process-inspired" `[ART]`, no chemical claim.
    E6InC41,
    /// C41 stock in E6 chemistry: softer curves, shifted mask.
    C41InE6,
}

/// Compiled emulsion parameters (profile + adjustments resolved).
pub struct Emulsion {
    pub polarity: Polarity,
    pub layers: LayerLayout,
    pub m_exc: Mat3,
    pub crosstalk: Mat3,
    pub splines: Vec<MonotoneSpline>,
    pub d_min: [f64; 3],
    pub x_floor: f64,
    /// 18 % gray anchor: `s = 10^(mid_log_e − mean_c log10 X_18)`.
    pub exposure_scale: f64,
    /// Halation kernels per channel (σ in *pixels* at the render scale)
    /// plus gains — resolved from the profile + intensity adjustment.
    pub halation: [HalationKernel; 3],
    /// Effective push/pull exposure factor `2^(stops·0.5)`.
    pub push_exposure: f64,
    /// Effective γ multiplier (clamped so effective γ stays in [0.3, 1.6]).
    pub gamma_mult: [f64; 3],
    /// Crosstalk scale from dev-contrast saturation coupling.
    pub crosstalk_scale: f64,
    pub cross_process: CrossProcessMode,
    pub bleach_bypass: f64,
}

/// Per-channel halation kernel (unit DC gain ⇒ gain == α).
pub struct HalationKernel {
    pub alpha: f64,
    pub kernel: GaussianKernel,
}

impl HalationKernel {
    fn disabled() -> Self {
        Self {
            alpha: 0.0,
            kernel: GaussianKernel::new(0.0),
        }
    }
    pub fn margin(&self) -> u32 {
        self.kernel.radius_px
    }
}

/// Margins (rows) the emulsion block needs on its input.
impl Emulsion {
    pub fn input_margin(&self) -> u32 {
        self.halation.iter().map(|h| h.margin()).max().unwrap_or(0)
    }
}

/// Recipe-level adjustments (user knobs over the stock profile).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Adjustments {
    /// Film chain master switch.
    pub enabled: bool,
    /// Exposure compensation in stops (§A): `E ← E · 2^EV`.
    pub ev: f64,
    /// Push/pull processing in stops (§E): exposure `2^(±s·0.5)` + γ
    /// coupling. Range [-3, 3].
    pub push_pull_stops: f64,
    /// Development contrast multiplier (§E). Range [0.6, 1.6].
    pub dev_contrast: f64,
    /// Print/scan exposure offset in stops (§F): scales `L_p`.
    pub print_exposure_stops: f64,
    /// Inversion path override (None = profile default).
    pub inversion_path: Option<InversionPath>,
    /// Grain intensity scale on σ(D) (§G). Range [0, 2].
    pub grain_intensity: f64,
    /// Halation intensity scale on α (§C). Range [0, 1] — can only
    /// attenuate; the ε energy guard must never be exceeded.
    pub halation_intensity: f64,
    /// Grain seed override (determinism knob).
    pub grain_seed: Option<u32>,
    /// Cross-process mode (§E [ART]).
    pub cross_process: CrossProcessMode,
    /// Bleach bypass weight (§E [ART]). Range [0, 1].
    pub bleach_bypass: f64,
    /// Phone-input pre-desaturation (§A5 [ART]). Range [0, 1].
    pub pre_desaturate: f64,
}

impl Default for Adjustments {
    fn default() -> Self {
        Self {
            enabled: true,
            ev: 0.0,
            push_pull_stops: 0.0,
            dev_contrast: 1.0,
            print_exposure_stops: 0.0,
            inversion_path: None,
            grain_intensity: 1.0,
            halation_intensity: 1.0,
            grain_seed: None,
            cross_process: CrossProcessMode::None,
            bleach_bypass: 0.0,
            pre_desaturate: 0.0,
        }
    }
}

impl Adjustments {
    pub fn validate(&self) -> Result<(), String> {
        for (name, v, lo, hi) in [
            ("ev", self.ev, -10.0, 10.0),
            ("push_pull_stops", self.push_pull_stops, -3.0, 3.0),
            ("dev_contrast", self.dev_contrast, 0.6, 1.6),
            ("print_exposure_stops", self.print_exposure_stops, -5.0, 5.0),
            ("grain_intensity", self.grain_intensity, 0.0, 2.0),
            ("halation_intensity", self.halation_intensity, 0.0, 1.0),
            ("bleach_bypass", self.bleach_bypass, 0.0, 1.0),
            ("pre_desaturate", self.pre_desaturate, 0.0, 1.0),
        ] {
            if !(v.is_finite() && (lo..=hi).contains(&v)) {
                return Err(format!("adjustments.{name} must be within [{lo}, {hi}]"));
            }
        }
        Ok(())
    }
}

/// Resolve the emulsion from a validated profile + adjustments + the
/// render's film frame (halation σ_mm → σ_px).
pub fn resolve_emulsion(profile: &StockProfile, adj: &Adjustments, mm_per_pixel: f64) -> Emulsion {
    // C3 anchor: X_18 = M_exc · (0.18, 0.18, 0.18); s puts the *mean of
    // per-layer logs* at mid_log_e (v1 convention, documented).
    let x18 = profile.m_exc.apply([0.18, 0.18, 0.18]);
    let mean_log =
        (x18[0].max(1e-12).log10() + x18[1].max(1e-12).log10() + x18[2].max(1e-12).log10()) / 3.0;
    let exposure_scale = 10f64.powf(profile.mid_log_e - mean_log);

    // Halation kernels at render scale; intensity only attenuates (≤ 1).
    let intensity = adj.halation_intensity.clamp(0.0, 1.0);
    let mut halation = [
        HalationKernel::disabled(),
        HalationKernel::disabled(),
        HalationKernel::disabled(),
    ];
    for (c, hc) in profile.halation.iter().enumerate() {
        let alpha = hc.alpha * intensity;
        if alpha <= 0.0 {
            continue;
        }
        // Mixture → single combined 1D kernel: weighted sum of per-component
        // gaussians, then unit-DC normalized. Radius from the largest σ.
        let sigma_max = hc.kernel.iter().map(|g| g.sigma_mm).fold(0.0f64, f64::max);
        let sigma_px = sigma_max / mm_per_pixel;
        if sigma_px < 0.05 {
            // Sub-pixel halo: still a gain on the field itself (constant
            // term) — approximate with the identity kernel.
            halation[c] = HalationKernel {
                alpha,
                kernel: GaussianKernel::new(0.0),
            };
            continue;
        }
        // Combined kernel via weighted tap sum (deterministic order).
        let radius = (3.0 * sigma_px).ceil() as i64;
        let wsum: f64 = hc.kernel.iter().map(|g| g.weight).sum();
        let mut taps = Vec::with_capacity((2 * radius + 1) as usize);
        for i in -radius..=radius {
            let d = i as f64 * mm_per_pixel; // mm from center
            let mut t = 0.0;
            for g in &hc.kernel {
                let s_mm = g.sigma_mm;
                t += g.weight * (-(d * d) / (2.0 * s_mm * s_mm)).exp();
            }
            taps.push(t / wsum);
        }
        let sum: f64 = taps.iter().sum();
        for t in &mut taps {
            *t /= sum;
        }
        halation[c] = HalationKernel {
            alpha,
            kernel: GaussianKernel::from_taps(taps, radius as u32),
        };
    }

    // Chemistry (§E).
    let p = adj.push_pull_stops;
    let push_exposure = 2f64.powf(p * 0.5);
    let gamma_push = 1.0 + 0.25 * p; // contrast couples to development
    let mut gamma_mult = [gamma_push * adj.dev_contrast; 3];
    if let CrossProcessMode::E6InC41 = adj.cross_process {
        gamma_mult[0] *= 1.3; // harder red (process-inspired [ART])
    } else if let CrossProcessMode::C41InE6 = adj.cross_process {
        for g in &mut gamma_mult {
            *g *= 0.8; // softer overall
        }
    }
    // Effective γ guard: base γ per layer from the spline, clamp the
    // multiplier so γ_eff ∈ [0.3, 1.6] (sliders cannot create physically
    // undefined states — §E guards). Monochrome has one active layer.
    let base_splines = profile.hd_splines();
    let n_active = base_splines.len();
    for (c, gm) in gamma_mult.iter_mut().enumerate().take(n_active) {
        let base_gamma = base_splines[c].max_gradient();
        let eff = base_gamma * *gm;
        if eff < 0.3 {
            *gm = 0.3 / base_gamma;
        } else if eff > 1.6 {
            *gm = 1.6 / base_gamma;
        }
    }
    // Saturation coupling via crosstalk scale (dev contrast > 1 → cleaner).
    let crosstalk_scale = (1.0 - 0.5 * (adj.dev_contrast - 1.0)).clamp(0.4, 1.6);

    // Curve-set remap for cross-process [ART]: E6InC41 swaps g/b curves.
    let mut splines = base_splines;
    if adj.cross_process == CrossProcessMode::E6InC41 && profile.layers == LayerLayout::Color {
        splines.swap(1, 2);
    }

    Emulsion {
        polarity: profile.polarity,
        layers: profile.layers,
        m_exc: profile.m_exc,
        crosstalk: profile.crosstalk,
        splines,
        d_min: profile.d_min,
        x_floor: profile.x_floor,
        exposure_scale,
        halation,
        push_exposure,
        gamma_mult,
        crosstalk_scale,
        cross_process: adj.cross_process,
        bleach_bypass: adj.bleach_bypass,
    }
}

/// §A + §C1 + §C3 + §E(pp exposure): exposure conditioning and layer
/// excitation (per-pixel, no margin). `ev` from adjustments (§A).
pub fn apply_excitation(
    src: &Plane,
    dst: &mut Plane,
    y0: u32,
    y1: u32,
    em: &Emulsion,
    ev_stops: f64,
    pre_desaturate: f64,
) {
    let ev_factor = 2f64.powf(ev_stops);
    let total_scale = em.exposure_scale * em.push_exposure * ev_factor;
    for y in y0..y1 {
        for x in 0..src.w {
            let mut e = src.px64(x, y);
            // §A5 phone pre-treatment [ART]: pull saturation down.
            if pre_desaturate > 0.0 {
                let lum = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
                for e_c in &mut e {
                    *e_c = *e_c * (1.0 - pre_desaturate) + lum * pre_desaturate;
                }
            }
            let mut x_layer = em.m_exc.apply(e);
            for v in &mut x_layer {
                *v *= total_scale;
            }
            dst.set3(x, y, x_layer);
        }
    }
}

/// In-place excitation (per-pixel read-then-write on the same plane).
pub fn apply_excitation_inplace(
    plane: &mut Plane,
    y0: u32,
    y1: u32,
    em: &Emulsion,
    ev_stops: f64,
    pre_desaturate: f64,
) {
    let ev_factor = 2f64.powf(ev_stops);
    let total_scale = em.exposure_scale * em.push_exposure * ev_factor;
    for y in y0..y1 {
        for x in 0..plane.w {
            let mut e = plane.px64(x, y);
            if pre_desaturate > 0.0 {
                let lum = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
                for e_c in &mut e {
                    *e_c = *e_c * (1.0 - pre_desaturate) + lum * pre_desaturate;
                }
            }
            let mut x_layer = em.m_exc.apply(e);
            for v in &mut x_layer {
                *v *= total_scale;
            }
            plane.set3(x, y, x_layer);
        }
    }
}

/// §C2 halation: `X_halo = X + H(X)` with per-channel kernels and gains.
///
/// Requires the rows `[valid_rows.0, valid_rows.1)` of `src` to hold
/// **valid** data; the output window `[y0, y1)` expanded by the kernel
/// radius fits inside that range up to the *image* edge. Taps outside the
/// valid range are zero padding (no film / no data), never stale plane
/// memory — this is what makes strip rendering byte-identical to
/// monolithic (§L tile gate). Separable evaluation: horizontal pass into
/// a row-scratch, vertical pass accumulating the α-weighted halo straight
/// into `dst` — every output pixel reads the same fixed tap window in the
/// same order. Monochrome profiles use the red row for the single layer.
pub fn apply_halation(
    src: &Plane,
    dst: &mut Plane,
    y0: u32,
    y1: u32,
    em: &Emulsion,
    valid_rows: (u32, u32),
) {
    // Pass-through first (channels without an active kernel stay exact).
    for y in y0..y1 {
        for x in 0..src.w {
            dst.set3(x, y, src.px64(x, y));
        }
    }
    let layers: &[usize] = match em.layers {
        LayerLayout::Color => &[0, 1, 2],
        LayerLayout::Monochrome => &[0],
    };
    for &c in layers {
        let hk = &em.halation[c];
        if hk.alpha <= 0.0 || hk.kernel.is_identity() {
            continue;
        }
        let r = hk.kernel.radius_px as i64;
        let taps = &hk.kernel.taps;
        let w_i = src.w as i64;
        // Row range needing a horizontal pass (vertical window of the
        // output strip, clamped to the *valid* rows — zero padding
        // outside, matching the monolithic render at image edges).
        let sy_lo = (y0 as i64 - r).max(i64::from(valid_rows.0));
        let sy_hi = (y1 as i64 + r).min(i64::from(valid_rows.1));
        let mut scratch = vec![0.0f64; (sy_hi - sy_lo) as usize * src.w as usize];
        // Horizontal pass (fixed tap order ⇒ deterministic).
        let row_of = |sy: i64| (sy - sy_lo) as usize * src.w as usize;
        for sy in sy_lo..sy_hi {
            for x in 0..src.w {
                let mut acc = 0.0f64;
                for (ti, t) in taps.iter().enumerate() {
                    let sx = x as i64 + ti as i64 - r;
                    if sx < 0 || sx >= w_i {
                        continue; // zero padding beyond the film edge
                    }
                    acc += t * f64::from(src.px(sx as u32, sy as u32)[c]);
                }
                scratch[row_of(sy) + x as usize] = acc;
            }
        }
        // Vertical pass + additive halo into dst.
        for y in y0..y1 {
            for x in 0..src.w {
                let mut halo = 0.0f64;
                for (ti, t) in taps.iter().enumerate() {
                    let sy = y as i64 + ti as i64 - r;
                    if sy < sy_lo || sy >= sy_hi {
                        continue;
                    }
                    halo += t * scratch[row_of(sy) + x as usize];
                }
                let mut v = dst.px64(x, y);
                v[c] += hk.alpha * halo;
                dst.set3(x, y, v);
            }
        }
    }
}

/// §D characteristic response + §E chemistry post-modifiers (per-pixel,
/// no margin): H&D splines with `X_floor` clamp, γ pivot from push/pull +
/// dev contrast, inter-layer crosstalk, cross-process D_min shift, and
/// bleach-bypass silver blend `[ART]`.
///
/// Convention (spec-literal): `D_c = D_min,c + f_c(log10 max(X_c, X_floor))`
/// — control points are the response **above fog**; `d_min` is the fog
/// level. Splines extrapolate flat; results are clamped to the spline's
/// endpoint range, so densities are finite and bounded for any input
/// (FILM_MODEL §D numerical edges).
pub fn apply_response(src: &Plane, dst: &mut Plane, y0: u32, y1: u32, em: &Emulsion) {
    let layers: &[usize] = match em.layers {
        LayerLayout::Color => &[0, 1, 2],
        LayerLayout::Monochrome => &[0],
    };
    // Precompute per-layer pivot values and clamps.
    let mut f_mid = [0.0f64; 3];
    let mut f_lo = [0.0f64; 3];
    let mut f_hi = [0.0f64; 3];
    for (li, &c) in layers.iter().enumerate() {
        let sp = &em.splines[li];
        let (x0, x1) = sp.x_range();
        f_mid[c] = sp.eval(0.5 * (x0 + x1)); // pivot: control-range center
        f_lo[c] = sp.eval(x0).min(sp.eval(x1));
        f_hi[c] = sp.eval(x0).max(sp.eval(x1));
    }
    // Cross-process D_min shift (process-inspired [ART]).
    let mut d_min = em.d_min;
    if em.cross_process == CrossProcessMode::E6InC41 {
        d_min[0] += 0.08; // lifted red fog
    } else if em.cross_process == CrossProcessMode::C41InE6 {
        for v in &mut d_min {
            *v = (*v * 0.6).max(0.0); // cleaner fog
        }
    }
    let c_scale = em.crosstalk_scale;
    let bb = em.bleach_bypass;
    for y in y0..y1 {
        for x in 0..src.w {
            let x_layer = src.px64(x, y);
            let mut d = [0.0f64; 3];
            for (li, &c) in layers.iter().enumerate() {
                let sp = &em.splines[li];
                let log_e = x_layer[c].max(em.x_floor).log10();
                let f = sp.eval(log_e);
                let mut f_g = f_mid[c] + em.gamma_mult[li] * (f - f_mid[c]);
                if f_g < f_lo[c] {
                    f_g = f_lo[c];
                } else if f_g > f_hi[c] {
                    f_g = f_hi[c];
                }
                d[c] = d_min[c] + f_g;
            }
            // Inter-layer crosstalk (§D): D ← D + C·D_off.
            let ct = &em.crosstalk;
            let d_in = d;
            for &i in layers {
                let mut off = 0.0;
                for &j in layers {
                    if i != j {
                        off += ct.0[3 * i + j] * d_in[j];
                    }
                }
                d[i] += c_scale * off;
            }
            // Bleach bypass [ART]: blend a luminance-weighted high-contrast
            // silver density into all dye layers.
            if bb > 0.0 && em.layers == LayerLayout::Color {
                let lum = 0.25 * d[0] + 0.5 * d[1] + 0.25 * d[2];
                let silver = ((lum - 0.15) * 1.9).clamp(0.0, 2.3);
                for d_c in &mut d {
                    *d_c = (1.0 - bb) * *d_c + bb * silver;
                }
            }
            // Densities are non-negative by construction (flat toe clamp +
            // non-negative f below pivot handling via lo clamp); enforce
            // the absolute bound defensively.
            for d_c in &mut d {
                if *d_c < 0.0 {
                    *d_c = 0.0;
                }
            }
            dst.set3(x, y, d);
        }
    }
}

/// In-place response (per-pixel read-then-write on the same plane).
pub fn apply_response_inplace(plane: &mut Plane, y0: u32, y1: u32, em: &Emulsion) {
    let layers: &[usize] = match em.layers {
        LayerLayout::Color => &[0, 1, 2],
        LayerLayout::Monochrome => &[0],
    };
    let mut f_mid = [0.0f64; 3];
    let mut f_lo = [0.0f64; 3];
    let mut f_hi = [0.0f64; 3];
    for (li, &c) in layers.iter().enumerate() {
        let sp = &em.splines[li];
        let (x0, x1) = sp.x_range();
        f_mid[c] = sp.eval(0.5 * (x0 + x1));
        f_lo[c] = sp.eval(x0).min(sp.eval(x1));
        f_hi[c] = sp.eval(x0).max(sp.eval(x1));
    }
    let mut d_min = em.d_min;
    if em.cross_process == CrossProcessMode::E6InC41 {
        d_min[0] += 0.08;
    } else if em.cross_process == CrossProcessMode::C41InE6 {
        for v in &mut d_min {
            *v = (*v * 0.6).max(0.0);
        }
    }
    let c_scale = em.crosstalk_scale;
    let bb = em.bleach_bypass;
    for y in y0..y1 {
        for x in 0..plane.w {
            let x_layer = plane.px64(x, y);
            let mut d = [0.0f64; 3];
            for (li, &c) in layers.iter().enumerate() {
                let sp = &em.splines[li];
                let log_e = x_layer[c].max(em.x_floor).log10();
                let f = sp.eval(log_e);
                let mut f_g = f_mid[c] + em.gamma_mult[li] * (f - f_mid[c]);
                if f_g < f_lo[c] {
                    f_g = f_lo[c];
                } else if f_g > f_hi[c] {
                    f_g = f_hi[c];
                }
                d[c] = d_min[c] + f_g;
            }
            let ct = &em.crosstalk;
            let d_in = d;
            for &i in layers {
                let mut off = 0.0;
                for &j in layers {
                    if i != j {
                        off += ct.0[3 * i + j] * d_in[j];
                    }
                }
                d[i] += c_scale * off;
            }
            if bb > 0.0 && em.layers == LayerLayout::Color {
                let lum = 0.25 * d[0] + 0.5 * d[1] + 0.25 * d[2];
                let silver = ((lum - 0.15) * 1.9).clamp(0.0, 2.3);
                for d_c in &mut d {
                    *d_c = (1.0 - bb) * *d_c + bb * silver;
                }
            }
            for d_c in &mut d {
                if *d_c < 0.0 {
                    *d_c = 0.0;
                }
            }
            plane.set3(x, y, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::testing::test_profile;
    use crate::profile::HalationChannel;

    fn plane_const(w: u32, h: u32, v: f64) -> Plane {
        Plane {
            w,
            h,
            data: vec![v as f32; (w * h * 3) as usize],
        }
    }

    #[test]
    fn adjustments_ranges() {
        assert!(Adjustments::default().validate().is_ok());
        let mut a = Adjustments {
            push_pull_stops: 5.0,
            ..Adjustments::default()
        };
        assert!(a.validate().is_err());
        a.push_pull_stops = -3.0;
        assert!(a.validate().is_ok());
    }

    #[test]
    fn excitation_anchors_18_percent_gray() {
        // With default adjustments, an 18 % gray input must land at the
        // profile's mid_log_e on the mean of per-layer logs (§C3).
        let p = test_profile();
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.006);
        let src = plane_const(4, 4, 0.18);
        let mut dst = Plane::new(4, 4);
        apply_excitation(&src, &mut dst, 0, 4, &em, 0.0, 0.0);
        let x = dst.px64(1, 1);
        let mean_log = (x[0].log10() + x[1].log10() + x[2].log10()) / 3.0;
        // f32 plane storage round-trips the f64 excitation (~1e-7 rel).
        assert!(
            (mean_log - p.mid_log_e).abs() < 1e-6,
            "mean_log {mean_log} vs mid_log_e {}",
            p.mid_log_e
        );
    }

    #[test]
    fn halation_energy_guard_on_constant_field() {
        // Unit-DC kernels ⇒ constant field X maps to X + α·X exactly:
        // the added exposure equals α ≤ ε (FILM_MODEL §C2 gate).
        let p = test_profile();
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.01); // coarse: kernel active
        let w = 64;
        let src = plane_const(w, w, 1.0);
        let mut dst = Plane::new(w, w);
        apply_halation(&src, &mut dst, 4, w - 4, &em, (0, w));
        // Interior pixels (away from the zero-padded border).
        for y in 20..44 {
            for x in 20..44 {
                let v = dst.px64(x, y);
                for c in 0..3 {
                    let alpha = em.halation[c].alpha;
                    assert!(
                        (v[c] - (1.0 + alpha)).abs() < 1e-4,
                        "channel {c}: {v:?} vs {}",
                        1.0 + alpha
                    );
                }
            }
        }
    }

    #[test]
    fn halation_impulse_energy_equals_alpha() {
        // Σ response to a unit impulse = α (per channel) — energy
        // accounting, independent of kernel shape.
        let p = test_profile();
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.01);
        let w = 96;
        let mut src = Plane::new(w, w);
        src.set3(48, 48, [1.0, 1.0, 1.0]);
        let mut dst = Plane::new(w, w);
        apply_halation(&src, &mut dst, 8, w - 8, &em, (0, w));
        let mut sum = [0.0f64; 3];
        for y in 0..w {
            for x in 0..w {
                let v = dst.px64(x, y);
                let s0 = src.px64(x, y);
                for c in 0..3 {
                    sum[c] += v[c] - s0[c]; // halo only
                }
            }
        }
        for (c, &s) in sum.iter().enumerate() {
            assert!(
                (s - em.halation[c].alpha).abs() < 1e-3,
                "channel {c}: impulse energy {s} vs α {}",
                em.halation[c].alpha
            );
        }
    }

    #[test]
    fn response_is_monotone_in_exposure_for_negative() {
        // Property (§L gate, per-pixel): increasing excitation must never
        // decrease density for a negative stock (crosstalk off).
        let mut p = test_profile();
        p.crosstalk = Mat3([0.0; 9]);
        p.print.scanner_matrix = Mat3::IDENTITY;
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.006);
        let w = 64;
        // A horizontal ramp of excitations 1e-4 .. 10.
        let mut src = Plane::new(w, 1);
        for x in 0..w {
            let t = x as f64 / (w - 1) as f64;
            let e = 10f64.powf(-4.0 + 4.0 * t);
            src.set3(x, 0, [e, e, e]);
        }
        let mut dst = Plane::new(w, 1);
        apply_response(&src, &mut dst, 0, 1, &em);
        let mut prev = [f64::NEG_INFINITY; 3];
        for x in 0..w {
            let d = dst.px64(x, 0);
            for c in 0..3 {
                assert!(d[c] >= prev[c] - 1e-9, "not monotone at x={x} c={c}");
                prev[c] = d[c];
            }
        }
    }

    #[test]
    fn response_never_nan_or_negative_on_hostile_input() {
        let p = test_profile();
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.006);
        let mut src = Plane::new(8, 1);
        let hostile = [-1e30, -1.0, 0.0, 1e-9, 0.18, 1.0, 1e30, f64::NAN];
        for (x, &v) in hostile.iter().enumerate() {
            src.set3(x as u32, 0, [v, v, v]);
        }
        let mut dst = Plane::new(8, 1);
        apply_response(&src, &mut dst, 0, 1, &em);
        for x in 0..8 {
            let d = dst.px64(x, 0);
            for c in 0..3 {
                assert!(d[c].is_finite() && d[c] >= 0.0, "x={x} c={c}: {d:?}");
            }
        }
    }

    #[test]
    fn gamma_pivot_stays_in_curve_bounds() {
        // Extreme push + max dev contrast: effective γ clamped, densities
        // bounded by spline endpoints (§E guards).
        let p = test_profile();
        let adj = Adjustments {
            push_pull_stops: 3.0,
            dev_contrast: 1.6,
            ..Default::default()
        };
        assert!(adj.validate().is_ok());
        let em = resolve_emulsion(&p, &adj, 0.006);
        let mut src = Plane::new(16, 1);
        for x in 0..16 {
            let e = 10f64.powf(-4.0 + 4.0 * (x as f64 / 15.0));
            src.set3(x, 0, [e, e, e]);
        }
        let mut dst = Plane::new(16, 1);
        apply_response(&src, &mut dst, 0, 1, &em);
        for x in 0..16 {
            let d = dst.px64(x, 0);
            for &d_c in &d {
                assert!(d_c.is_finite() && (0.0..=3.0).contains(&d_c));
            }
        }
    }

    #[test]
    fn crosstalk_mixes_layers() {
        // Positive off-diagonal C raises a layer whose neighbors are dense.
        let mut p = test_profile();
        p.crosstalk = Mat3([
            0.0, 0.2, 0.0, //
            0.0, 0.0, 0.0, //
            0.0, 0.0, 0.0,
        ]);
        let em = resolve_emulsion(&p, &Adjustments::default(), 0.006);
        let mut src = Plane::new(2, 1);
        // Layer values chosen so r-curve output is low, g high.
        src.set3(0, 0, [1e-4, 1.0, 1e-4]);
        src.set3(1, 0, [1e-4, 1e-4, 1e-4]);
        let mut dst = Plane::new(2, 1);
        apply_response(&src, &mut dst, 0, 1, &em);
        let with_neighbor = dst.px64(0, 0)[0];
        let without = dst.px64(1, 0)[0];
        assert!(
            with_neighbor > without,
            "red density must rise when green layer is dense: {with_neighbor} vs {without}"
        );
    }

    #[test]
    fn halation_intensity_cannot_exceed_energy_guard() {
        let mut p = test_profile();
        p.halation = [
            HalationChannel {
                alpha: 0.05,
                kernel: test_profile().halation[0].kernel.clone(),
            },
            HalationChannel {
                alpha: 0.0,
                kernel: test_profile().halation[2].kernel.clone(),
            },
            HalationChannel {
                alpha: 0.0,
                kernel: test_profile().halation[2].kernel.clone(),
            },
        ];
        let adj = Adjustments {
            halation_intensity: 1.0,
            ..Default::default()
        };
        let em = resolve_emulsion(&p, &adj, 0.01);
        assert!(em.halation[0].alpha <= p.halation_epsilon + 1e-12);
    }
}
