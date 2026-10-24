//! Monotone C¹ cubic-Hermite splines for H&D and paper characteristic
//! curves (FILM_MODEL §D, §F).
//!
//! Design contract (schema-enforced upstream, re-validated here):
//! * control points `{ (x_i, y_i) }` are strictly increasing in `x`;
//! * `y` is monotone: non-decreasing for negative stocks, non-increasing
//!   for reversal (the caller passes the expected direction);
//! * tangents follow Fritsch–Carlson so the interpolant is monotone
//!   between points (C¹, no overshoot);
//! * evaluation **extrapolates flat** — outside `[x_0, x_n]` the value is
//!   clamped to the endpoint `y` (FILM_MODEL §D "never negative densities,
//!   never NaN"; toe/shoulder saturate);
//! * all math is f64 (`+ − × ÷` only — no transcendentals), so CPU results
//!   are bit-identical across platforms (S2 discipline).

use serde::{Deserialize, Serialize};

/// Direction of a characteristic curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CurveDirection {
    /// Negative stock: density non-decreasing with log-exposure.
    Increasing,
    /// Reversal stock: density non-increasing with log-exposure.
    Decreasing,
}

/// A validated monotone C¹ cubic-Hermite spline.
#[derive(Debug, Clone, PartialEq)]
pub struct MonotoneSpline {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Fritsch–Carlson tangents.
    ms: Vec<f64>,
    direction: CurveDirection,
}

/// Errors from curve construction/validation.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CurveError {
    #[error("curve needs at least 2 control points, got {0}")]
    TooFewPoints(usize),
    #[error("control point {0}: x values must be strictly increasing")]
    XNotStrictlyIncreasing(usize),
    #[error("control point {0}: y must be finite")]
    YNotFinite(usize),
    #[error("control point {0}: y must be monotone {1} for this curve direction")]
    NotMonotone(usize, &'static str),
}

impl MonotoneSpline {
    /// Validate and build from control points.
    ///
    /// `x` is `log10(exposure)` (or the paper equivalent); `y` is density.
    pub fn new(xs: Vec<f64>, ys: Vec<f64>, direction: CurveDirection) -> Result<Self, CurveError> {
        if xs.len() != ys.len() {
            return Err(CurveError::TooFewPoints(usize::min(xs.len(), ys.len())));
        }
        if xs.len() < 2 {
            return Err(CurveError::TooFewPoints(xs.len()));
        }
        for (i, (&x, &y)) in xs.iter().zip(&ys).enumerate() {
            if !x.is_finite() || !y.is_finite() {
                return Err(CurveError::YNotFinite(i));
            }
            // (x is finite here — the check above ran — so `<=` cannot
            // mis-handle NaN.)
            if i > 0 && x <= xs[i - 1] {
                return Err(CurveError::XNotStrictlyIncreasing(i));
            }
            if i > 0 {
                let ok = match direction {
                    CurveDirection::Increasing => y >= ys[i - 1],
                    CurveDirection::Decreasing => y <= ys[i - 1],
                };
                if !ok {
                    return Err(CurveError::NotMonotone(
                        i,
                        match direction {
                            CurveDirection::Increasing => "non-decreasing",
                            CurveDirection::Decreasing => "non-increasing",
                        },
                    ));
                }
            }
        }
        let ms = fritsch_carlson(&xs, &ys);
        Ok(Self {
            xs,
            ys,
            ms,
            direction,
        })
    }

    /// Evaluate at `x` (flat extrapolation outside the control range).
    pub fn eval(&self, x: f64) -> f64 {
        if !x.is_finite() {
            // NaN/∞ input: clamp to the direction-appropriate saturation end
            // — a curve never produces NaN (FILM_MODEL §D numerical edges).
            return match self.direction {
                CurveDirection::Increasing => {
                    if x > 0.0 {
                        *self.ys.last().unwrap()
                    } else {
                        self.ys[0]
                    }
                }
                CurveDirection::Decreasing => {
                    if x > 0.0 {
                        *self.ys.last().unwrap()
                    } else {
                        self.ys[0]
                    }
                }
            };
        }
        if x <= self.xs[0] {
            return self.ys[0];
        }
        let last = self.xs.len() - 1;
        if x >= self.xs[last] {
            return self.ys[last];
        }
        // Binary search for the segment (deterministic, no data-dependent
        // float reordering).
        let (mut lo, mut hi) = (0usize, last);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.xs[mid] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (x0, x1) = (self.xs[lo], self.xs[lo + 1]);
        let (y0, y1) = (self.ys[lo], self.ys[lo + 1]);
        let (m0, m1) = (self.ms[lo], self.ms[lo + 1]);
        let h = x1 - x0;
        let t = (x - x0) / h;
        // Cubic Hermite basis (only + − × ÷).
        let t2 = t * t;
        let t3 = t2 * t;
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * y0 + h10 * h * m0 + h01 * y1 + h11 * h * m1
    }

    /// Control-point `x` range (for validation reporting).
    pub fn x_range(&self) -> (f64, f64) {
        (self.xs[0], *self.xs.last().unwrap())
    }

    /// Maximum absolute segment gradient (Δy/Δx over control intervals) —
    /// the straight-line gradient `γ` proxy used by the §E range guards.
    pub fn max_gradient(&self) -> f64 {
        self.xs
            .windows(2)
            .zip(self.ys.windows(2))
            .map(|(xs, ys)| {
                let (x0, x1) = (xs[0], xs[1]);
                let (y0, y1) = (ys[0], ys[1]);
                ((y1 - y0) / (x1 - x0)).abs()
            })
            .fold(0.0, f64::max)
    }

    /// Number of control points.
    pub fn len(&self) -> usize {
        self.xs.len()
    }

    /// True when the curve has no control points (never — constructor
    /// enforces ≥ 2; exists for clippy).
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The direction this curve was validated for.
    pub fn direction(&self) -> CurveDirection {
        self.direction
    }

    /// Raw control data for mirrors (GPU packing, fit tooling):
    /// `(x, y, tangent)` triplets, all the same length.
    pub fn control_data(&self) -> (&[f64], &[f64], &[f64]) {
        (&self.xs, &self.ys, &self.ms)
    }
}

/// Fritsch–Carlson monotone tangents.
///
/// One-sided slopes where neighbouring segment gradients disagree in sign,
/// harmonic-mean-limited where they agree — the classic recipe guaranteeing
/// a monotone C¹ interpolant through monotone data.
fn fritsch_carlson(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut d = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        d.push((ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]));
    }
    let mut m = vec![0.0; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        if d[i - 1] * d[i] <= 0.0 {
            m[i] = 0.0;
        } else {
            let w1 = 2.0 * (xs[i + 1] - xs[i]) + (xs[i] - xs[i - 1]);
            let w2 = (xs[i + 1] - xs[i]) + 2.0 * (xs[i] - xs[i - 1]);
            m[i] = (w1 + w2) / (w1 / d[i - 1] + w2 / d[i]);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classic_negative() -> MonotoneSpline {
        // A plausible H&D negative curve: logE from −4 to 0, densities
        // 0.05 → 1.6 with soft toe and shoulder.
        MonotoneSpline::new(
            vec![-4.0, -3.0, -2.0, -1.0, -0.5, 0.0],
            vec![0.05, 0.08, 0.35, 1.05, 1.4, 1.55],
            CurveDirection::Increasing,
        )
        .unwrap()
    }

    #[test]
    fn rejects_non_monotone() {
        let err = MonotoneSpline::new(
            vec![0.0, 1.0, 2.0],
            vec![0.1, 0.5, 0.2],
            CurveDirection::Increasing,
        )
        .unwrap_err();
        assert!(matches!(err, CurveError::NotMonotone(2, "non-decreasing")));
    }

    #[test]
    fn rejects_non_increasing_x() {
        let err = MonotoneSpline::new(
            vec![0.0, 0.0, 1.0],
            vec![0.1, 0.2, 0.3],
            CurveDirection::Increasing,
        )
        .unwrap_err();
        assert!(matches!(err, CurveError::XNotStrictlyIncreasing(1)));
    }

    #[test]
    fn passes_through_control_points() {
        let c = classic_negative();
        let xs = [-4.0, -3.0, -2.0, -1.0, -0.5, 0.0];
        let ys = [0.05, 0.08, 0.35, 1.05, 1.4, 1.55];
        for (&x, &y) in xs.iter().zip(&ys) {
            assert!((c.eval(x) - y).abs() < 1e-12, "at x={x}");
        }
    }

    #[test]
    fn flat_extrapolation_both_ends() {
        let c = classic_negative();
        assert_eq!(c.eval(-10.0), 0.05);
        assert_eq!(c.eval(5.0), 1.55);
    }

    #[test]
    fn never_nan_on_hostile_inputs() {
        let c = classic_negative();
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -4.0, 0.0] {
            assert!(c.eval(x).is_finite(), "x={x}");
        }
    }

    #[test]
    fn monotone_between_points_dense() {
        // Property: dense sampling is non-decreasing (increasing curve).
        let c = classic_negative();
        let mut prev = f64::NEG_INFINITY;
        let mut x = -4.5;
        while x <= 0.5 {
            let v = c.eval(x);
            assert!(v >= prev - 1e-12, "not monotone at x={x}: {v} < {prev}");
            prev = v;
            x += 0.001;
        }
    }

    #[test]
    fn monotone_decreasing_supported() {
        let c = MonotoneSpline::new(
            vec![-3.0, -2.0, -1.0, 0.0],
            vec![2.8, 2.6, 1.2, 0.15],
            CurveDirection::Decreasing,
        )
        .unwrap();
        let mut prev = f64::INFINITY;
        let mut x = -3.5;
        while x <= 0.5 {
            let v = c.eval(x);
            assert!(v <= prev + 1e-12, "not monotone at x={x}");
            prev = v;
            x += 0.001;
        }
    }

    #[test]
    fn max_gradient_is_straight_line_slope() {
        let c = classic_negative();
        // Steepest control segment: (−2,0.35) → (−1,1.05): γ = 0.7.
        assert!((c.max_gradient() - 0.7).abs() < 1e-12);
    }
}
