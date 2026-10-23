//! # melography-film
//!
//! The film-emulation chain (FILM_MODEL.md stages A–I). **M0 ships the
//! identity chain only** — a real stage that passes pixels through the full
//! stage contract (id, spec, determinism, halo) so the CLI roundtrip and the
//! recipe schema exercise the real machinery. The emulsion math (curves,
//! crosstalk, halation, grain, print/scan) is M2 work, tests-first on the
//! CPU oracle per PLAN.md.
//!
//! Contract (COLOR_PIPELINE §1): film stages consume and produce AP1-linear
//! unbounded f32.

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
