//! # melography-presets
//!
//! The non-destructive-editing data layer (ARCHITECTURE §7): recipes,
//! sidecars, stock profiles, versioning/migration. **M0 ships recipe v0**
//! (what the CLI roundtrip records) **and atomic file writes**; the full
//! sidecar v1 (fingerprint policy, unknown-field preservation, migration
//! chain tests) is M1 work (issue #9).
//!
//! Non-negotiables already in force here:
//! * sidecars are versioned JSON — unknown fields are *kept inert*,
//!   never executed (deny-by-default semantics);
//! * writes are atomic (temp file + rename) so a crash never truncates a
//!   user's edit history.

use melography_core::{Channels, ImageF32};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const RECIPE_VERSION: u32 = 0;

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

/// Atomic write: temp file in the same directory, fsync, rename over the
/// target (ARCHITECTURE §7 atomic-writes rule).
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), RecipeError> {
    let dir = path
        .parent()
        .ok_or_else(|| RecipeError::Io("target has no parent directory".into()))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("recipe"),
        std::process::id()
    ));
    std::fs::write(&tmp, contents).map_err(|e| RecipeError::Io(format!("write: {e}")))?;
    // fsync the temp file before rename so the rename never publishes a
    // partially-flushed file.
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        RecipeError::Io(format!("rename: {e}"))
    })?;
    Ok(())
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
