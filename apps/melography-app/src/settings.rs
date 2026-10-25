//! Persisted app settings (UX_SPEC §7 Settings window, issue #23).
//!
//! One JSON document in the library root (`settings.json`), written through
//! the shared durable primitive (kill-battery-tested — a crash mid-save
//! leaves the previous complete settings). Loading is schema-tolerant in
//! the *sidecar* sense: unknown fields survive round-trips; a corrupt or
//! missing file falls back to defaults and never blocks startup.

use melography_core::durable::atomic_write_durable;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// UX_SPEC §7: `ask` is the default — updates never start silently.
pub const UPDATE_CHANNELS: [&str; 4] = ["ask", "none", "stable", "beta"];
pub const THEMES: [&str; 3] = ["dark", "light", "contrast"];
pub const LANGS: [&str; 2] = ["en", "vi"];
pub const LOG_LEVELS: [&str; 4] = ["error", "warn", "info", "debug"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// UI language (`en` | `vi`) — UX_SPEC §10.
    pub language: String,
    /// `dark` | `light` | `contrast` (the high-contrast variant, §10).
    pub theme: String,
    /// Preview transport max dimension (px). Larger = sharper, slower.
    pub preview_max_dim: u32,
    /// Soft budget for the render caches (MiB) — advisory today, enforced
    /// when the tile-cache work lands (#25).
    pub cache_budget_mb: u32,
    /// GPU preference (the preview pipeline is CPU-side today — the UI
    /// labels this honestly; the flag drives the GPU path when it ships).
    pub gpu_enabled: bool,
    /// `ask` | `none` | `stable` | `beta` (ADR-0005 consent flow).
    pub update_channel: String,
    /// Offline-first explainer toggle (nothing leaves the machine either
    /// way — the setting gates *optional* update/model checks only).
    pub offline_mode: bool,
    /// `error` | `warn` | `info` | `debug`.
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            theme: "dark".into(),
            preview_max_dim: 1600,
            cache_budget_mb: 512,
            gpu_enabled: true,
            update_channel: "ask".into(),
            offline_mode: true,
            log_level: "info".into(),
        }
    }
}

impl Settings {
    /// Load from `path`; a missing file = fresh defaults. A *corrupt*
    /// file also falls back to defaults (settings must never wedge the
    /// app), but the parse failure is reported so the caller can surface
    /// it honestly instead of pretending nothing happened.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Settings>(&bytes) {
                Ok(s) => {
                    let s = s.clamped();
                    (s, None)
                }
                Err(e) => (Self::default(), Some(format!("settings corrupt: {e}"))),
            },
            Err(_) => (Self::default(), None),
        }
    }

    /// Durable save (tmp → fsync → rename → dir-fsync).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        atomic_write_durable(path, &bytes)
    }

    /// Clamp user-editable values into their valid ranges (defense in
    /// depth: the UI validates too, but a hand-edited file must not
    /// smuggle garbage into the render path). Applied at load AND at
    /// `set_settings` (A1: an out-of-range `preview_max_dim` must never
    /// reach the render fallback, not even for one session).
    pub(crate) fn clamped(self) -> Self {
        let mut s = self;
        if !LANGS.contains(&s.language.as_str()) {
            s.language = "en".into();
        }
        if !THEMES.contains(&s.theme.as_str()) {
            s.theme = "dark".into();
        }
        if !UPDATE_CHANNELS.contains(&s.update_channel.as_str()) {
            s.update_channel = "ask".into();
        }
        if !LOG_LEVELS.contains(&s.log_level.as_str()) {
            s.log_level = "info".into();
        }
        s.preview_max_dim = s.preview_max_dim.clamp(256, 2560);
        s.cache_budget_mb = s.cache_budget_mb.clamp(64, 4096);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let s = Settings::default();
        let c = s.clone().clamped();
        assert_eq!(s, c, "factory defaults must survive clamping");
    }

    #[test]
    fn corrupt_file_falls_back_with_report() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, b"{ not json").unwrap();
        let (s, err) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(err.is_some(), "corruption is reported, never silent");
    }

    #[test]
    fn roundtrip_and_clamp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings {
            language: "vi".into(),
            theme: "contrast".into(),
            preview_max_dim: 9999, // out of range → 2560
            cache_budget_mb: 1,    // out of range → 64
            update_channel: "beta".into(),
            ..Settings::default()
        };
        s.save(&p).unwrap();
        let (loaded, err) = Settings::load(&p);
        assert!(err.is_none());
        assert_eq!(loaded.language, "vi");
        assert_eq!(loaded.preview_max_dim, 2560);
        assert_eq!(loaded.cache_budget_mb, 64);
    }

    #[test]
    fn garbage_enums_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            br#"{"language":"xx","theme":"neon","update_channel":"silent","log_level":"loud"}"#,
        )
        .unwrap();
        let (s, _) = Settings::load(&p);
        assert_eq!(s.language, "en");
        assert_eq!(s.theme, "dark");
        assert_eq!(s.update_channel, "ask");
        assert_eq!(s.log_level, "info");
    }
}
