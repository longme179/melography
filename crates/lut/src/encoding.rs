//! LUT color-encoding contracts (luts.md §3, issue #7).
//!
//! A LUT is authored against an *assumed* input/output color encoding.
//! Applying a "Rec.709 log" LUT to AP1-linear data produces silent garbage —
//! the single most common color bug in LUT workflows. MELOGRAPHY therefore
//! carries the declared encodings through every layer and **wraps** the LUT:
//! working space (AP1) → declared input encoding → LUT → declared output
//! encoding → AP1.
//!
//! **Fixed vocabulary** (luts.md §3; serde names are the sidecar-stable
//! strings used in recipes and the import dialog — extend only additively):
//! `srgb-display`, `rec709-display-linear`, `rec709-gamma2.4`,
//! `display-p3-display`, `acescct`, `acescc`, `agx-log`, `custom-1d-shaper`.
//!
//! **M1 implemented set:** everything except `agx-log` and
//! `custom-1d-shaper` — those two parse, serialize and round-trip through
//! the whole stack, but applying a LUT that declares them returns
//! [`LutError::EncodingNotImplemented`] (the import UI shows them disabled
//! with "coming soon" rather than guessing a meaning). Never silent.
//!
//! **ACEScc/ACEScct convention (documented, deliberate):** both transfers
//! are applied to **AP1 working values** — the `lin_AP1` convention of the
//! reference implementations (colour-science; OCIO ACES configs use ACEScg
//! working space the same way). LUTs authored strictly against AP0/ACES2065-1 differ only by the AP0↔AP1 primaries change; if real-world LUTs require
//! it, an `acescc-ap0` variant can be added additively later.

use crate::LutError;
use melography_color::{matrix::Mat3, spaces};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The fixed declared-encoding vocabulary (luts.md §3). serde names are
/// kebab-case exactly as written in the spec doc — they appear in sidecars
/// and must stay stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LutEncoding {
    /// sRGB primaries (D65) + sRGB piecewise transfer, code values 0..1.
    #[serde(rename = "srgb-display")]
    SrgbDisplay,
    /// Rec.709 primaries (identical to sRGB primaries), linear.
    #[serde(rename = "rec709-display-linear")]
    Rec709DisplayLinear,
    /// Rec.709 primaries, pure 2.4 power gamma (no linear toe).
    #[serde(rename = "rec709-gamma2.4")]
    Rec709Gamma2Point4,
    /// Display-P3 primaries (D65) + sRGB piecewise transfer.
    #[serde(rename = "display-p3-display")]
    DisplayP3Display,
    /// ACEScct log transfer on AP1 values (primaries unchanged).
    #[serde(rename = "acescct")]
    Acescct,
    /// ACEScc log transfer on AP1 values (primaries unchanged).
    #[serde(rename = "acescc")]
    Acescc,
    /// AgX log — fixed vocabulary entry, **not implemented in M1**.
    #[serde(rename = "agx-log")]
    AgxLog,
    /// Custom 1D shaper LUT — fixed vocabulary entry, **not implemented in M1**.
    #[serde(rename = "custom-1d-shaper")]
    Custom1dShaper,
}

impl LutEncoding {
    /// Stable string form (== serde name) for UI/library records.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SrgbDisplay => "srgb-display",
            Self::Rec709DisplayLinear => "rec709-display-linear",
            Self::Rec709Gamma2Point4 => "rec709-gamma2.4",
            Self::DisplayP3Display => "display-p3-display",
            Self::Acescct => "acescct",
            Self::Acescc => "acescc",
            Self::AgxLog => "agx-log",
            Self::Custom1dShaper => "custom-1d-shaper",
        }
    }

    /// Parse from the stable string (serde-equivalent, for non-serde callers).
    pub fn from_str_fixed(s: &str) -> Result<Self, LutError> {
        serde_json::from_value(serde_json::Value::String(s.to_string()))
            .map_err(|_| LutError::EncodingUnknown(s.to_string()))
    }

    /// Human-readable label for UI fallbacks (catalogs own the real strings).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::SrgbDisplay => "sRGB (display)",
            Self::Rec709DisplayLinear => "Rec.709 (linear)",
            Self::Rec709Gamma2Point4 => "Rec.709 (gamma 2.4)",
            Self::DisplayP3Display => "Display P3 (display)",
            Self::Acescct => "ACEScct",
            Self::Acescc => "ACEScc",
            Self::AgxLog => "AgX log (not yet supported)",
            Self::Custom1dShaper => "Custom 1D shaper (not yet supported)",
        }
    }

    /// Whether this build can actually *apply* the encoding (vocabulary
    /// entries may exist before their math ships — see module docs).
    #[must_use]
    pub fn implemented(self) -> bool {
        !matches!(self, Self::AgxLog | Self::Custom1dShaper)
    }
}

// ─── Transfer functions (sign-symmetric where the working data is unbounded) ──

/// Pure 2.4 power function, sign-symmetric (working values are unbounded;
/// negatives mirror through the origin to stay invertible).
fn powf64(base: f64, e: f64) -> f64 {
    base.powf(e)
}

fn gamma2p4_encode(x: f32) -> f32 {
    x.signum() * powf64(x.abs() as f64, 1.0 / 2.4) as f32
}
fn gamma2p4_decode(x: f32) -> f32 {
    x.signum() * powf64(x.abs() as f64, 2.4) as f32
}

// ACEScct constants (S-2016-001; reference implementations agree).
const CCT_X_BRK: f32 = 0.007_812_5;
const CCT_Y_BRK: f32 = 0.155_251_1;
const CCT_A: f32 = 10.540_237;
const CCT_B: f32 = 0.072_905_53;

fn log2_f32(x: f32) -> f32 {
    (x as f64).log2() as f32
}
fn pow2_f32(x: f32) -> f32 {
    powf64(2.0, x as f64) as f32
}

/// ACEScct encode (S-2016-001): linear segment for x ≤ X_BRK, log2 segment
/// above; the linear segment extends smoothly to negative inputs.
fn acescct_encode(x: f32) -> f32 {
    if x <= CCT_X_BRK {
        x * CCT_A + CCT_B
    } else {
        (log2_f32(x) + 9.72) / 17.52
    }
}
fn acescct_decode(y: f32) -> f32 {
    if y > CCT_Y_BRK {
        pow2_f32(y * 17.52 - 9.72)
    } else {
        (y - CCT_B) / CCT_A
    }
}

// ACEScc constants (S-2014-005).
const CC_TWO_POW_NEG15: f32 = 3.051_757_8e-5; // 2^-15
const CC_TWO_POW_NEG16: f32 = 1.525_878_9e-5; // 2^-16
const CC_F16_MAX: f32 = 65504.0;

fn acescc_encode(x: f32) -> f32 {
    if x < 0.0 {
        (log2_f32(CC_TWO_POW_NEG16) + 9.72) / 17.52
    } else if x < CC_TWO_POW_NEG15 {
        (log2_f32(CC_TWO_POW_NEG16 + x * 0.5) + 9.72) / 17.52
    } else {
        (log2_f32(x) + 9.72) / 17.52
    }
}
fn acescc_decode(y: f32) -> f32 {
    if y < (9.72 - 15.0) / 17.52 {
        (pow2_f32(y * 17.52 - 9.72) - CC_TWO_POW_NEG16) * 2.0
    } else if y >= (log2_f32(CC_F16_MAX) + 9.72) / 17.52 {
        CC_F16_MAX
    } else {
        pow2_f32(y * 17.52 - 9.72)
    }
}

// ─── Primaries (AP1 ↔ encoding primaries; Rec.709 == sRGB primaries) ───────

fn ap1_to_p3_linear() -> Mat3 {
    static M: OnceLock<Mat3> = OnceLock::new();
    *M.get_or_init(|| {
        spaces::p3_linear_to_ap1()
            .inverse()
            .expect("P3↔AP1 is invertible by construction")
    })
}

/// Encode one RGB triple from AP1-linear working space into the declared
/// encoding (primaries change + transfer). Sign-symmetric transfers keep
/// out-of-gamut negatives representable (CP §1 hard rule: no premature
/// clamping — clamping to the LUT domain happens in the sampler, and is
/// *reported*).
pub fn wrap_to(v_ap1: [f32; 3], enc: LutEncoding) -> Result<[f32; 3], LutError> {
    if !enc.implemented() {
        return Err(LutError::EncodingNotImplemented(enc.as_str()));
    }
    let v = match enc {
        LutEncoding::SrgbDisplay | LutEncoding::Rec709Gamma2Point4 => {
            spaces::ap1_to_srgb_linear().apply_f32(v_ap1)
        }
        LutEncoding::Rec709DisplayLinear => spaces::ap1_to_srgb_linear().apply_f32(v_ap1),
        LutEncoding::DisplayP3Display => ap1_to_p3_linear().apply_f32(v_ap1),
        // ACES log encodings stay on AP1 primaries by convention (module doc).
        LutEncoding::Acescct | LutEncoding::Acescc => v_ap1,
        LutEncoding::AgxLog | LutEncoding::Custom1dShaper => unreachable!("guarded above"),
    };
    let tf: fn(f32) -> f32 = match enc {
        LutEncoding::SrgbDisplay => melography_color::srgb_tf::linear_to_srgb,
        LutEncoding::Rec709Gamma2Point4 => gamma2p4_encode,
        LutEncoding::Rec709DisplayLinear => |x| x,
        LutEncoding::DisplayP3Display => melography_color::srgb_tf::linear_to_srgb,
        LutEncoding::Acescct => acescct_encode,
        LutEncoding::Acescc => acescc_encode,
        LutEncoding::AgxLog | LutEncoding::Custom1dShaper => unreachable!("guarded above"),
    };
    Ok([tf(v[0]), tf(v[1]), tf(v[2])])
}

/// Decode one RGB triple from the declared encoding back to AP1-linear.
pub fn unwrap_from(v_enc: [f32; 3], enc: LutEncoding) -> Result<[f32; 3], LutError> {
    if !enc.implemented() {
        return Err(LutError::EncodingNotImplemented(enc.as_str()));
    }
    let itf: fn(f32) -> f32 = match enc {
        LutEncoding::SrgbDisplay => melography_color::srgb_tf::srgb_to_linear,
        LutEncoding::Rec709Gamma2Point4 => gamma2p4_decode,
        LutEncoding::Rec709DisplayLinear => |x| x,
        LutEncoding::DisplayP3Display => melography_color::srgb_tf::srgb_to_linear,
        LutEncoding::Acescct => acescct_decode,
        LutEncoding::Acescc => acescc_decode,
        LutEncoding::AgxLog | LutEncoding::Custom1dShaper => unreachable!("guarded above"),
    };
    let v = [itf(v_enc[0]), itf(v_enc[1]), itf(v_enc[2])];
    let out = match enc {
        LutEncoding::SrgbDisplay
        | LutEncoding::Rec709Gamma2Point4
        | LutEncoding::Rec709DisplayLinear => spaces::srgb_linear_to_ap1().apply_f32(v),
        LutEncoding::DisplayP3Display => spaces::p3_linear_to_ap1().apply_f32(v),
        LutEncoding::Acescct | LutEncoding::Acescc => v,
        LutEncoding::AgxLog | LutEncoding::Custom1dShaper => unreachable!("guarded above"),
    };
    Ok(out)
}

/// Round-trip check used by tests and the import validator: encode→decode
/// must reproduce the input within 2e-4 (transfer precision at f32; AP1
/// matrices round-trip within 1e-5 — S2 golden battery covers the matrices
/// themselves at tighter tolerance).
pub fn roundtrip_error(v_ap1: [f32; 3], enc: LutEncoding) -> Result<f32, LutError> {
    let enc_v = wrap_to(v_ap1, enc)?;
    let back = unwrap_from(enc_v, enc)?;
    let d = [
        (back[0] - v_ap1[0]).abs(),
        (back[1] - v_ap1[1]).abs(),
        (back[2] - v_ap1[2]).abs(),
    ];
    Ok(d[0].max(d[1]).max(d[2]))
}
