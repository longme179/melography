//! # melography-lut
//!
//! LUT engine (M1, issues #7 + #9): strict token-stream `.cube`
//! parsing/validation, tetrahedral CPU interpolation (the oracle-quality
//! reference), color-encoding contracts with wrap/unwrap around every LUT,
//! intensity/mix + insertion points, and the content-addressed library with
//! provenance + load-time integrity checks.
//!
//! Design contracts frozen here (COLOR_PIPELINE §1 hard rule, luts.md §2/§3):
//! * every LUT application **wraps its declared encoding** — the pipeline
//!   converts working AP1 into the LUT's declared input space, evaluates the
//!   LUT, converts the declared output back to AP1;
//! * inputs clamp to the declared domain *before* interpolation and the
//!   clamp is **reported** (amber "domain clipping" badge), never silent;
//! * unknown/malformed input is a structured error with a line number —
//!   never a best-effort guess.
//!
//! The WGSL mirror of the tetrahedral sampler lives in `melography-gpu`
//! (feature `wgpu`); both are validated against the *independent* Python
//! oracle in `tools/lut_oracle.py` (written from the math in luts.md, not
//! from this code) — see `tests/oracle_parity.rs`.

pub mod cube;
pub mod encoding;
pub mod library;
pub mod stage;
pub mod tetra;

pub use cube::{
    parse_cube, parse_cube_file, CubeError, CubeErrorKind, CubeKind, CubeLut, CubeWarning,
    ParseOptions,
};
pub use encoding::{roundtrip_error, unwrap_from, wrap_to, LutEncoding};
pub use library::{ImportProvenance, LibraryError, LibraryIndex, LutLibrary, LutRecord};
pub use stage::{apply_chain, ApplyReport, InsertionPoint, LutApply};
pub use tetra::{sample, sample1d, sample3d, ClampFlags};

use melography_core::{ColorEncoding, DeterminismClass, Stage, StageId, StageSpec};
use serde::{Deserialize, Serialize};

/// LUT error taxonomy (ARCHITECTURE §8 `LutError` class, made concrete).
#[derive(Debug, thiserror::Error)]
pub enum LutError {
    #[error("unsupported LUT format or version: {0}")]
    Unsupported(String),
    #[error("malformed LUT: {detail}")]
    Malformed { detail: String },
    #[error("LUT file {path} is malformed: {source}")]
    MalformedFile {
        path: String,
        #[source]
        source: CubeError,
    },
    #[error("encoding contract violated: LUT expects {expected}, pipeline provides {provided}")]
    EncodingMismatch { expected: String, provided: String },
    #[error("unknown encoding identifier: {0} (the fixed vocabulary lives in encoding.rs)")]
    EncodingUnknown(String),
    #[error(
        "encoding {0} is declared in the vocabulary but not implemented in this build — \
         refusing to guess (import UI should have disabled it)"
    )]
    EncodingNotImplemented(&'static str),
    #[error("LUT {hash} is missing from the library (removed or never imported)")]
    Missing { hash: String },
    #[error("LUT {hash} failed its content-address integrity check")]
    ChecksumMismatch { hash: String },
    #[error("library I/O error: {detail}")]
    LibraryIo { detail: String },
}

impl From<CubeError> for LutError {
    fn from(e: CubeError) -> Self {
        Self::Malformed {
            detail: e.to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Stage-contract view (M0 shape, kept for pipeline wiring; the concrete
// application math lives in `stage::LutApply`).
// ─────────────────────────────────────────────────────────────────────────────

/// Declared identity of a LUT (parse-derived + user-declared parts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutSpec {
    /// Content-addressed id (SHA-256 of the file bytes; library key).
    pub content_hash: String,
    pub size: u32,
    pub kind: CubeKind,
    pub declared_input: LutEncoding,
    pub declared_output: LutEncoding,
}

impl LutSpec {
    /// Build from a parsed LUT + hash + declarations.
    #[must_use]
    pub fn new(
        content_hash: impl Into<String>,
        lut: &CubeLut,
        declared_input: LutEncoding,
        declared_output: LutEncoding,
    ) -> Self {
        Self {
            content_hash: content_hash.into(),
            size: lut.size,
            kind: lut.kind,
            declared_input,
            declared_output,
        }
    }
}

/// Stage-contract view of a LUT insertion (the `Stage` trait wiring the
/// scheduler sees; math in `stage::LutApply`).
pub struct LutStage {
    pub spec: LutSpec,
}

impl Stage for LutStage {
    fn id(&self) -> StageId {
        StageId::new(format!(
            "lut.{:?}.{}",
            self.spec.kind, self.spec.content_hash
        ))
    }
    fn spec(&self) -> StageSpec {
        // The pipeline-visible contract is AP1→AP1: the declared encodings
        // are *wrapped inside* the stage (luts.md §3), so the scheduler's
        // encoding contract stays the working space. The declarations are
        // serialized in the recipe (stage::LutApply) and shown in the UI.
        StageSpec {
            input: ColorEncoding::Ap1Linear,
            output: ColorEncoding::Ap1Linear,
        }
    }
    fn determinism(&self) -> DeterminismClass {
        DeterminismClass::Deterministic
    }
    fn halo(&self) -> melography_core::Margins {
        // Point operation: no tile overlap required (tiled ≡ monolithic by
        // construction; the test suite still proves it, luts.md §5).
        melography_core::Margins::default()
    }
}
