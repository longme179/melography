//! Colorimetric difference oracle: XYZ→CIELAB conversion (D50 PCS) and
//! CIEDE2000. This module is the *correctness reference* used by the golden
//! tests (COLOR_PIPELINE §8) — it must stay dependency-free and explicit.
//!
//! CIEDE2000 implementation follows Sharma, Wu & Dalal (2005),
//! "The CIEDE2000 Color-Difference Formula: Implementation Notes,
//! Supplementary Test Data, and Mathematical Observations", Color Research
//! & Application 30(1). The published test pairs are used in
//! `tests/ciede2000_vectors.rs` (standard constants, cited).

/// CIE 1931 2° observer, D50 illuminant (ICC PCS white).
pub const D50_XYZ: [f64; 3] = [0.34567 / 0.35850, 1.0, (1.0 - 0.34567 - 0.35850) / 0.35850];

const DELTA: f64 = 6.0 / 29.0;
const DELTA_SQ: f64 = DELTA * DELTA;

#[must_use]
fn lab_f(t: f64) -> f64 {
    if t > DELTA * DELTA * DELTA {
        t.cbrt()
    } else {
        t / (3.0 * DELTA_SQ) + 4.0 / 29.0
    }
}

/// XYZ (Y-normalized, D50) → CIELAB.
#[must_use]
pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let fx = lab_f(xyz[0] / D50_XYZ[0]);
    let fy = lab_f(xyz[1] / D50_XYZ[1]);
    let fz = lab_f(xyz[2] / D50_XYZ[2]);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 color difference between two CIELAB colors.
#[must_use]
pub fn delta_e_2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let [l1, a1, b1] = lab1;
    let [l2, a2, b2] = lab2;

    // Step 1: C', h'
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let c_bar = (c1 + c2) / 2.0;
    let c_bar7 = c_bar.powi(7);
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + 25.0_f64.powi(7))).sqrt());
    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();

    let hp = |a: f64, b: f64| -> f64 {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            let mut h = b.atan2(a).to_degrees();
            if h < 0.0 {
                h += 360.0;
            }
            h
        }
    };
    let h1p = hp(a1p, b1);
    let h2p = hp(a2p, b2);

    // Step 2: ΔL', ΔC', ΔH'
    let dl = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else {
        let mut d = h2p - h1p;
        if d > 180.0 {
            d -= 360.0;
        } else if d < -180.0 {
            d += 360.0;
        }
        d
    };
    // ΔH' is derived from the chord: 2·sqrt(C1'C2')·sin(Δh'/2)
    let dh = 2.0 * (c1p * c2p).sqrt() * (dhp.to_radians() / 2.0).sin();

    // Step 3: averages
    let l_bar = (l1 + l2) / 2.0;
    let cp_bar = (c1p + c2p) / 2.0;
    let h_sum = h1p + h2p;
    // Sharma et al. note: when either C' is zero the average is the SUM, and
    // the |Δh'|>180 branches wrap by ±360 before averaging.
    let h_bar_pre = if c1p * c2p == 0.0 {
        h_sum
    } else if (h1p - h2p).abs() > 180.0 && h_sum < 360.0 {
        h_sum + 360.0
    } else if (h1p - h2p).abs() > 180.0 {
        h_sum - 360.0
    } else {
        h_sum
    };
    let h_bar = h_bar_pre / 2.0;

    let t = 1.0 - 0.17 * ((h_bar - 30.0).to_radians()).cos()
        + 0.24 * ((2.0 * h_bar).to_radians()).cos()
        + 0.32 * ((3.0 * h_bar + 6.0).to_radians()).cos()
        - 0.20 * ((4.0 * h_bar - 63.0).to_radians()).cos();
    let d_theta = 30.0 * (-(((h_bar - 275.0) / 25.0).powi(2))).exp();
    let cp_bar7 = cp_bar.powi(7);
    let rc = 2.0 * (cp_bar7 / (cp_bar7 + 25.0_f64.powi(7))).sqrt();
    let lm50 = (l_bar - 50.0) * (l_bar - 50.0);
    let sl = 1.0 + 0.015 * lm50 / (20.0 + lm50).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let rt = -((2.0 * d_theta).to_radians()).sin() * rc;

    let (dl_term, dc_term, dh_term) = (dl / sl, dcp / sc, dh / sh);
    (dl_term * dl_term + dc_term * dc_term + dh_term * dh_term + rt * dc_term * dh_term).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_colors_have_zero_difference() {
        let lab = [50.0, 2.5, 0.0];
        assert!(delta_e_2000(lab, lab).abs() < 1e-12);
    }

    #[test]
    fn lab_black_white_extremes() {
        assert_eq!(xyz_to_lab([0.0, 0.0, 0.0])[0], 0.0);
        let white = xyz_to_lab(D50_XYZ);
        assert!((white[0] - 100.0).abs() < 1e-9);
        assert!(white[1].abs() < 1e-9 && white[2].abs() < 1e-9);
    }

    #[test]
    fn hue_wraparound_branch() {
        // h1' = ~330°, h2' = ~30° exercises the |Δh|>180 branch
        let l1 = a1b1_to_lab(50.0, 20.0, -12.0);
        let l2 = a1b1_to_lab(50.0, 16.0, 12.0);
        let de = delta_e_2000(l1, l2);
        assert!(de > 0.0 && de < 40.0, "implausible ΔE00 for hue wrap: {de}");
    }

    fn a1b1_to_lab(l: f64, a: f64, b: f64) -> [f64; 3] {
        [l, a, b]
    }
}
