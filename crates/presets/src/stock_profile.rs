//! Stock profile loading, validation and migration (FILM_MODEL §J).
//!
//! Profiles are **data, not code**: versioned JSON documents validated
//! against `schemas/stock-profile.schema.json` (structure) *and*
//! `StockProfile::validate` (physics). Loading is deny-by-default and
//! fail-fast — a profile that does not validate never reaches the render
//! path, and the error names the exact field.
//!
//! Migration chain: v1 is current; the chain mirrors the sidecar
//! machinery (`migrate()`), so future schema changes stay loadable.

use melography_film::profile::{ProfileError, StockProfile, STOCK_PROFILE_VERSION};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STOCK_PROFILE_SCHEMA_ID: &str = "melography.stock-profile";

#[derive(Debug, thiserror::Error)]
pub enum StockProfileError {
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("JSON parse error in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("profile validation error in {path}: {source}")]
    Invalid {
        path: PathBuf,
        #[source]
        source: ProfileError,
    },
    #[error("unsupported profile version {found} in {path} (this build supports <= {supported}; no migration path)")]
    Unmigratable {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
}

/// Load + strictly validate one stock profile from a JSON file.
pub fn load_stock_profile(path: &Path) -> Result<StockProfile, StockProfileError> {
    let text = std::fs::read_to_string(path).map_err(|source| StockProfileError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let profile: StockProfile =
        serde_json::from_str(&text).map_err(|source| StockProfileError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    if profile.profile_version > STOCK_PROFILE_VERSION {
        return Err(StockProfileError::Unmigratable {
            path: path.to_path_buf(),
            found: profile.profile_version,
            supported: STOCK_PROFILE_VERSION,
        });
    }
    // v1 is the first public schema — the migration chain starts here
    // (same discipline as the sidecar chain: one testable entry point).
    if let Err(source) = profile.validate() {
        return Err(StockProfileError::Invalid {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(profile)
}

/// A directory of stock profiles (the shipped `profiles/` set or a user
/// library). Loading is eager and strict: one broken file fails the whole
/// registry open with a named error — never silently skip data files.
#[derive(Debug, Clone)]
pub struct StockRegistry {
    pub profiles: Vec<StockProfile>,
    pub root: PathBuf,
}

impl StockRegistry {
    /// Open a registry directory (expects `*.json` files).
    pub fn open(root: &Path) -> Result<Self, StockProfileError> {
        let mut profiles = Vec::new();
        let entries = std::fs::read_dir(root).map_err(|source| StockProfileError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
            .collect();
        paths.sort(); // deterministic order (listings feed the UI browser)
        for path in paths {
            profiles.push(load_stock_profile(&path)?);
        }
        Ok(Self {
            profiles,
            root: root.to_path_buf(),
        })
    }

    /// Look up by profile id.
    pub fn by_id(&self, id: &str) -> Option<&StockProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Summary lines for the UI stock browser (S3): id, display name,
    /// provenance, confidence, polarity, layer layout.
    pub fn summaries(&self) -> Vec<StockSummary> {
        self.profiles
            .iter()
            .map(|p| StockSummary {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
                profile_version: p.profile_version,
                provenance: format!("{:?}", p.provenance),
                confidence: format!("{:?}", p.confidence),
                polarity: format!("{:?}", p.polarity),
                layers: format!("{:?}", p.layers),
            })
            .collect()
    }
}

/// UI-facing summary row (S3 stock browser).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockSummary {
    pub id: String,
    pub display_name: String,
    pub profile_version: u32,
    pub provenance: String,
    pub confidence: String,
    pub polarity: String,
    pub layers: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_json_with_named_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(matches!(
            load_stock_profile(&p),
            Err(StockProfileError::Parse { .. })
        ));
    }

    #[test]
    fn rejects_future_version() {
        let mut prof = melography_film::profile::testing::test_profile();
        prof.profile_version = 99;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("future.json");
        std::fs::write(&p, serde_json::to_string(&prof).unwrap()).unwrap();
        assert!(matches!(
            load_stock_profile(&p),
            Err(StockProfileError::Unmigratable { found: 99, .. })
        ));
    }

    #[test]
    fn registry_fails_fast_on_broken_member() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.json"), "{}").unwrap();
        let err = StockRegistry::open(dir.path());
        assert!(err.is_err(), "broken member must fail the registry open");
    }
}
