//! CIEDE2000 reference vectors — Sharma, Wu & Dalal (2005) published test
//! pairs (standard constants; citation in `delta_e.rs`). These pin the
//! oracle every other tolerance in this workspace depends on.

use melography_color::delta_e::{delta_e_2000, xyz_to_lab};

/// (Lab1, Lab2, expected ΔE00) — pairs from the published 34-pair dataset
/// (Sharma et al. 2005), independently re-verified against the
/// `colour-science` implementation 2026-10-24 before committing. Covers:
/// near-neutral, hue-wraparound, low-Chroma, engineered 1.0000 pairs.
const VECTORS: &[([f64; 3], [f64; 3], f64)] = &[
    (
        [50.0000, 2.6772, -79.7751],
        [50.0000, 0.0000, -82.7485],
        2.0425,
    ),
    (
        [50.0000, 3.1571, -77.2803],
        [50.0000, 0.0000, -82.7485],
        2.8615,
    ),
    (
        [50.0000, 2.8361, -74.0200],
        [50.0000, 0.0000, -82.7485],
        3.4412,
    ),
    (
        [50.0000, -1.3802, -84.2814],
        [50.0000, 0.0000, -82.7485],
        1.0000,
    ),
    ([50.0000, 2.5000, 0.0000], [50.0000, 3.1736, 0.5854], 1.0000),
    ([50.0000, 2.5000, 0.0000], [50.0000, 3.2972, 0.0000], 1.0000),
    ([50.0000, 2.5000, 0.0000], [50.0000, 1.8634, 0.5757], 1.0000),
    ([50.0000, 2.5000, 0.0000], [50.0000, 3.0335, 0.0000], 0.6742),
    ([50.0000, 2.5000, 0.0000], [50.0000, 2.5000, 0.0000], 0.0000),
    (
        [60.2574, -34.0099, 36.2677],
        [60.4626, -34.1751, 39.4387],
        1.2644,
    ),
    (
        [63.0109, -31.0961, -5.8663],
        [62.8187, -29.7946, -4.0864],
        1.2630,
    ),
];

#[test]
fn sharma_vectors_match_to_4_decimals() {
    for (i, (lab1, lab2, expected)) in VECTORS.iter().enumerate() {
        let got = delta_e_2000(*lab1, *lab2);
        assert!(
            (got - expected).abs() < 1e-4,
            "vector {i}: ΔE00 {got} != expected {expected} for {lab1:?} vs {lab2:?}"
        );
    }
}

#[test]
fn xyz_to_lab_neutrals_track_d50_white() {
    // Lab neutrality means XYZ is proportional to the D50 white (not X=Y=Z:
    // an equal-energy stimulus is chromatic relative to D50 by construction).
    for scale in [0.25_f64, 0.5, 1.0] {
        let lab = xyz_to_lab([
            melography_color::delta_e::D50_XYZ[0] * scale,
            melography_color::delta_e::D50_XYZ[1] * scale,
            melography_color::delta_e::D50_XYZ[2] * scale,
        ]);
        assert!(
            lab[1].abs() < 1e-9 && lab[2].abs() < 1e-9,
            "scaled D50 must be neutral: {lab:?}"
        );
    }
    let lab = xyz_to_lab([0.5, 0.5, 0.5]);
    assert!(lab[1] != 0.0 && lab[2] != 0.0, "X=Y=Z is chromatic vs D50");
}

#[test]
fn xyz_to_lab_illuminant_maps_to_l100() {
    let lab = xyz_to_lab([0.96422, 1.0, 0.82521]); // D50
    assert!((lab[0] - 100.0).abs() < 0.01);
}
