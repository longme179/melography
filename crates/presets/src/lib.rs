//! # melography-presets
//!
//! The non-destructive-editing data layer (ARCHITECTURE §7): recipes,
//! sidecars, stock profiles, versioning/migration. **M1 ships sidecar v1**
//! (fingerprint policy, LUT-by-hash stages, unknown-field preservation,
//! the v0→v1 migration chain — `sidecar.rs`, issue #9); recipe v0 remains
//! readable forever via `sidecar::migrate`.
//!
//! Non-negotiables already in force here:
//! * sidecars are versioned JSON — unknown fields are *kept inert*,
//!   never executed (deny-by-default semantics);
//! * writes are atomic (temp file + rename) so a crash never truncates a
//!   user's edit history.

pub mod sidecar;
pub mod stock_profile;

pub use sidecar::{
    migrate, Fingerprint, FingerprintMatch, ImageRef, Pipeline, SidecarV1, StageEntry,
    SIDECAR_SCHEMA, SIDECAR_VERSION,
};
pub use stock_profile::{
    load_stock_profile, StockProfileError, StockRegistry, StockSummary, STOCK_PROFILE_SCHEMA_ID,
};

use melography_core::{Channels, ImageF32};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const RECIPE_VERSION: u32 = 0;

pub(crate) fn default_true() -> bool {
    true
}

/// RFC 3339 UTC timestamp without a chrono dependency (same civil-date
/// algorithm as the lut library module; shared here for sidecars and the
/// app's settings/recents/presets stores).
pub fn utc_now_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

#[derive(Debug, thiserror::Error)]
#[error("recipe error: {0}")]
pub enum RecipeError {
    #[error("unsupported recipe version {0} (supported: {RECIPE_VERSION})")]
    UnsupportedVersion(u32),
    #[error("IO error: {0}")]
    Io(String),
}

/// Recipe v0: a decoded input + the film chain + insertion points. This is
/// the shape the M0 CLI roundtrip records; M1 grows it into the full sidecar
/// (LUT references by content hash, AI suggestions with provenance, undo
/// history — all additive, version-bumped fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecipeV0 {
    pub version: u32,
    /// Input encoding recorded at decode (provenance for the transforms).
    pub input_encoding: InputEncodingRecord,
    /// The film chain parameters (identity at M0).
    pub film: serde_json::Value, // melography-film::FilmChain — kept as JSON here to avoid a crate cycle; typed at the edges
    /// Working space is always AP1 (COLOR_PIPELINE §4) — recorded for audit.
    pub working_space: WorkingSpace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSpace {
    Ap1,
}

/// Record of the input encoding a recipe was authored against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InputEncodingRecord {
    UntaggedSrgb,
    Icc { sha256: String },
}

/// Deterministic recipe fingerprint for the input image (M0: dimensions +
/// content hash of the decoded buffer; M1 replaces this with the full
/// multi-hash fingerprint policy — issue #9).
#[must_use]
pub fn input_fingerprint(decoded: &ImageF32) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(decoded.width().to_le_bytes());
    h.update(decoded.height().to_le_bytes());
    h.update([match decoded.channels() {
        Channels::Rgb => 3u8,
        Channels::Rgba => 4u8,
    }]);
    h.update(decoded.as_bytes());
    format!("{:x}", h.finalize())
}

/// Serialize a recipe to pretty JSON (stable field order via serde derive).
pub fn to_json(recipe: &RecipeV0) -> Result<String, RecipeError> {
    serde_json::to_string_pretty(recipe).map_err(|e| RecipeError::Io(e.to_string()))
}

/// Parse a recipe, enforcing the version gate.
pub fn from_json(json: &str) -> Result<RecipeV0, RecipeError> {
    let recipe: RecipeV0 =
        serde_json::from_str(json).map_err(|e| RecipeError::Io(format!("parse: {e}")))?;
    if recipe.version != RECIPE_VERSION {
        return Err(RecipeError::UnsupportedVersion(recipe.version));
    }
    Ok(recipe)
}

/// Atomic durable write, delegated to the shared primitive
/// (`melography_core::durable`): temp sibling → write-handle `sync_all` →
/// drop → rename → POSIX dir-fsync (ARCHITECTURE §7 atomic-writes rule).
///
/// Historical note: the previous local implementation opened the temp
/// file READ-ONLY for its fsync — on Windows `FlushFileBuffers` requires
/// write access and fails with `ERROR_ACCESS_DENIED`, a failure the old
/// code silently ignored, so Windows saves shipped without any real
/// sync. The shared primitive syncs on the write-mode handle (the exact
/// bug class that failed windows-latest CI on the M3 manager).
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), RecipeError> {
    melography_core::durable::atomic_write_durable(path, contents)
        .map_err(|e| RecipeError::Io(format!("atomic write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RecipeV0 {
        RecipeV0 {
            version: RECIPE_VERSION,
            input_encoding: InputEncodingRecord::UntaggedSrgb,
            film: serde_json::json!({"version": 0, "identity": {"enabled": true}}),
            working_space: WorkingSpace::Ap1,
        }
    }

    #[test]
    fn recipe_json_roundtrip() {
        let json = to_json(&sample()).unwrap();
        assert!(json.contains("\"version\": 0"));
        let back = from_json(&json).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn future_version_is_rejected_explicitly() {
        let mut json = to_json(&sample()).unwrap();
        json = json.replace("\"version\": 0", "\"version\": 99");
        assert!(matches!(
            from_json(&json),
            Err(RecipeError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("recipe.json");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        // no temp litter
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files must be cleaned: {leftovers:?}"
        );
    }
}
