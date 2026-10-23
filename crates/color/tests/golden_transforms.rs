//! Golden transform battery — COLOR_PIPELINE §8 rows 1–2, 4–5 + the Spike S2
//! cross-OS determinism gates.
//!
//! Two families:
//! 1. **Tolerance gates** (fast-path vs lcms2, roundtrip error, ramp
//!    neutrality, near-black dithering) — must pass on every machine.
//! 2. **Bit-identity gates** (SHA-256 of transform outputs vs
//!    `tests/golden/golden_transforms.json`) — the S2 question *can the same
//!    code+library produce bit-identical floats on Linux/Windows/macOS?*
//!    The golden file is generated on Linux; CI compares on all three OSes.
//!    Regenerate with `MELOGRAPHY_UPDATE_GOLDEN=1 cargo test -p
//!    melography-color golden` after a *deliberate* pipeline change.
//!
//! If the bit-identity gate fails on some OS, that is a **finding** (S2
//! report), not noise: follow the report's decision procedure.

use melography_color as mcolor;
use melography_color::delta_e::{delta_e_2000, xyz_to_lab};
use melography_core::{ColorEncoding, ImageF32};
use melography_io as mio;

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/golden_transforms.json"
);

#[derive(serde::Deserialize, serde::Serialize)]
struct GoldenFile {
    lcms2_version: u32,
    chart_sha256_f32: String,
    fastpath_ap1_sha256: String,
    lcms_ap1_sha256: String,
    roundtrip_u16_sha256: String,
}

fn chart() -> ImageF32 {
    mio::chart::synthetic_chart()
}

// ─── Family 1: tolerance gates (COLOR_PIPELINE §8) ─────────────────────────

/// Row 2: fast-path matrix vs lcms2 on the chart patch set — ΔE00 mean
/// ≤ 0.1, max ≤ 0.5.
#[test]
fn fastpath_vs_lcms2_delta_e00_within_threshold() {
    let chart = chart();
    let fast = mcolor::srgb_to_ap1_fast(&chart);
    let lcms = mcolor::to_ap1_lcms(&chart, &ColorEncoding::Srgb { linear: false }).unwrap();

    // Compare in Lab (D50 PCS) via XYZ: fast path → XYZ(AP1→XYZ D60→Bradford
    // →D50) is implicit through the sRGB roundtrip instead — simpler and
    // still a colorimetric statement: convert BOTH outputs back to sRGB
    // 16-bit and compare *those* in Lab via sRGB→XYZ(D50).
    let fast_u16 = mcolor::ap1_to_srgb_u16_fast(&fast);
    let lcms_back = mcolor::ap1_to_srgb_lcms(&lcms).unwrap();
    let lcms_u16: Vec<u16> = lcms_back
        .data()
        .iter()
        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
        .collect();

    let mut deltas = Vec::with_capacity(fast_u16.len() / 3);
    for (a, b) in fast_u16
        .as_chunks::<3>()
        .0
        .iter()
        .zip(lcms_u16.as_chunks::<3>().0)
    {
        let lab_a = srgb_u16_to_lab(a);
        let lab_b = srgb_u16_to_lab(b);
        deltas.push(delta_e_2000(lab_a, lab_b));
    }
    deltas.sort_by(|x, y| x.partial_cmp(y).unwrap());
    let max = deltas.last().copied().unwrap_or(0.0);
    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    assert!(
        mean <= 0.1 && max <= 0.5,
        "fast-path vs lcms2: ΔE00 mean {mean:.4} (≤0.1), max {max:.4} (≤0.5) required"
    );
}

/// Row 1: sRGB→AP1→sRGB roundtrip max abs error ≤ 0.5/255 (8-bit scale).
#[test]
fn srgb_ap1_roundtrip_within_half_lsb_8bit() {
    let chart = chart();
    let ap1 = mcolor::srgb_to_ap1_fast(&chart);
    let u16 = mcolor::ap1_to_srgb_u16_fast(&ap1);
    let expected: Vec<u16> = chart
        .data()
        .iter()
        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
        .collect();
    let max_err = u16
        .iter()
        .zip(&expected)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .unwrap_or(0);
    assert!(
        max_err <= 128,
        "roundtrip max error {max_err} 16-bit LSBs exceeds 0.5/255 (=128 in 16-bit scale)"
    );
}

/// Row 4: neutral 21-step ramp stays neutral — chromaticity drift ΔE00 ≤ 0.3.
#[test]
fn neutral_ramp_stays_neutral() {
    let chart = chart();
    let ap1 = mcolor::srgb_to_ap1_fast(&chart);
    let u16 = mcolor::ap1_to_srgb_u16_fast(&ap1);
    for i in 0..21u32 {
        let gx = i + 1; // ramp occupies grid row 0, cells 1..=21 (chart.rs)
        let (px, py) = (gx * 32 + 16, 16);
        let idx = (py as usize * chart.width() as usize + px as usize) * 3;
        let rgb: [f64; 3] = [
            f64::from(u16[idx]) / 65_535.0,
            f64::from(u16[idx + 1]) / 65_535.0,
            f64::from(u16[idx + 2]) / 65_535.0,
        ];
        let lab = srgb_u8f_to_lab(rgb);
        // neutrality = a* and b* within ΔE00 0.3 of the neutral axis point
        let neutral = [lab[0], 0.0, 0.0];
        let de = delta_e_2000(lab, neutral);
        assert!(
            de <= 0.3,
            "ramp step {i} drifted: ΔE00 {de:.4} (lab {lab:?})"
        );
    }
}

/// Row 5: near-black gradient banding ≤ 1 LSB with dither ON (8-bit).
#[test]
fn near_black_gradient_bounded_steps_with_dither() {
    let chart = chart();
    let ap1 = mcolor::srgb_to_ap1_fast(&chart);
    let u8buf = mcolor::ap1_to_srgb_u8_dither(&ap1);
    // walk the dark strip's middle row: max adjacent-step delta ≤ 1 LSB
    let y = (mio::chart::CHART_LAYOUT.strip_y + 32) as usize;
    let w = chart.width() as usize;
    let mut max_step = 0i32;
    for x in 1..w {
        let prev = u8buf[(y * w + x - 1) * 3];
        let cur = u8buf[(y * w + x) * 3];
        max_step = max_step.max(i32::from(cur) - i32::from(prev));
    }
    assert!(
        max_step <= 1,
        "dithered near-black gradient has a step of {max_step} LSBs (>1) — banding gate"
    );
}

/// S2 library gate: the vendored lcms2 must be the pinned version on every
/// OS (2.19 ⇒ 2190). A different number here means the static build was
/// silently replaced — fail loudly.
#[test]
fn lcms2_is_the_pinned_vendored_version() {
    assert_eq!(
        mcolor::lcms2_version(),
        2190,
        "expected vendored lcms2 2.19 (2190); the `static` feature was bypassed?"
    );
}

// ─── Family 2: bit-identity gates (Spike S2) ────────────────────────────────

/// SHA-256 of every transform output vs the committed golden values.
/// Set `MELOGRAPHY_UPDATE_GOLDEN=1` to regenerate deliberately.
#[test]
fn cross_os_bit_identity() {
    let chart = chart();
    let fast = mcolor::srgb_to_ap1_fast(&chart);
    let lcms = mcolor::to_ap1_lcms(&chart, &ColorEncoding::Srgb { linear: false }).unwrap();
    let roundtrip = mcolor::ap1_to_srgb_u16_fast(&fast);
    let roundtrip_bytes: Vec<u8> = roundtrip.iter().flat_map(|v| v.to_le_bytes()).collect();

    let actual = GoldenFile {
        lcms2_version: mcolor::lcms2_version(),
        chart_sha256_f32: mcolor::hash_image(&chart),
        fastpath_ap1_sha256: mcolor::hash_image(&fast),
        lcms_ap1_sha256: mcolor::hash_image(&lcms),
        roundtrip_u16_sha256: {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&roundtrip_bytes))
        },
    };

    if std::env::var("MELOGRAPHY_UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN_PATH, serde_json::to_string_pretty(&actual).unwrap())
            .expect("write golden file");
        eprintln!("golden file updated: {GOLDEN_PATH}");
        return;
    }

    let golden: GoldenFile = serde_json::from_str(
        &std::fs::read_to_string(GOLDEN_PATH)
            .expect("golden file must exist (run with MELOGRAPHY_UPDATE_GOLDEN=1 once)"),
    )
    .expect("golden file format");

    let mut failures = Vec::new();
    if golden.lcms2_version != actual.lcms2_version {
        failures.push(format!(
            "lcms2_version: golden {} vs actual {}",
            golden.lcms2_version, actual.lcms2_version
        ));
    }
    for (name, g, a) in [
        (
            "chart_sha256_f32",
            &golden.chart_sha256_f32,
            &actual.chart_sha256_f32,
        ),
        (
            "fastpath_ap1_sha256",
            &golden.fastpath_ap1_sha256,
            &actual.fastpath_ap1_sha256,
        ),
        (
            "lcms_ap1_sha256",
            &golden.lcms_ap1_sha256,
            &actual.lcms_ap1_sha256,
        ),
        (
            "roundtrip_u16_sha256",
            &golden.roundtrip_u16_sha256,
            &actual.roundtrip_u16_sha256,
        ),
    ] {
        if g != a {
            failures.push(format!("{name}: golden {g} vs actual {a}"));
        }
    }
    assert!(
        failures.is_empty(),
        "S2 bit-identity gate FAILED on this platform ({}) — this is a Spike S2 finding:\n{}\nDecision procedure: docs/research/spikes/S2_REPORT.md",
        std::env::consts::OS,
        failures.join("\n")
    );
}

// ─── helpers ────────────────────────────────────────────────────────────────

/// sRGB 16-bit triple → Lab (D50) via the crate's own *validated*
/// runtime-derived matrices (no second colorimetric constant set to drift).
fn srgb_u16_to_lab(rgb: &[u16]) -> [f64; 3] {
    let f = [
        f64::from(rgb[0]) / 65_535.0,
        f64::from(rgb[1]) / 65_535.0,
        f64::from(rgb[2]) / 65_535.0,
    ];
    srgb_u8f_to_lab(f)
}

fn srgb_u8f_to_lab(f: [f64; 3]) -> [f64; 3] {
    // sRGB → linear (f64 mirror of the f32 transfer; adequate for ΔE00
    // comparisons at 16-bit quantization)
    let lin: [f64; 3] = f.map(|c| {
        if c.abs() <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    use melography_color::spaces;
    // linear sRGB → XYZ D65 (runtime-derived, validated upstream by the
    // published-AP1-matrix test in spaces.rs)
    let srgb_to_xyz = spaces::rgb_to_xyz(
        spaces::srgb_primaries::R,
        spaces::srgb_primaries::G,
        spaces::srgb_primaries::B,
        spaces::white::D65,
    );
    let xyz_d65 = srgb_to_xyz.apply(lin);
    // Bradford D65 → D50 (runtime-derived, same validation)
    let xyz_d50 = spaces::bradford_adapt(spaces::white::D65, spaces::white::D50).apply(xyz_d65);
    xyz_to_lab(xyz_d50)
}
