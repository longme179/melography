//! User preset store (UX_SPEC §2 S5 Preset manager, issue #23).
//!
//! A preset is a *named recipe*: the same `StageEntry` list a sidecar
//! carries (film stock + adjustments + lens, tone, LUT stack), stored one
//! JSON per preset under `<library>/presets/`. Applying reuses the exact
//! stage→state code path as sidecar resume, so a preset and a re-opened
//! recipe can never drift apart.
//!
//! Distinction the UI depends on (UX_SPEC §2): stock *profiles* are
//! shipped data with calibration provenance; *user presets* are named
//! recipe snapshots — `source: "user"` and a full provenance line.

use melography_core::durable::atomic_write_durable;
use melography_presets::sidecar::StageEntry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserPreset {
    /// Stable id (hash of the *original* name at creation). Renames keep
    /// the id — files on disk never churn because of a display change.
    pub id: String,
    pub name: String,
    pub created_utc: String,
    pub modified_utc: String,
    /// Provenance line (always "user" today; import/export arrives later).
    pub source: String,
    /// The recipe payload — sidecar-shaped stages, applied verbatim.
    pub stages: Vec<StageEntry>,
}

impl Default for UserPreset {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            created_utc: String::new(),
            modified_utc: String::new(),
            source: "user".into(),
            stages: Vec::new(),
        }
    }
}

/// Stable 8-hex id from a name + creation-time salt (masked to 32 bits
/// so the id is ALWAYS exactly 8 chars — `find` depends on that). Not a
/// security boundary — a filename that is unique, filesystem-safe and
/// stable.
fn preset_id(name: &str) -> String {
    let salt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{name}\u{1f}{salt}").as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{:08x}", h & 0xffff_ffff)
}

/// Filesystem-safe display name for the store (ids are the real key; this
/// only guards against pathological names breaking the dir listing).
pub fn sanitize_name(name: &str) -> String {
    let trimmed = name.trim();
    let cleaned: String = trimmed
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " ._-()[]'\"àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches(['-', '.']).to_string();
    if cleaned.is_empty() {
        "preset".to_string()
    } else {
        cleaned.chars().take(64).collect()
    }
}

impl UserPreset {
    pub fn new(name: &str, stages: Vec<StageEntry>) -> Self {
        let now = melography_presets::utc_now_rfc3339();
        Self {
            id: preset_id(name),
            name: sanitize_name(name),
            created_utc: now.clone(),
            modified_utc: now,
            source: "user".into(),
            stages,
        }
    }

    pub fn file(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.json", self.id))
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write_durable(&self.file(dir), &bytes)
    }

    /// Save under a new id (first write).
    pub fn store_new(mut self, dir: &Path) -> std::io::Result<Self> {
        // Ensure id uniqueness against the existing set (hash collision
        // with a live preset → resalt until free; checked under the state
        // mutex by the caller, so no TOCTOU in practice).
        while self.file(dir).exists() {
            self.id = preset_id(&self.name);
        }
        self.save(dir)?;
        Ok(self)
    }

    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let p: UserPreset = serde_json::from_slice(&bytes).ok()?;
        if p.id.is_empty() || p.stages.is_empty() {
            return None; // corrupt or payload-free — skipped, never fatal
        }
        Some(p)
    }

    /// All presets, sorted by name (a stable, locale-neutral ordering;
    /// the UI re-sorts with locale-aware collation if it wants).
    pub fn list(dir: &Path) -> Vec<Self> {
        let mut out: Vec<Self> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Self::load(&e.path()))
            .collect();
        out.sort_by_key(|p| p.name.to_lowercase());
        out
    }

    pub fn find(dir: &Path, id: &str) -> Option<Self> {
        // Never trust the id as a path (defense against hand-crafted
        // input reaching the command).
        if id.chars().any(|c| !c.is_ascii_hexdigit()) || id.len() != 8 {
            return None;
        }
        Self::load(&dir.join(format!("{id}.json")))
    }

    pub fn delete(dir: &Path, id: &str) -> bool {
        match Self::find(dir, id) {
            Some(_) => std::fs::remove_file(dir.join(format!("{id}.json"))).is_ok(),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_stages() -> Vec<StageEntry> {
        use melography_color::tone::ToneParams;
        vec![StageEntry::Tone {
            params: ToneParams {
                ev: 0.5,
                ..ToneParams::default()
            },
            source: None,
            enabled: true,
            extra: Default::default(),
        }]
    }

    #[test]
    fn save_list_rename_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = UserPreset::new("Portraits warm", sample_stages());
        p.clone().store_new(dir.path()).unwrap();
        let listed = UserPreset::list(dir.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Portraits warm");
        assert_eq!(listed[0].stages.len(), 1);

        // rename = same id, new name + modified bump
        let mut renamed = listed[0].clone();
        renamed.name = sanitize_name("Portraits warm v2");
        renamed.modified_utc = melography_presets::utc_now_rfc3339();
        renamed.save(dir.path()).unwrap();
        let relisted = UserPreset::list(dir.path());
        assert_eq!(relisted.len(), 1);
        assert_eq!(relisted[0].name, "Portraits warm v2");
        assert_eq!(relisted[0].id, listed[0].id, "id survives rename");

        assert!(UserPreset::delete(dir.path(), &relisted[0].id));
        assert!(UserPreset::list(dir.path()).is_empty());
        assert!(!UserPreset::delete(dir.path(), &relisted[0].id));
    }

    #[test]
    fn find_rejects_non_hex_ids() {
        let dir = tempfile::tempdir().unwrap();
        assert!(UserPreset::find(dir.path(), "../../etc/passwd").is_none());
        assert!(UserPreset::find(dir.path(), "zzzzzzzz").is_none());
        assert!(UserPreset::find(dir.path(), "deadbeef").is_none());
    }

    #[test]
    fn sanitize_keeps_vietnamese_diacritics() {
        assert_eq!(sanitize_name("  Ánh sáng chiều  "), "Ánh sáng chiều");
        assert_eq!(sanitize_name("../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_name("///"), "preset");
        let long = "x".repeat(100);
        assert_eq!(sanitize_name(&long).len(), 64);
    }

    #[test]
    fn corrupt_preset_files_are_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.json"), b"not json").unwrap();
        std::fs::write(
            dir.path().join("empty.json"),
            br#"{"id":"abcd1234","name":"x","created_utc":"","modified_utc":"","source":"user","stages":[]}"#,
        )
        .unwrap();
        let p = UserPreset::new("good", sample_stages());
        p.store_new(dir.path()).unwrap();
        let listed = UserPreset::list(dir.path());
        assert_eq!(listed.len(), 1, "corrupt + empty stages skipped");
        assert_eq!(listed[0].name, "good");
    }
}
