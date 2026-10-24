//! # melography-lut
//!
//! LUT parsing, validation, tetrahedral interpolation and the content-
//! addressed library. **M0 ships the type skeleton only** — the `.cube`
//! parser + tetrahedral CPU engine + independent oracle arrive at M1
//! (issue #7, research `docs/research/luts.md`).
//!
//! The frozen contract surface (COLOR_PIPELINE §1 hard rule): every LUT
//! stage *wraps its declared encoding* at its insertion point —
//! `LutEncoding` exists from day one so no later design can "forget" it.

use melography_core::{ColorEncoding, DeterminismClass, Stage, StageId, StageSpec};
use serde::{Deserialize, Serialize};

/// Pixel encoding a LUT expects at its input / produces at its output.
/// Mismatched insertion = explicit error, never silent reinterpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LutEncoding {
    /// LUT domain/range are sRGB code values (0..1 encoded convention).
    SrgbEncoded,
    /// LUT operates on linear display-referred values.
    LinearDisplayReferred,
    /// LUT operates in the AP1 working space (linear).
    Ap1Linear,
}

/// Declared identity of a LUT file (full validation at M1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutSpec {
    /// Content-addressed id (sha256 of the parsed data; library key).
    pub content_hash: String,
    pub size: u32,
    pub input_encoding: LutEncoding,
    pub output_encoding: LutEncoding,
    /// 3D LUT (size³×3) or 1D shaper chain; M0 models 3D only.
    pub kind: LutKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LutKind {
    Cube3d,
}

#[derive(Debug, thiserror::Error)]
#[error("LUT error: {0}")]
pub enum LutError {
    #[error("unsupported LUT format or version: {0}")]
    Unsupported(String),
    #[error("malformed LUT: {0}")]
    Malformed(String),
    #[error(
        "encoding contract violated: LUT expects {expected:?}, pipeline provides {provided:?}"
    )]
    EncodingMismatch {
        expected: LutEncoding,
        provided: LutEncoding,
    },
}

/// Stage-contract view of a LUT insertion (instantiated per loaded LUT at
/// M1; the spec wiring is fixed now).
pub struct LutStage {
    pub spec: LutSpec,
}

impl Stage for LutStage {
    fn id(&self) -> StageId {
        StageId::new(format!("lut.3d.{}", self.spec.content_hash))
    }
    fn spec(&self) -> StageSpec {
        StageSpec {
            input: match self.spec.input_encoding {
                LutEncoding::SrgbEncoded => ColorEncoding::Srgb { linear: false },
                LutEncoding::LinearDisplayReferred => ColorEncoding::Srgb { linear: true },
                LutEncoding::Ap1Linear => ColorEncoding::Ap1Linear,
            },
            output: match self.spec.output_encoding {
                LutEncoding::SrgbEncoded => ColorEncoding::Srgb { linear: false },
                LutEncoding::LinearDisplayReferred => ColorEncoding::Srgb { linear: true },
                LutEncoding::Ap1Linear => ColorEncoding::Ap1Linear,
            },
        }
    }
    fn determinism(&self) -> DeterminismClass {
        DeterminismClass::Deterministic
    }
}
