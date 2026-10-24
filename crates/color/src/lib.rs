//! # melography-color
//!
//! Color management for MELOGRAPHY (ADR-0002, COLOR_PIPELINE.md):
//!
//! * **Working space:** AP1 (ACEScg) linear, unbounded f32.
//! * **Fast paths:** derived sRGB↔AP1 / P3→AP1 matrices, golden-tested
//!   against lcms2 in CI (COLOR_PIPELINE §8).
//! * **CMM:** lcms2, compiled from the **vendored 2.19 source** on every OS
//!   (`static` feature) so all platforms share one library version — the
//!   foundation of Spike S2's cross-OS determinism gate.
//! * **Oracle:** dependency-free XYZ→Lab + CIEDE2000 for the golden tests.
//!
//! M0 scope: S1/S5 stage math + programmatic profiles + the golden battery.
//! Display-profile discovery (S4 stage), gamut mapping policies and
//! soft-proofing arrive with M1.

pub mod delta_e;
pub mod lcms;
pub mod matrix;
pub mod pipeline;
pub mod spaces;
pub mod srgb_tf;
pub mod tone;

pub use lcms::{
    ap1_profile, icc_bytes_deterministic, lcms2_version, p3_profile, profile_for_encoding,
    srgb_profile, ColorError,
};
pub use pipeline::{
    ap1_pattern_display_reference, ap1_to_srgb_lcms, ap1_to_srgb_u16_fast, ap1_to_srgb_u8_dither,
    default_export_encoding, from_ap1_lcms, p3_linear_to_ap1_fast, srgb_to_ap1_fast, to_ap1_lcms,
};
pub use spaces::{ap1_to_srgb_linear, p3_linear_to_ap1, srgb_linear_to_ap1};

use sha2::{Digest, Sha256};

/// SHA-256 of an image's raw f32 payload — the cross-OS determinism
/// fingerprint used by the golden tests and `melography-cli golden`.
#[must_use]
pub fn hash_image(image: &melography_core::ImageF32) -> String {
    let mut h = Sha256::new();
    h.update(image.width().to_le_bytes());
    h.update(image.height().to_le_bytes());
    h.update([match image.channels() {
        melography_core::Channels::Rgb => 3u8,
        melography_core::Channels::Rgba => 4u8,
    }]);
    h.update(image.as_bytes());
    format!("{:x}", h.finalize())
}
