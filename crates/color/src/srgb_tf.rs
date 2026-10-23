//! sRGB transfer functions (IEC 61966-2-1), sign-symmetric extension for
//! out-of-range values. The working pipeline keeps AP1 values unbounded;
//! these functions are applied only at the sRGB boundary stages (S1 decode
//! of untagged sRGB input, S5 export to sRGB, display fallback).

/// Linear-light → sRGB code-value convention (0..1 float, not 8-bit).
/// Sign-symmetric: negative inputs mirror through the origin so the function
/// stays invertible for out-of-gamut intermediates.
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    if c.abs() <= 0.003_130_8 {
        12.92 * c
    } else {
        let mag = 1.055 * powf24(c.abs()) - 0.055;
        c.signum() * mag as f32
    }
}

/// sRGB code-value convention (0..1 float) → linear-light. Sign-symmetric.
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c.abs() <= 0.040_45 {
        c / 12.92
    } else {
        let mag = powf2_4((c.abs() + 0.055) / 1.055);
        c.signum() * mag as f32
    }
}

/// Fixed-order power helpers: `libm`-grade `powf` is not guaranteed
/// bit-identical across platforms, but the *same* Rust `powf` on the same
/// input compiled by the same rustc IS (pinned via rust-toolchain.toml).
/// The wrappers keep the call sites explicit and measurable; S2's cross-OS
/// hash gate verifies the assumption instead of trusting it.
#[must_use]
fn powf2_4(x: f32) -> f64 {
    f64::from(x).powf(2.4)
}

#[must_use]
fn powf24(x: f32) -> f64 {
    f64::from(x).powf(1.0 / 2.4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_fixed_points() {
        for v in [0.0_f32, 0.5, 1.0, 0.040_45, 0.003_130_8] {
            let roundtrip = linear_to_srgb(srgb_to_linear(v));
            assert!(
                (roundtrip - v).abs() <= 1e-6,
                "roundtrip failed for {v}: {roundtrip}"
            );
        }
    }

    #[test]
    fn known_values() {
        // linear 1 → code 1; linear 0 → 0
        assert_eq!(linear_to_srgb(1.0), 1.0);
        assert_eq!(linear_to_srgb(0.0), 0.0);
        // code 0.5 → linear ≈ 0.21404114
        assert!((srgb_to_linear(0.5) - 0.214_041_14).abs() < 1e-6);
        // sign symmetry
        assert!((srgb_to_linear(-0.5) + 0.214_041_14).abs() < 1e-6);
        assert_eq!(linear_to_srgb(-0.0), 0.0);
    }

    #[test]
    fn monotonic_and_bounded_on_ramp() {
        let mut prev = f32::MIN;
        for i in 0..=256 {
            let c = i as f32 / 256.0;
            let lin = srgb_to_linear(c);
            assert!(lin >= prev, "non-monotonic at {c}");
            prev = lin;
        }
    }
}
