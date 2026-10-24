//! Stage F — negative → positive (print/scan, FILM_MODEL §F).
//!
//! The **second nonlinear reproduction** — what makes output "look like a
//! print" instead of "look like a filter". Negative stocks pass through
//! mask subtraction → transmission → print exposure → paper curves (or the
//! scanner path) → linearization. Reversal stocks skip the inversion
//! (their density already falls with exposure) and project directly.
//!
//! Paths (profile-default, user-overridable):
//! * `Paper` — print exposure + paper characteristic curves (default);
//! * `Scanner` — log-response window + gamma + 3×3 + positive mapping;
//! * `DirectScan` `[ART]` — "direct film scan": linearize the negative
//!   through the mask with no second medium (the spec's explicit bypass,
//!   UI-labelled as artistic).
//!
//! Output: print/slide space linearized to [0, 1] at the white anchor,
//! then the profile's 3×3 output matrix to AP1-linear (unbounded — the
//! matrix may push out of gamut; the export stage owns gamut handling).

use crate::curves::MonotoneSpline;
use crate::optics::Plane;
use crate::profile::{InversionPath, LayerLayout, Polarity, PrintParams, StockProfile};

/// Effective maximum density of the scanner positive mapping.
const SCANNER_D_MAX: f64 = 2.2;

/// Resolved print/scan stage.
pub struct Print {
    pub polarity: Polarity,
    pub layers: LayerLayout,
    pub path: InversionPath,
    pub d_mask: [f64; 3],
    /// Print light `L_p = 2^(print_exposure_stops)`.
    pub l_p: f64,
    pub paper: Vec<MonotoneSpline>,
    pub paper_white_density: f64,
    pub scanner_black: f64,
    pub scanner_white: f64,
    pub scanner_gamma: f64,
    pub scanner_matrix: crate::profile::Mat3,
    pub scanner_glare: f64,
    pub output_matrix: crate::profile::Mat3,
}

impl Print {
    pub fn resolve(
        profile: &StockProfile,
        path_override: Option<InversionPath>,
        print_exposure_stops: f64,
    ) -> Self {
        let p: &PrintParams = &profile.print;
        let path = match (path_override, profile.polarity) {
            (Some(x), _) => x,
            (None, Polarity::Reversal) => InversionPath::Paper, // unused for reversal
            (None, Polarity::Negative) => p.default_path,
        };
        Self {
            polarity: profile.polarity,
            layers: profile.layers,
            path,
            d_mask: p.d_mask,
            l_p: 2f64.powf(print_exposure_stops),
            paper: profile.paper_splines(),
            paper_white_density: p.paper_white_density,
            scanner_black: p.scanner_black,
            scanner_white: p.scanner_white,
            scanner_gamma: p.scanner_gamma,
            scanner_matrix: p.scanner_matrix,
            scanner_glare: p.scanner_glare,
            output_matrix: p.output_matrix,
        }
    }

    /// Per-pixel density → AP1-linear (no margin).
    pub fn apply(&self, src: &Plane, dst: &mut Plane, y0: u32, y1: u32) {
        let layers: &[usize] = match self.layers {
            LayerLayout::Color => &[0, 1, 2],
            LayerLayout::Monochrome => &[0],
        };
        for y in y0..y1 {
            for x in 0..src.w {
                let d = src.px64(x, y);
                let mut lin = [0.0f64; 3];
                match self.polarity {
                    Polarity::Reversal => {
                        // Direct projection: T = 10^−D, normalized at the
                        // white anchor (clear film base).
                        for (li, &c) in layers.iter().enumerate() {
                            let d_eff = d[c] - self.d_mask[li];
                            let t = 10f64.powf(-d_eff);
                            lin[c] = t * 10f64.powf(self.paper_white_density);
                        }
                    }
                    Polarity::Negative => match self.path {
                        InversionPath::Paper => {
                            for (li, &c) in layers.iter().enumerate() {
                                let d_eff = d[c] - self.d_mask[li];
                                let t = 10f64.powf(-d_eff);
                                let x_p = self.l_p * t;
                                let d_p = self.paper[li].eval(x_p.max(1e-12).log10());
                                lin[c] = 10f64.powf(self.paper_white_density - d_p);
                            }
                        }
                        InversionPath::Scanner => {
                            for (li, &c) in layers.iter().enumerate() {
                                let d_eff = d[c] - self.d_mask[li];
                                let t = 10f64.powf(-d_eff);
                                // Scanner glare: broadband additive lift.
                                let x_p = self.l_p * t + self.scanner_glare;
                                let log_x = x_p.max(1e-12).log10();
                                let yv = ((log_x - self.scanner_black)
                                    / (self.scanner_white - self.scanner_black))
                                    .clamp(0.0, 1.0);
                                let z = yv.powf(1.0 / self.scanner_gamma);
                                let s = self.scanner_matrix.apply([z, z, z]);
                                // Positive scan: bright ⇒ low density.
                                lin[c] = 10f64.powf(
                                    self.paper_white_density
                                        - SCANNER_D_MAX * (1.0 - s[li].clamp(0.0, 1.0)),
                                );
                            }
                        }
                        InversionPath::DirectScan => {
                            // [ART] direct film scan: no second medium.
                            for (li, &c) in layers.iter().enumerate() {
                                let d_eff = d[c] - self.d_mask[li];
                                lin[c] = 10f64.powf(self.paper_white_density - d_eff);
                            }
                        }
                    },
                }
                // Print/slide space is reflective/projitive: [0, 1] before
                // the output matrix.
                for v in &mut lin {
                    *v = v.clamp(0.0, 1.0);
                }
                let out = self.output_matrix.apply(lin);
                // Monochrome: replicate layer 0.
                if self.layers == LayerLayout::Monochrome {
                    dst.set3(x, y, [out[0], out[0], out[0]]);
                } else {
                    dst.set3(x, y, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::testing::test_profile;

    fn uniform_density(w: u32, h: u32, d: f64) -> Plane {
        Plane {
            w,
            h,
            data: vec![d as f32; (w * h * 3) as usize],
        }
    }

    #[test]
    fn paper_path_inverts_dark_with_bright() {
        // Film: bright subject → DENSE negative. Print: dense negative
        // blocks light → paper stays thin → print BRIGHT. The two-stage
        // inversion is what makes a positive.
        let p = test_profile();
        let pr = Print::resolve(&p, None, 0.0);
        let mut src = Plane::new(2, 1);
        src.set3(0, 0, [0.3, 0.3, 0.3]); // thin negative (dark subject)
        src.set3(1, 0, [1.8, 1.8, 1.8]); // dense negative (bright subject)
        let mut dst = Plane::new(2, 1);
        pr.apply(&src, &mut dst, 0, 1);
        let dark_subject = dst.px64(0, 0)[0];
        let bright_subject = dst.px64(1, 0)[0];
        assert!(
            bright_subject > dark_subject,
            "bright subject must print brighter: {bright_subject} vs {dark_subject}"
        );
    }

    #[test]
    fn print_exposure_burns_darker() {
        // More print light = burning: the paper sees more exposure and
        // prints darker (paper is negative-working).
        let p = test_profile();
        let pr0 = Print::resolve(&p, None, 0.0);
        let pr1 = Print::resolve(&p, None, 1.0); // +1 stop print light
        let src = uniform_density(4, 4, 1.0);
        let mut d0 = Plane::new(4, 4);
        let mut d1 = Plane::new(4, 4);
        pr0.apply(&src, &mut d0, 0, 4);
        pr1.apply(&src, &mut d1, 0, 4);
        assert!(
            d1.px64(1, 1)[0] < d0.px64(1, 1)[0],
            "more print light must darken (burn): {} vs {}",
            d1.px64(1, 1)[0],
            d0.px64(1, 1)[0]
        );
    }

    #[test]
    fn output_bounded_before_matrix() {
        // Any density (including hostile) → finite [0,1]-anchored output.
        let p = test_profile();
        let pr = Print::resolve(&p, None, 0.0);
        let mut src = Plane::new(6, 1);
        let hostile = [-5.0, 0.0, 0.5, 1.5, 8.0, 1e6];
        for (x, &v) in hostile.iter().enumerate() {
            src.set3(x as u32, 0, [v, v, v]);
        }
        let mut dst = Plane::new(6, 1);
        pr.apply(&src, &mut dst, 0, 1);
        for x in 0..6 {
            let v = dst.px64(x, 0);
            for (c, &v_c) in v.iter().enumerate() {
                assert!(v_c.is_finite(), "x={x} c={c}");
            }
        }
    }

    #[test]
    fn scanner_path_differs_from_paper() {
        let p = test_profile();
        let paper = Print::resolve(&p, Some(InversionPath::Paper), 0.0);
        let scanner = Print::resolve(&p, Some(InversionPath::Scanner), 0.0);
        let src = uniform_density(8, 8, 1.0);
        let mut a = Plane::new(8, 8);
        let mut b = Plane::new(8, 8);
        paper.apply(&src, &mut a, 0, 8);
        scanner.apply(&src, &mut b, 0, 8);
        assert_ne!(a.px64(4, 4), b.px64(4, 4));
    }
}
