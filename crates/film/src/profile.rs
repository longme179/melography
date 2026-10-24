//! Stock profile data types (FILM_MODEL §J) — data, not code.
//!
//! A `StockProfile` is a *versioned, provenance-labelled, strictly validated*
//! JSON document. Everything the film chain does is parametrized by these
//! fields plus recipe-level adjustments (`crate::ChainParams`); profiles
//! never ship code.
//!
//! Validation is deny-by-default: every field has explicit ranges
//! (FILM_MODEL §D/§E guards), and `StockProfile::validate` is the single
//! gate both the loader (melography-presets) and the fit tool output must
//! pass. Ranges exist so sliders can never create physically undefined
//! states (non-monotone response, negative densities, exploding kernels).

use crate::curves::{CurveDirection, MonotoneSpline};
use serde::{Deserialize, Serialize};

/// Bump on breaking profile-schema changes (migration lives in presets).
pub const STOCK_PROFILE_VERSION: u32 = 1;

/// How the profile's numbers came to be (FILM_MODEL §J; surfaced in UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provenance {
    /// Densitometry on real stock (dataset pointer required).
    Measured,
    /// Fit to a licensed reference LUT/scan — look-matching, *not* stock
    /// measurement; labelled as such everywhere.
    ReferenceFitted,
    /// Authored by an artist against the model's physical bounds — no
    /// measured calibration (the honest default for shipped profiles).
    ArtistAuthored,
    /// Anything goes; UI shows an "experimental" warning.
    Experimental,
}

/// Confidence in the parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// What kind of input image the profile expects (FILM_MODEL §A5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum InputAssumption {
    /// Phone pipeline output (baked local tone mapping) — optional
    /// pre-treatment available.
    PhoneHeavilyProcessed,
    /// Neutral camera JPEG/RAW conversion.
    #[default]
    NeutralCameraJpeg,
    /// No assumption.
    Unknown,
}

/// Stock polarity (FILM_MODEL §D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Polarity {
    Negative,
    Reversal,
}

/// Layer layout: three dye layers or single silver layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayerLayout {
    Color,
    Monochrome,
}

/// 3×3 matrix stored row-major (rows = film layers).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mat3(pub [f64; 9]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([
        1.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, //
        0.0, 0.0, 1.0,
    ]);

    #[inline]
    pub fn row(&self, r: usize) -> [f64; 3] {
        [self.0[3 * r], self.0[3 * r + 1], self.0[3 * r + 2]]
    }

    #[inline]
    pub fn apply(&self, v: [f64; 3]) -> [f64; 3] {
        let r0 = self.row(0);
        let r1 = self.row(1);
        let r2 = self.row(2);
        [
            r0[0] * v[0] + r0[1] * v[1] + r0[2] * v[2],
            r1[0] * v[0] + r1[1] * v[1] + r1[2] * v[2],
            r2[0] * v[0] + r2[1] * v[1] + r2[2] * v[2],
        ]
    }
}

/// One layer's characteristic curve: control points in
/// `(log10 exposure, density)` plus asymptotic bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerCurve {
    pub log_e: Vec<f64>,
    pub density: Vec<f64>,
}

impl LayerCurve {
    fn spline(&self, dir: CurveDirection) -> Result<MonotoneSpline, String> {
        MonotoneSpline::new(self.log_e.clone(), self.density.clone(), dir)
            .map_err(|e| format!("layer curve: {e}"))
    }
}

/// Halation kernel: gaussian-mixture, physically-sized (FILM_MODEL §C2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GaussianComponent {
    /// 1σ radius on the film plane, millimetres (> 0).
    pub sigma_mm: f64,
    /// Mixture weight (≥ 0; normalized internally).
    pub weight: f64,
}

/// Per-channel halation parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HalationChannel {
    /// Gain of the re-exposure (unit-DC-gain kernels ⇒ α is the relative
    /// added exposure for a constant field). Energy guard: α ≤ 0.05.
    pub alpha: f64,
    pub kernel: Vec<GaussianComponent>,
}

/// Grain amplitude as a function of local density (piecewise-linear,
/// endpoints extended flat).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigmaCurve {
    pub density: Vec<f64>,
    pub sigma: Vec<f64>,
}

/// Deterministic grain parameters (FILM_MODEL §G, grain.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrainParams {
    /// Lattice cells per millimetre (> 0).
    pub cells_per_mm: f64,
    /// Base grain-centre intensity λ at the reference density
    /// (centres per cell, > 0).
    pub lambda: f64,
    /// Mean grain radius, millimetres (> 0).
    pub mu_r_mm: f64,
    /// Grain radius standard deviation, millimetres (≥ 0).
    pub sigma_r_mm: f64,
    /// Amplitude curve σ(D): target 48 µm-aperture RMS density
    /// fluctuation vs density (grain.md §4 convention).
    pub sigma_curve: SigmaCurve,
    /// Expected radial power-spectrum slope (log-log, film-plane
    /// frequency, 0.1–0.8 Nyquist band) — the §L PSD gate target.
    pub psd_slope: f64,
    /// Layer correlation matrix (3×3 Pearson ρ, symmetric, PSD,
    /// unit diagonal). Monochrome ignores it.
    pub rho: Mat3,
    /// Default seed (user-adjustable; part of determinism contract).
    pub seed: u32,
}

/// Print/scan inversion parameters (FILM_MODEL §F).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InversionPath {
    /// Paper response curves (default for most stocks).
    Paper,
    /// Log-response + 3×3 + gamma "scanner" model.
    Scanner,
    /// `[ART]` direct film scan — linearize the negative directly, no
    /// second medium (explicit bypass, UI-labelled).
    DirectScan,
}

/// Paper or scanner response per layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintParams {
    /// Which inversion path this stock defaults to.
    pub default_path: InversionPath,
    /// Orange-mask densities subtracted before transmission (per layer).
    pub d_mask: [f64; 3],
    /// Paper characteristic curves (one per layer), in
    /// `(log10 print exposure, reflection density)`.
    pub paper_curves: Vec<LayerCurve>,
    /// Paper white reflection density (linearization anchor).
    pub paper_white_density: f64,
    /// Scanner path: black point, white point (log10 exposure units),
    /// gamma, and the 3×3 scanner matrix.
    pub scanner_black: f64,
    pub scanner_white: f64,
    pub scanner_gamma: f64,
    pub scanner_matrix: Mat3,
    /// Scanner glare: broadband additive lift on print exposure (≥ 0).
    pub scanner_glare: f64,
    /// Print-space → AP1 linear output matrix (profile-declared).
    pub output_matrix: Mat3,
}

/// The complete stock profile (FILM_MODEL §J). Strictly validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockProfile {
    pub schema: String,
    pub id: String,
    pub display_name: String,
    pub profile_version: u32,
    pub provenance: Provenance,
    pub confidence: Confidence,
    pub input_assumption: InputAssumption,
    pub polarity: Polarity,
    pub layers: LayerLayout,
    /// Film-plane crop rule (§0).
    pub crop_rule: crate::frame::CropRule,
    /// Layer excitation matrix (rows = layers, non-negative,
    /// diagonal-dominant).
    pub m_exc: Mat3,
    /// Inter-layer crosstalk (zero diagonal, mild off-diagonal).
    pub crosstalk: Mat3,
    /// Per-layer H&D curves.
    pub curves: Vec<LayerCurve>,
    /// Per-layer fog/base densities (≥ 0), added before crosstalk.
    pub d_min: [f64; 3],
    /// Exposure reference: log10 exposure an 18 % gray input should map
    /// to on the reference layer (metering anchor, §C3).
    pub mid_log_e: f64,
    /// Reciprocity floor clamp on excitation (§D `X_floor`).
    pub x_floor: f64,
    /// Halation (red-biased). Monochrome: single-channel semantics via the
    /// red row.
    pub halation: [HalationChannel; 3],
    /// Halation energy guard ε (§C2): max relative added exposure.
    pub halation_epsilon: f64,
    pub grain: GrainParams,
    pub print: PrintParams,
    /// Physical references (citations, not data).
    pub physical_references: Vec<String>,
    /// Calibration dataset pointer + fit report (required for
    /// measured/reference-fitted provenance).
    pub calibration: Option<CalibrationRef>,
    /// License + provenance text.
    pub license: String,
    pub provenance_text: String,
}

/// Pointer to the calibration evidence behind a measured/fitted profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRef {
    /// Dataset identifier/path (must carry its own rights statement).
    pub dataset: String,
    /// Illuminant the fit used.
    pub illuminant: String,
    /// Scanner/print settings the fit used.
    pub rendering_conditions: String,
    /// Path to the fit report inside this repo.
    pub fit_report: String,
}

/// Validation error — one concrete violation at a time (loader surfaces
/// all by iterating, but each error is self-contained).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProfileError {
    #[error("profile schema field must be {expected:?}, got {got:?}")]
    WrongSchema { expected: String, got: String },
    #[error("profile_version must be {expected}, got {got}")]
    WrongVersion { expected: u32, got: u32 },
    #[error("{field}: {reason}")]
    Range { field: String, reason: String },
    #[error("curve {which}: {reason}")]
    Curve { which: String, reason: String },
    #[error("provenance {0:?} requires a calibration reference")]
    MissingCalibration(Provenance),
}

impl StockProfile {
    /// Strict validation (deny-by-default). Returns `Ok(())` or the first
    /// violation; callers wanting all violations can iterate fields
    /// themselves — the loader reports the first (fail fast, no partial
    /// loads).
    pub fn validate(&self) -> Result<(), ProfileError> {
        let expected_schema = "melography.stock-profile";
        if self.schema != expected_schema {
            return Err(ProfileError::WrongSchema {
                expected: expected_schema.to_string(),
                got: self.schema.clone(),
            });
        }
        if self.profile_version != STOCK_PROFILE_VERSION {
            return Err(ProfileError::WrongVersion {
                expected: STOCK_PROFILE_VERSION,
                got: self.profile_version,
            });
        }
        if self.id.is_empty() || self.display_name.is_empty() {
            return Err(ProfileError::Range {
                field: "id/display_name".into(),
                reason: "must be non-empty".into(),
            });
        }
        // Provenance honesty: measured/reference-fitted must point at data.
        match self.provenance {
            Provenance::Measured | Provenance::ReferenceFitted => {
                if self.calibration.is_none() {
                    return Err(ProfileError::MissingCalibration(self.provenance));
                }
            }
            Provenance::ArtistAuthored | Provenance::Experimental => {
                if self.calibration.is_some() {
                    return Err(ProfileError::Range {
                        field: "calibration".into(),
                        reason: format!(
                            "provenance {:?} must not claim calibration data",
                            self.provenance
                        ),
                    });
                }
            }
        }
        // M_exc: non-negative everywhere; diagonal-dominant for the color
        // layout (spectral-basis sanity). Monochrome collapses to a
        // luminance-like row (FILM_MODEL §D) — dominance does not apply.
        for r in 0..3 {
            let row = self.m_exc.row(r);
            for &v in &row {
                if !(v.is_finite() && v >= 0.0) {
                    return Err(ProfileError::Range {
                        field: format!("m_exc row {r}"),
                        reason: "entries must be finite and non-negative".into(),
                    });
                }
            }
            if self.layers == LayerLayout::Color {
                let off = row[0] + row[1] + row[2] - row[r];
                if row[r] < off {
                    return Err(ProfileError::Range {
                        field: format!("m_exc row {r}"),
                        reason: "must be diagonal-dominant".into(),
                    });
                }
            }
        }
        // Crosstalk: zero diagonal, mild off-diagonal (|C_ij| ≤ 0.3).
        for i in 0..9 {
            let v = self.crosstalk.0[i];
            if !v.is_finite() {
                return Err(ProfileError::Range {
                    field: "crosstalk".into(),
                    reason: "entries must be finite".into(),
                });
            }
            if i % 4 == 0 && v != 0.0 {
                return Err(ProfileError::Range {
                    field: "crosstalk".into(),
                    reason: "diagonal must be zero (off-diagonal dye contamination only)".into(),
                });
            }
            if i % 4 != 0 && v.abs() > 0.3 {
                return Err(ProfileError::Range {
                    field: "crosstalk".into(),
                    reason: "off-diagonal must be mild (|C_ij| ≤ 0.3)".into(),
                });
            }
        }
        // Curves: right count, direction, γ range, D_min.
        let want_curves = match self.layers {
            LayerLayout::Color => 3,
            LayerLayout::Monochrome => 1,
        };
        if self.curves.len() != want_curves {
            return Err(ProfileError::Range {
                field: "curves".into(),
                reason: format!(
                    "expected {want_curves} layer curves, got {}",
                    self.curves.len()
                ),
            });
        }
        let dir = match self.polarity {
            Polarity::Negative => CurveDirection::Increasing,
            Polarity::Reversal => CurveDirection::Decreasing,
        };
        for (li, lc) in self.curves.iter().enumerate() {
            let spline = lc.spline(dir).map_err(|reason| ProfileError::Curve {
                which: format!("layer {li}"),
                reason,
            })?;
            let gamma = spline.max_gradient();
            if !(0.3..=1.6).contains(&gamma) {
                return Err(ProfileError::Range {
                    field: format!("curves[{li}].gamma"),
                    reason: format!(
                        "straight-line gradient must be within [0.3, 1.6], got {gamma:.3}"
                    ),
                });
            }
            // §D numerical edges: densities are never negative — control
            // points (response above fog) must respect that bound too.
            for (pi, &d) in lc.density.iter().enumerate() {
                if d < 0.0 {
                    return Err(ProfileError::Range {
                        field: format!("curves[{li}].density[{pi}]"),
                        reason: "densities must be ≥ 0 (never-negative rule)".into(),
                    });
                }
            }
        }
        for (li, &dmin) in self.d_min.iter().enumerate() {
            if !(dmin.is_finite() && dmin >= 0.0) {
                return Err(ProfileError::Range {
                    field: format!("d_min[{li}]"),
                    reason: "must be finite and ≥ 0".into(),
                });
            }
        }
        if !(self.x_floor.is_finite() && self.x_floor > 0.0) {
            return Err(ProfileError::Range {
                field: "x_floor".into(),
                reason: "must be > 0 (reciprocity floor clamp)".into(),
            });
        }
        // Halation: α ≤ ε, α_r ≫ α_g ≥ α_b, kernels positive.
        for (c, h) in self.halation.iter().enumerate() {
            if !(h.alpha.is_finite() && h.alpha >= 0.0 && h.alpha <= self.halation_epsilon) {
                return Err(ProfileError::Range {
                    field: format!("halation[{c}].alpha"),
                    reason: format!(
                        "must be within [0, ε={}]: energy guard (unit-DC-gain kernels)",
                        self.halation_epsilon
                    ),
                });
            }
            if h.kernel.is_empty() {
                return Err(ProfileError::Range {
                    field: format!("halation[{c}].kernel"),
                    reason: "at least one gaussian component required".into(),
                });
            }
            let mut wsum = 0.0;
            for k in &h.kernel {
                if !(k.sigma_mm.is_finite() && k.sigma_mm > 0.0) {
                    return Err(ProfileError::Range {
                        field: format!("halation[{c}].kernel.sigma_mm"),
                        reason: "must be > 0".into(),
                    });
                }
                if !(k.weight.is_finite() && k.weight >= 0.0) {
                    return Err(ProfileError::Range {
                        field: format!("halation[{c}].kernel.weight"),
                        reason: "must be ≥ 0".into(),
                    });
                }
                wsum += k.weight;
            }
            if wsum <= 0.0 {
                return Err(ProfileError::Range {
                    field: format!("halation[{c}].kernel"),
                    reason: "weights must not all be zero".into(),
                });
            }
        }
        let [hr, hg, hb] = &self.halation;
        if !(hr.alpha >= hg.alpha && hg.alpha >= hb.alpha) {
            return Err(ProfileError::Range {
                field: "halation.alpha ordering".into(),
                reason: "must satisfy α_r ≥ α_g ≥ α_b (red-biased physics)".into(),
            });
        }
        // Grain: positive scales, ρ a proper correlation matrix.
        let g = &self.grain;
        for (field, val, lower) in [
            ("cells_per_mm", g.cells_per_mm, 1.0),
            ("lambda", g.lambda, 0.0),
            ("mu_r_mm", g.mu_r_mm, 0.0),
            ("sigma_r_mm", g.sigma_r_mm, 0.0),
        ] {
            if !(val.is_finite() && val > lower) {
                return Err(ProfileError::Range {
                    field: format!("grain.{field}"),
                    reason: format!("must be finite and > {lower}"),
                });
            }
        }
        if g.sigma_curve.density.len() != g.sigma_curve.sigma.len()
            || g.sigma_curve.density.is_empty()
        {
            return Err(ProfileError::Range {
                field: "grain.sigma_curve".into(),
                reason: "equal-length, non-empty control arrays required".into(),
            });
        }
        for (i, &s) in g.sigma_curve.sigma.iter().enumerate() {
            if !(s.is_finite() && s >= 0.0) {
                return Err(ProfileError::Range {
                    field: format!("grain.sigma_curve.sigma[{i}]"),
                    reason: "must be finite and ≥ 0".into(),
                });
            }
        }
        if !g.psd_slope.is_finite() || g.psd_slope.abs() > 10.0 {
            return Err(ProfileError::Range {
                field: "grain.psd_slope".into(),
                reason: "must be a finite log-log slope within ±10".into(),
            });
        }
        validate_correlation(&g.rho).map_err(|reason| ProfileError::Range {
            field: "grain.rho".into(),
            reason,
        })?;
        // Print/scan.
        let p = &self.print;
        if p.paper_curves.len() != want_curves {
            return Err(ProfileError::Range {
                field: "print.paper_curves".into(),
                reason: format!(
                    "expected {want_curves} paper curves, got {}",
                    p.paper_curves.len()
                ),
            });
        }
        for (pi, pc) in p.paper_curves.iter().enumerate() {
            pc.spline(CurveDirection::Increasing)
                .map_err(|reason| ProfileError::Curve {
                    which: format!("paper {pi}"),
                    reason,
                })?;
            for &d in &pc.density {
                if d < 0.0 {
                    return Err(ProfileError::Range {
                        field: format!("print.paper_curves[{pi}]"),
                        reason: "densities must be ≥ 0 (never-negative rule)".into(),
                    });
                }
            }
        }
        for &m in &p.d_mask {
            if !(m.is_finite() && m >= 0.0) {
                return Err(ProfileError::Range {
                    field: "print.d_mask".into(),
                    reason: "must be finite and ≥ 0".into(),
                });
            }
        }
        if !(p.scanner_black.is_finite()
            && p.scanner_white.is_finite()
            && p.scanner_black < p.scanner_white)
        {
            return Err(ProfileError::Range {
                field: "print.scanner_black/white".into(),
                reason: "black < white required (log10 exposure window)".into(),
            });
        }
        if !(p.scanner_gamma.is_finite() && (0.5..=3.0).contains(&p.scanner_gamma)) {
            return Err(ProfileError::Range {
                field: "print.scanner_gamma".into(),
                reason: "must be within [0.5, 3.0]".into(),
            });
        }
        if !(p.scanner_glare.is_finite() && p.scanner_glare >= 0.0) {
            return Err(ProfileError::Range {
                field: "print.scanner_glare".into(),
                reason: "must be ≥ 0".into(),
            });
        }
        for v in p.output_matrix.0.iter().chain(p.scanner_matrix.0.iter()) {
            if !v.is_finite() {
                return Err(ProfileError::Range {
                    field: "print matrices".into(),
                    reason: "entries must be finite".into(),
                });
            }
        }
        if self.physical_references.is_empty() {
            return Err(ProfileError::Range {
                field: "physical_references".into(),
                reason: "at least one citation required (provenance honesty)".into(),
            });
        }
        if self.license.is_empty() {
            return Err(ProfileError::Range {
                field: "license".into(),
                reason: "license text required".into(),
            });
        }
        Ok(())
    }

    /// Curve direction implied by polarity.
    pub fn curve_direction(&self) -> CurveDirection {
        match self.polarity {
            Polarity::Negative => CurveDirection::Increasing,
            Polarity::Reversal => CurveDirection::Decreasing,
        }
    }

    /// Compiled H&D splines (validation already passed).
    pub fn hd_splines(&self) -> Vec<MonotoneSpline> {
        let dir = self.curve_direction();
        self.curves
            .iter()
            .map(|lc| lc.spline(dir).expect("validate() ran"))
            .collect()
    }

    /// Compiled paper splines.
    pub fn paper_splines(&self) -> Vec<MonotoneSpline> {
        self.print
            .paper_curves
            .iter()
            .map(|lc| {
                lc.spline(CurveDirection::Increasing)
                    .expect("validate() ran")
            })
            .collect()
    }
}

/// ρ matrix must be symmetric, unit-diagonal, positive semi-definite
/// (all eigenvalues ≥ −1e-9; 3×3 analytic check via characteristic
/// polynomial bounds — cheap and allocation-free via leading minors is
/// *not* sufficient for PSD, so use the trace/determinant invariants).
fn validate_correlation(m: &Mat3) -> Result<(), String> {
    for i in 0..3 {
        if (m.0[4 * i] - 1.0).abs() > 1e-9 {
            return Err(format!("rho[{i}][{i}] must be 1.0"));
        }
    }
    for i in 0..3 {
        for j in (i + 1)..3 {
            if (m.0[3 * i + j] - m.0[3 * j + i]).abs() > 1e-9 {
                return Err(format!("rho must be symmetric at ({i},{j})"));
            }
            if m.0[3 * i + j].abs() > 1.0 + 1e-9 {
                return Err(format!("rho[{i}][{j}] must be within [-1, 1]"));
            }
        }
    }
    // PSD check via Sylvester + eigenvalue bound: for a 3×3 correlation
    // matrix, PSD ⟺ all principal minors ≥ 0.
    let (a, b, c) = (m.0[1], m.0[2], m.0[5]); // rho01, rho02, rho12
    let m2 = |i: usize, j: usize| 1.0 - m.0[3 * i + j] * m.0[3 * i + j];
    if m2(0, 1) < -1e-9 || m2(0, 2) < -1e-9 || m2(1, 2) < -1e-9 {
        return Err("2×2 principal minors must be ≥ 0".into());
    }
    let det = 1.0 + 2.0 * a * b * c - a * a - b * b - c * c;
    if det < -1e-9 {
        return Err(format!(
            "determinant must be ≥ 0 (PSD), got {det:.4} — layer correlation is not realizable"
        ));
    }
    Ok(())
}

/// Shared test fixtures (used by unit tests *and* the integration gates
/// in `tests/gates.rs` — integration tests cannot see `#[cfg(test)]`
/// modules). Not part of the public API surface; hidden from docs.
#[doc(hidden)]
pub mod testing {
    use super::*;

    /// A minimal valid color-negative profile used across the crate's
    /// tests. Numbers are artist-authored plausible values, not measured.
    pub fn test_profile() -> StockProfile {
        let curve = |d0: f64, d1: f64, d2: f64, d3: f64, d4: f64, d5: f64| LayerCurve {
            log_e: vec![-4.0, -3.0, -2.0, -1.0, -0.5, 0.0],
            density: vec![d0, d1, d2, d3, d4, d5],
        };
        // Paper is negative-working: more print exposure → higher
        // reflection density (white paper base → black maximum).
        let paper = LayerCurve {
            log_e: vec![-3.5, -2.5, -1.5, -0.8, -0.3],
            density: vec![0.02, 0.05, 0.45, 1.60, 2.10],
        };
        StockProfile {
            schema: "melography.stock-profile".into(),
            id: "test-negative-400".into(),
            display_name: "Test Negative 400".into(),
            profile_version: STOCK_PROFILE_VERSION,
            provenance: Provenance::ArtistAuthored,
            confidence: Confidence::Medium,
            input_assumption: InputAssumption::NeutralCameraJpeg,
            polarity: Polarity::Negative,
            layers: LayerLayout::Color,
            crop_rule: Default::default(),
            m_exc: Mat3([
                1.00, 0.06, 0.02, //
                0.05, 0.95, 0.04, //
                0.03, 0.07, 0.90,
            ]),
            crosstalk: Mat3([
                0.0, 0.06, 0.02, //
                0.04, 0.0, 0.05, //
                0.02, 0.03, 0.0,
            ]),
            curves: vec![
                curve(0.05, 0.08, 0.35, 1.05, 1.40, 1.55),
                curve(0.07, 0.10, 0.37, 1.07, 1.42, 1.57),
                curve(0.10, 0.13, 0.40, 1.10, 1.45, 1.60),
            ],
            d_min: [0.05, 0.07, 0.10],
            mid_log_e: -2.0,
            x_floor: 1e-5,
            halation: [
                HalationChannel {
                    alpha: 0.045,
                    kernel: vec![
                        GaussianComponent { sigma_mm: 0.010, weight: 0.7 },
                        GaussianComponent { sigma_mm: 0.050, weight: 0.3 },
                    ],
                },
                HalationChannel {
                    alpha: 0.010,
                    kernel: vec![GaussianComponent { sigma_mm: 0.008, weight: 1.0 }],
                },
                HalationChannel { alpha: 0.0, kernel: vec![GaussianComponent { sigma_mm: 0.006, weight: 1.0 }] },
            ],
            halation_epsilon: 0.05,
            grain: GrainParams {
                cells_per_mm: 48.0,
                lambda: 1.2,
                mu_r_mm: 0.0055,
                sigma_r_mm: 0.0025,
                sigma_curve: SigmaCurve {
                    density: vec![0.0, 0.6, 1.2, 2.0],
                    sigma: vec![0.010, 0.030, 0.038, 0.020],
                },
                // Measured self-consistency anchor: the field's radial PSD
                // slope in the 0.1-0.8 Nyquist band at 6 µm/px (gate
                // re-measures and enforces ±15%). The fit tool recomputes
                // this for authored profiles.
                psd_slope: -0.869,
                rho: Mat3([
                    1.0, 0.35, 0.20, //
                    0.35, 1.0, 0.30, //
                    0.20, 0.30, 1.0,
                ]),
                seed: 7,
            },
            print: PrintParams {
                default_path: InversionPath::Paper,
                d_mask: [0.05, 0.25, 0.55],
                paper_curves: vec![paper.clone(), paper.clone(), paper.clone()],
                paper_white_density: 0.02,
                scanner_black: -3.0,
                scanner_white: -0.2,
                scanner_gamma: 1.0,
                scanner_matrix: Mat3::IDENTITY,
                scanner_glare: 0.002,
                output_matrix: Mat3::IDENTITY,
            },
            physical_references: vec![
                "Newson et al., stochastic film grain model (IEEE TIP 2017) — model form only, no data".into(),
            ],
            calibration: None,
            license: "CC0-1.0 (test fixture)".into(),
            provenance_text: "Test fixture — artist-authored plausible parameters".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::test_profile;
    use super::*;

    #[test]
    fn test_profile_valid() {
        test_profile().validate().unwrap();
    }

    #[test]
    fn rejects_wrong_schema() {
        let mut p = test_profile();
        p.schema = "something.else".into();
        assert!(matches!(
            p.validate(),
            Err(ProfileError::WrongSchema { .. })
        ));
    }

    #[test]
    fn rejects_measured_without_calibration() {
        let mut p = test_profile();
        p.provenance = Provenance::Measured;
        assert!(matches!(
            p.validate(),
            Err(ProfileError::MissingCalibration(_))
        ));
    }

    #[test]
    fn rejects_artist_with_calibration_claim() {
        let mut p = test_profile();
        p.calibration = Some(CalibrationRef {
            dataset: "x".into(),
            illuminant: "D55".into(),
            rendering_conditions: "test".into(),
            fit_report: "docs/x.md".into(),
        });
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_non_diagonal_dominant_excitation() {
        let mut p = test_profile();
        p.m_exc = Mat3([
            0.2, 0.5, 0.4, //
            0.1, 0.9, 0.1, //
            0.3, 0.2, 0.8,
        ]);
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_gamma_out_of_range() {
        let mut p = test_profile();
        p.curves[0] = LayerCurve {
            log_e: vec![-2.0, -1.0],
            density: vec![0.0, 2.0], // γ = 2.0 > 1.6
        };
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_halation_energy_guard_violation() {
        let mut p = test_profile();
        p.halation[0].alpha = 0.2; // > ε = 0.05
        assert!(p.validate().is_err());
    }

    #[test]
    fn rejects_non_psd_rho() {
        let mut p = test_profile();
        p.grain.rho = Mat3([
            1.0, 0.95, 0.95, //
            0.95, 1.0, -0.95, //
            0.95, -0.95, 1.0,
        ]);
        assert!(p.validate().is_err());
    }

    #[test]
    fn json_roundtrip_stable() {
        let p = test_profile();
        let j = serde_json::to_string_pretty(&p).unwrap();
        let back: StockProfile = serde_json::from_str(&j).unwrap();
        assert_eq!(back, p);
    }
}
