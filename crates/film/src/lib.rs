//! # melography-film
//!
//! The film-emulation chain (FILM_MODEL.md stages A–I). **M0 shipped the
//! identity chain; M2 implements the emulsion math** (curves, crosstalk,
//! halation, grain, print/scan) stage-by-stage, tests-first on the CPU
//! oracle per PLAN.md issue #10.
//!
//! Contract (COLOR_PIPELINE §1): film stages consume and produce AP1-linear
//! unbounded f32.
//!
//! Determinism discipline (S2): all math that feeds golden hashes runs in
//! f64 with `+ − × ÷` plus `ln/exp/log10/sqrt/cos` (f64) — the S2 spike and
//! the M1 golden gates proved bit-identical results for these ops across
//! the Linux/Windows/macOS CI fleet. Integer randomness is u32-only
//! (WGSL-portable). No threading, no data-dependent summation order.

pub mod chain;
pub mod curves;
pub mod emulsion;
pub mod frame;
pub mod grain;
pub mod hash;
pub mod optics;
pub mod print;
pub mod profile;

pub use curves::{CurveDirection, CurveError, MonotoneSpline};
pub use emulsion::{Adjustments, CrossProcessMode, Emulsion, HalationKernel};
pub use frame::{CropRule, FilmFrame, FRAME_LONG_MM, FRAME_SHORT_MM};
pub use hash::{hash4, mix32, RandStream};
pub use optics::{GaussianKernel, LensParams, Plane};
pub use profile::{
    CalibrationRef, Confidence, GaussianComponent, GrainParams, HalationChannel, InputAssumption,
    InversionPath, LayerCurve, LayerLayout, Mat3, Polarity, PrintParams, ProfileError, Provenance,
    SigmaCurve, StockProfile, STOCK_PROFILE_VERSION,
};

use melography_core::{ColorEncoding, DeterminismClass, ImageF32, Stage, StageId, StageSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
#[error("film chain error: {0}")]
pub enum FilmError {
    #[error("stage {0} rejected the input: {1}")]
    StageRejected(String, String),
}

/// Version of the film-chain parameter schema (bump on breaking recipe
/// changes; migrations live in `melography-presets`).
pub const FILM_PARAMS_VERSION: u32 = 0;

/// Parameters for the M0 identity chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IdentityParams {
    /// Bypass flag (identity is bypass by definition; the flag exists so the
    /// recipe serialization shape matches future stages).
    #[serde(default)]
    pub enabled: bool,
}

/// The M0 film chain: a single identity stage. Real chain composition
/// (ordered stage list, guardrails) arrives with M2; the shape below is the
/// frozen part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FilmChain {
    pub version: u32,
    pub identity: IdentityParams,
}

impl FilmChain {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            version: FILM_PARAMS_VERSION,
            identity: IdentityParams { enabled: true },
        }
    }

    /// Apply the chain. Identity: returns a clone (unbounded values pass
    /// through untouched — CP §1 hard rule).
    pub fn apply(&self, input: &ImageF32) -> Result<ImageF32, FilmError> {
        Ok(input.clone())
    }
}

/// Stage-contract view of the identity stage.
pub struct IdentityStage;

impl Stage for IdentityStage {
    fn id(&self) -> StageId {
        StageId::new("film.identity.v0")
    }
    fn spec(&self) -> StageSpec {
        StageSpec {
            input: ColorEncoding::Ap1Linear,
            output: ColorEncoding::Ap1Linear,
        }
    }
    fn determinism(&self) -> DeterminismClass {
        DeterminismClass::Deterministic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use melography_core::Channels;

    #[test]
    fn identity_preserves_unbounded_values() {
        let img =
            ImageF32::from_interleaved(2, 1, Channels::Rgb, vec![-0.5, 0.0, 1.5, 2.0, -2.0, 0.25]);
        let out = FilmChain::new().apply(&img).unwrap();
        assert_eq!(out, img);
    }

    #[test]
    fn chain_serializes_stably() {
        let chain = FilmChain::new();
        let json = serde_json::to_string(&chain).unwrap();
        assert!(json.contains("\"version\":0"));
        let back: FilmChain = serde_json::from_str(&json).unwrap();
        assert_eq!(back, chain);
    }

    #[test]
    fn identity_stage_contract() {
        let s = IdentityStage;
        assert_eq!(s.id().0, "film.identity.v0");
        assert_eq!(s.spec().input, ColorEncoding::Ap1Linear);
        assert_eq!(s.determinism(), DeterminismClass::Deterministic);
        assert_eq!(s.halo(), melography_core::Margins::default());
    }
}
