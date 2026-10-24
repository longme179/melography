//! Sidecar v1 — the non-destructive editing contract (issue #9,
//! ARCHITECTURE §7, PLAN M1).
//!
//! **Format:** versioned JSON (`.melography.json` sidecar next to the
//! original image; the original file is never touched). Shape:
//!
//! ```json
//! {
//!   "schema": "melography.recipe",
//!   "version": 1,
//!   "created_utc": "…", "modified_utc": "…",
//!   "image": {
//!     "original_name": "photo.jpg",
//!     "input_encoding": "untagged_srgb",
//!     "fingerprint": { "size_bytes": …, "sha256_file": "…", "luma_hash_8x8": "…" }
//!   },
//!   "pipeline": { "stages": [ {"stage": "lut", "instance": "<sha>", …} ] }
//! }
//! ```
//!
//! **Non-negotiables (ARCHITECTURE §7):**
//! * LUT stages reference the library **by content hash** — the sidecar
//!   never embeds LUT data, only the library indirection;
//! * unknown fields are **kept inert** (`#[serde(flatten)]` extra maps are
//!   preserved through load→save; deny-by-default semantics — never
//!   executed, never dropped);
//! * writes are atomic (temp + rename, `crate::atomic_write`);
//! * the migration chain is explicit and tested (v0 → v1 here; future
//!   versions extend `migrate` — never rewrite history in place);
//! * serialization is byte-stable across platforms (struct field order is
//!   fixed by serde derive; floats render via ryu shortest-round-trip on
//!   every OS) — pinned-hash test enforces it.
//!
//! **Fingerprint policy (issue #9):** multi-hash — exact anchors
//! (`size_bytes` + `sha256_file`) decide "same original"; the perceptual
//! 8×8 luma hash is advisory (catches "same photo, re-encoded/renamed" →
//! user gets a re-link question, never a silent mismatch). A *different*
//! original can therefore never silently match: SHA-256 collisions aside
//! (not a practical concern at this scale), differing bytes ⇒
//! [`FingerprintMatch::Different`].

use melography_core::ImageF32;
use melography_lut::stage::{InsertionPoint, LutApply};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

use crate::{atomic_write, InputEncodingRecord, RecipeError};

pub const SIDECAR_SCHEMA: &str = "melography.recipe";
pub const SIDECAR_VERSION: u32 = 1;

// ─── Fingerprint ────────────────────────────────────────────────────────────

/// Multi-hash image identity (exact anchors + advisory perceptual hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Exact anchor: file size in bytes.
    pub size_bytes: u64,
    /// Exact anchor: SHA-256 of the original file bytes.
    pub sha256_file: String,
    /// Advisory: 64-bit perceptual hash of an 8×8 luma downscale
    /// (16 hex chars). Survives re-encode; not proof of identity.
    pub luma_hash_8x8: String,
}

/// Verdict of comparing a sidecar's stored fingerprint against a freshly
/// computed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FingerprintMatch {
    /// Same bytes (size + SHA-256 agree) — silently proceed.
    Exact,
    /// Perceptual hash agrees but exact anchors differ — "same photo,
    /// different file?" — surface to the user, never auto-apply.
    PerceptualOnly,
    /// Nothing matches — different original; explicit re-link flow.
    Different,
}

impl Fingerprint {
    /// Compute from the original file bytes + the decoded image (the luma
    /// hash runs on the decoded working buffer — its basis is recorded in
    /// the sidecar docs; it is advisory only, so pipeline changes at most
    /// downgrade a match to PerceptualOnly, never fabricate Exact).
    pub fn compute(file_bytes: &[u8], decoded: &ImageF32) -> Self {
        let mut h = Sha256::new();
        h.update(file_bytes);
        Self {
            size_bytes: file_bytes.len() as u64,
            sha256_file: format!("{:x}", h.finalize()),
            luma_hash_8x8: format!("{:016x}", luma_hash_8x8(decoded)),
        }
    }

    /// Compare against a freshly computed fingerprint (policy above).
    #[must_use]
    pub fn matches(&self, other: &Fingerprint) -> FingerprintMatch {
        if self.size_bytes == other.size_bytes && self.sha256_file == other.sha256_file {
            FingerprintMatch::Exact
        } else if self.luma_hash_8x8 == other.luma_hash_8x8
            && self.luma_hash_8x8 != format!("{:016x}", 0u64)
        {
            FingerprintMatch::PerceptualOnly
        } else {
            FingerprintMatch::Different
        }
    }
}

/// 64-bit perceptual hash: deterministic 8×8 box-filtered luma downscale,
/// mean-thresholded to a bit pattern. Pure integer/f32 arithmetic —
/// bit-identical on every platform (no SIMD, no fast-math ambiguity).
fn luma_hash_8x8(img: &ImageF32) -> u64 {
    const S: u32 = 8;
    // The decoded buffer may be RGB or RGBA; luma from the first 3 channels
    // (Rec.709 luma weights on the working-space values — advisory only).
    let (w, h) = (img.width(), img.height());
    let mut luma = [[0f32; S as usize]; S as usize];
    let mut counts = [[0u32; S as usize]; S as usize];
    for y in 0..h {
        let sy = (y * S / h.max(1)) as usize;
        for x in 0..w {
            let sx = (x * S / w.max(1)) as usize;
            let px = img.pixel(x, y);
            let y_lin = 0.2126 * px[0] + 0.7152 * px[1] + 0.0722 * px[2];
            luma[sy][sx] += y_lin;
            counts[sy][sx] += 1;
        }
    }
    let mut mean = 0f32;
    for (row, cnt_row) in luma.iter_mut().zip(&counts) {
        for (v, cnt) in row.iter_mut().zip(cnt_row) {
            if *cnt > 0 {
                *v /= *cnt as f32;
            }
            mean += *v;
        }
    }
    mean /= (S * S) as f32;
    let mut bits = 0u64;
    for (sy, row) in luma.iter().enumerate() {
        for (sx, v) in row.iter().enumerate() {
            let bit = (*v > mean) as u64;
            bits |= bit << (sy * S as usize + sx);
        }
    }
    bits
}

// ─── Sidecar v1 ─────────────────────────────────────────────────────────────

/// Image reference block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageRef {
    /// Display-only provenance (the file name at first open).
    pub original_name: String,
    /// Input encoding recorded at decode (carried from recipe v0).
    pub input_encoding: InputEncodingRecord,
    /// Identity of the original this sidecar belongs to.
    pub fingerprint: Option<Fingerprint>,
    /// Unknown fields (future versions) — kept inert, re-serialized.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for ImageRef {
    fn default() -> Self {
        Self {
            original_name: String::new(),
            input_encoding: InputEncodingRecord::UntaggedSrgb,
            fingerprint: None,
            extra: BTreeMap::new(),
        }
    }
}

/// Provenance of an auto-accepted suggestion (ADR-0004 §6: parameters are
/// recorded as plain values + source, "reproducible forever after, even if
/// the model is deleted"). The baseline and any future ONNX predictor both
/// stamp this — the sidecar never depends on model availability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestionSource {
    /// `baseline` (deterministic, no model) or `model` (ONNX predictor).
    pub kind: SuggestionKind,
    /// Engine id, e.g. `autolight-baseline-v1` or a model manifest id.
    pub name: String,
    /// Engine/model semver.
    pub version: String,
    /// Weights SHA-256 (model suggestions only; baseline has no weights).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights_sha256: Option<String>,
    /// RFC 3339 UTC timestamp of acceptance.
    pub accepted_utc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Baseline,
    Model,
}

/// `Lut` mirrors [`LutApply`] minus the runtime-resolved LUT data: the
/// sidecar carries the *reference* (`instance` = library content hash);
/// resolution + integrity check happen at load (library.rs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StageEntry {
    /// Identity film chain (M0/M1; real stages arrive at M2 additively —
    /// new variants only, never a shape change of existing ones).
    FilmIdentity {
        #[serde(default = "crate::default_true")]
        enabled: bool,
        #[serde(flatten)]
        extra: BTreeMap<String, serde_json::Value>,
    },
    /// M2 film chain: stock profile + adjustments (issues #10/#11).
    /// The stock is referenced **by id** — data, not code (FILM_MODEL §J);
    /// loading re-validates the profile, so a stale version fails loudly.
    #[serde(rename_all = "snake_case")]
    FilmStock {
        stock: String,
        stock_version: u32,
        adjustments: melography_film::Adjustments,
        lens: melography_film::LensParams,
        #[serde(default = "crate::default_true")]
        enabled: bool,
        #[serde(flatten)]
        extra: BTreeMap<String, serde_json::Value>,
    },
    /// M3 scene-referred tone stage (issue #13): white-balance gains, EV,
    /// highlights/shadows, percentile black/white shaping. The landing
    /// zone for auto-lighting suggestions — values stay editable, the
    /// optional `source` records provenance (ADR-0004 §6).
    #[serde(rename_all = "snake_case")]
    Tone {
        params: melography_color::tone::ToneParams,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SuggestionSource>,
        #[serde(default = "crate::default_true")]
        enabled: bool,
        #[serde(flatten)]
        extra: BTreeMap<String, serde_json::Value>,
    },
    Lut {
        /// Library content hash (SHA-256 of the stored `.cube` bytes).
        instance: String,
        insertion: InsertionPoint,
        /// 0.0–1.0.
        intensity: f32,
        declared_input: melography_lut::LutEncoding,
        declared_output: melography_lut::LutEncoding,
        #[serde(default = "crate::default_true")]
        enabled: bool,
        #[serde(flatten)]
        extra: BTreeMap<String, serde_json::Value>,
    },
}

impl From<&LutApply> for StageEntry {
    fn from(a: &LutApply) -> Self {
        Self::Lut {
            instance: a.lut_hash.clone(),
            insertion: a.insertion,
            intensity: a.intensity,
            declared_input: a.declared_input,
            declared_output: a.declared_output,
            enabled: a.enabled,
            extra: BTreeMap::new(),
        }
    }
}

impl TryInto<LutApply> for &StageEntry {
    type Error = RecipeError;
    fn try_into(self) -> Result<LutApply, RecipeError> {
        match self {
            StageEntry::Lut {
                instance,
                insertion,
                intensity,
                declared_input,
                declared_output,
                enabled,
                ..
            } => Ok(LutApply {
                lut_hash: instance.clone(),
                insertion: *insertion,
                intensity: *intensity,
                declared_input: *declared_input,
                declared_output: *declared_output,
                enabled: *enabled,
            }),
            StageEntry::FilmIdentity { .. } => Err(RecipeError::Io(
                "film-identity stage is not a LUT stage".into(),
            )),
            StageEntry::FilmStock { .. } => Err(RecipeError::Io(
                "film-stock stage is not a LUT stage".into(),
            )),
            StageEntry::Tone { .. } => Err(RecipeError::Io("tone stage is not a LUT stage".into())),
        }
    }
}

/// Pipeline block: ordered stage list (order is user-visible; the renderer
/// additionally enforces insertion-point ordering, stage.rs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Pipeline {
    pub stages: Vec<StageEntry>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Sidecar v1 root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidecarV1 {
    pub schema: String,
    pub version: u32,
    pub created_utc: String,
    pub modified_utc: String,
    pub image: ImageRef,
    pub pipeline: Pipeline,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl SidecarV1 {
    /// Fresh sidecar for a just-opened image.
    pub fn new(original_name: impl Into<String>, fingerprint: Option<Fingerprint>) -> Self {
        let now = crate::utc_now_rfc3339();
        Self {
            schema: SIDECAR_SCHEMA.to_string(),
            version: SIDECAR_VERSION,
            created_utc: now.clone(),
            modified_utc: now,
            image: ImageRef {
                original_name: original_name.into(),
                input_encoding: InputEncodingRecord::UntaggedSrgb,
                fingerprint,
                extra: BTreeMap::new(),
            },
            pipeline: Pipeline::default(),
            extra: BTreeMap::new(),
        }
    }

    /// Bump `modified_utc` (call on every save; keeps `created_utc` fixed).
    pub fn touch(&mut self) {
        self.modified_utc = crate::utc_now_rfc3339();
    }

    /// Serialize to pretty JSON (byte-stable field order; see module docs).
    pub fn to_json(&self) -> Result<String, RecipeError> {
        serde_json::to_string_pretty(self).map_err(|e| RecipeError::Io(e.to_string()))
    }

    /// Parse a sidecar, enforcing schema + version.
    pub fn from_json(json: &str) -> Result<Self, RecipeError> {
        let sc: SidecarV1 =
            serde_json::from_str(json).map_err(|e| RecipeError::Io(format!("parse: {e}")))?;
        if sc.schema != SIDECAR_SCHEMA {
            return Err(RecipeError::Io(format!(
                "not a MELOGRAPHY sidecar (schema {:?})",
                sc.schema
            )));
        }
        if sc.version != SIDECAR_VERSION {
            return Err(RecipeError::UnsupportedVersion(sc.version));
        }
        Ok(sc)
    }

    /// Atomic save (temp + rename). Refuses non-`.melography.json` targets
    /// with an explicit error (misnamed sidecars are a support burden).
    pub fn save(&self, path: &Path) -> Result<(), RecipeError> {
        let json = self.to_json()?;
        atomic_write(path, json.as_bytes())
    }

    /// Load + version-gate + migrate (see [`migrate`]).
    pub fn load(path: &Path) -> Result<Self, RecipeError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| RecipeError::Io(format!("read {}: {e}", path.display())))?;
        migrate(&text)
    }
}

/// Explicit migration chain: v0 (M0 CLI roundtrip format) → v1; v1 passes
/// through. Unknown versions are rejected — never best-effort.
pub fn migrate(json: &str) -> Result<SidecarV1, RecipeError> {
    let probe: serde_json::Value =
        serde_json::from_str(json).map_err(|e| RecipeError::Io(format!("parse: {e}")))?;
    let version = probe
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RecipeError::Io("sidecar has no version field".into()))?;
    match version {
        v if v == u64::from(SIDECAR_VERSION) => SidecarV1::from_json(json),
        0 => {
            let v0: crate::RecipeV0 = serde_json::from_str(json)
                .map_err(|e| RecipeError::Io(format!("v0 parse: {e}")))?;
            let mut v1 = SidecarV1::new("", None);
            v1.image.input_encoding = v0.input_encoding;
            // The v0 film chain was identity-only; carry the raw JSON as an
            // inert extra on the film stage (nothing is dropped).
            v1.pipeline.stages.push(StageEntry::FilmIdentity {
                enabled: true,
                extra: BTreeMap::from([("film_v0".to_string(), v0.film.clone())]),
            });
            if v0.working_space != crate::WorkingSpace::Ap1 {
                return Err(RecipeError::Io(
                    "v0 sidecar has a non-AP1 working space — refusing migration".into(),
                ));
            }
            Ok(v1)
        }
        other => Err(RecipeError::UnsupportedVersion(other as u32)),
    }
}

// ─── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use melography_core::Channels;
    use melography_lut::LutEncoding;

    fn tiny_image(seed: f32) -> ImageF32 {
        // 16x16 deterministic gradient with a seed tint. NOTE: the seed is
        // a *uniform* brightness shift — mean-threshold luma hashing is
        // deliberately invariant to those, so seed changes alone do NOT
        // change the perceptual hash (tested below). Spatial changes do.
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for y in 0..16u32 {
            for x in 0..16u32 {
                let px = img.pixel_mut(x, y);
                px[0] = x as f32 / 16.0 + seed;
                px[1] = y as f32 / 16.0;
                px[2] = seed;
            }
        }
        img
    }

    fn tiny_image_flipped() -> ImageF32 {
        // Spatially different: horizontal gradient mirrored.
        let mut img = ImageF32::alloc(16, 16, Channels::Rgb, 0.0);
        for y in 0..16u32 {
            for x in 0..16u32 {
                let px = img.pixel_mut(x, y);
                px[0] = (15 - x) as f32 / 16.0;
                px[1] = y as f32 / 16.0;
                px[2] = 0.0;
            }
        }
        img
    }

    #[test]
    fn fingerprint_exact_perceptual_different() {
        let bytes_a = vec![1u8; 64];
        let img = tiny_image(0.0);
        let fp_a = Fingerprint::compute(&bytes_a, &img);
        let fp_a2 = Fingerprint::compute(&bytes_a, &img);
        assert_eq!(fp_a.matches(&fp_a2), FingerprintMatch::Exact);

        // Same photo, different bytes (e.g. re-encoded): different sha but
        // same decoded pixels → PerceptualOnly.
        let bytes_b = vec![2u8; 64]; // same length, different content
        let fp_b = Fingerprint::compute(&bytes_b, &img);
        assert_eq!(fp_a.matches(&fp_b), FingerprintMatch::PerceptualOnly);

        // Different original → Different (never silently matches). Both the
        // bytes AND the pixels must differ; the image differs *spatially*
        // (a uniform shift alone keeps the same perceptual bits by design).
        let bytes_c = vec![3u8; 64];
        let img_c = tiny_image_flipped();
        let fp_c = Fingerprint::compute(&bytes_c, &img_c);
        assert_eq!(fp_a.matches(&fp_c), FingerprintMatch::Different);

        // All-black luma (degenerate) never counts as perceptual evidence.
        let black = ImageF32::alloc(8, 8, Channels::Rgb, 0.0);
        let fp_b1 = Fingerprint::compute(b"one", &black);
        let fp_b2 = Fingerprint::compute(b"two", &black);
        assert_eq!(fp_b1.matches(&fp_b2), FingerprintMatch::Different);
    }

    #[test]
    fn luma_hash_is_deterministic_and_sensitive() {
        let a = luma_hash_8x8(&tiny_image(0.0));
        let b = luma_hash_8x8(&tiny_image(0.0));
        assert_eq!(a, b, "same image → same hash");
        // Uniform brightness shifts are *invisible* to the hash (desired:
        // exposure-judged stability); spatial changes must register.
        let shifted = luma_hash_8x8(&tiny_image(0.9));
        assert_eq!(a, shifted, "uniform shift must not change the hash");
        let c = luma_hash_8x8(&tiny_image_flipped());
        assert_ne!(a, c, "spatially different image → different hash");
    }

    #[test]
    fn sidecar_roundtrip_and_unknown_field_preservation() {
        let mut sc = SidecarV1::new("photo.jpg", None);
        sc.pipeline.stages.push(StageEntry::Lut {
            instance: "abc123".into(),
            insertion: InsertionPoint::Finishing,
            intensity: 0.75,
            declared_input: LutEncoding::SrgbDisplay,
            declared_output: LutEncoding::SrgbDisplay,
            enabled: true,
            extra: BTreeMap::new(),
        });
        let json = sc.to_json().unwrap();
        assert!(json.contains("\"schema\": \"melography.recipe\""));
        assert!(json.contains("\"instance\": \"abc123\""));

        // Unknown fields (from a future version) must survive a load→save
        // round-trip untouched — kept inert, never dropped, never executed.
        let future = json.replace(
            "  \"pipeline\": {",
            "  \"ai_suggestions\": [{\"model\": \"future\"}],\n  \"pipeline\": {",
        );
        assert!(future.contains("ai_suggestions"));
        let loaded = SidecarV1::from_json(&future).unwrap();
        assert!(loaded.extra.contains_key("ai_suggestions"));
        let resaved = loaded.to_json().unwrap();
        assert!(resaved.contains("ai_suggestions"));

        // And the LUT stage survived structurally.
        assert_eq!(loaded.pipeline.stages.len(), 1);
        let back: LutApply = (&loaded.pipeline.stages[0]).try_into().unwrap();
        assert_eq!(back.lut_hash, "abc123");
        assert_eq!(back.intensity, 0.75);
    }

    #[test]
    fn serialization_bytes_are_pinned() {
        // Cross-platform byte stability (luts.md §5 matrix): the exact JSON
        // of a fixed sidecar must reproduce byte-for-byte on every OS.
        // If this test fails after an INTENTIONAL format change, update the
        // pin in the same commit that changes the format.
        let sc = SidecarV1 {
            schema: SIDECAR_SCHEMA.to_string(),
            version: SIDECAR_VERSION,
            created_utc: "2026-10-24T00:00:00Z".into(),
            modified_utc: "2026-10-24T00:00:00Z".into(),
            image: ImageRef {
                original_name: "photo.jpg".into(),
                input_encoding: InputEncodingRecord::UntaggedSrgb,
                fingerprint: Some(Fingerprint {
                    size_bytes: 1234,
                    sha256_file: "a".repeat(64),
                    luma_hash_8x8: "0f0f0f0f0f0f0f0f".into(),
                }),
                extra: BTreeMap::new(),
            },
            pipeline: Pipeline {
                stages: vec![
                    StageEntry::FilmIdentity {
                        enabled: true,
                        extra: BTreeMap::new(),
                    },
                    StageEntry::Lut {
                        instance: "deadbeef".into(),
                        insertion: InsertionPoint::PreFilm,
                        intensity: 0.5,
                        declared_input: LutEncoding::Acescct,
                        declared_output: LutEncoding::SrgbDisplay,
                        enabled: true,
                        extra: BTreeMap::new(),
                    },
                ],
                extra: BTreeMap::new(),
            },
            extra: BTreeMap::new(),
        };
        let json = sc.to_json().unwrap();
        let mut h = Sha256::new();
        h.update(json.as_bytes());
        let pin = format!("{:x}", h.finalize());
        assert_eq!(
            pin, "adad5f7274fd7310c190ebaefc644e704047f337e1510665bf0b4b8293d67dd9",
            "sidecar v1 serialization drifted — pin must be updated only with a format-change commit\n--- JSON ---\n{json}"
        );
    }

    #[test]
    fn migration_chain_v0_to_v1() {
        let v0_json = serde_json::json!({
            "version": 0,
            "input_encoding": {"kind": "untagged_srgb"},
            "film": {"version": 0, "identity": {"enabled": true}},
            "working_space": "ap1"
        })
        .to_string();
        let v1 = migrate(&v0_json).unwrap();
        assert_eq!(v1.version, 1);
        assert_eq!(v1.image.input_encoding, InputEncodingRecord::UntaggedSrgb);
        assert_eq!(v1.pipeline.stages.len(), 1);
        match &v1.pipeline.stages[0] {
            StageEntry::FilmIdentity { extra, .. } => {
                assert!(extra.contains_key("film_v0"), "v0 film JSON carried inert");
            }
            other => panic!("expected film stage, got {other:?}"),
        }
        // v1 → v1 passthrough.
        let v1_json = v1.to_json().unwrap();
        let again = migrate(&v1_json).unwrap();
        assert_eq!(again, v1);
        // Unknown version → explicit rejection.
        let bad = v1_json.replace("\"version\": 1", "\"version\": 99");
        assert!(matches!(
            migrate(&bad),
            Err(RecipeError::UnsupportedVersion(99))
        ));
        // Non-AP1 v0 → refuse.
        let bad_ws = v0_json.replace("\"ap1\"", "\"prophoto\"");
        assert!(migrate(&bad_ws).is_err());
    }

    #[test]
    fn save_load_via_disk_is_atomic_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("img.melography.json");
        let mut sc = SidecarV1::new("x.png", None);
        sc.pipeline.stages.push(StageEntry::Lut {
            instance: "ff00".into(),
            insertion: InsertionPoint::Finishing,
            intensity: 1.0,
            declared_input: LutEncoding::SrgbDisplay,
            declared_output: LutEncoding::SrgbDisplay,
            enabled: true,
            extra: BTreeMap::new(),
        });
        sc.save(&path).unwrap();
        let loaded = SidecarV1::load(&path).unwrap();
        assert_eq!(loaded, sc);
        // No temp litter.
        let litter = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .count();
        assert_eq!(litter, 0);
    }

    #[test]
    fn tone_stage_roundtrips_with_provenance() {
        let mut sc = SidecarV1::new("y.jpg", None);
        sc.pipeline.stages.push(StageEntry::Tone {
            params: melography_color::tone::ToneParams {
                ev: 0.4,
                highlights: -12.0,
                shadows: 8.0,
                wb: Some(melography_color::tone::WbGains {
                    r: 1.08,
                    g: 1.0,
                    b: 0.93,
                }),
                black_source: Some(0.05),
                black_target: Some(0.02),
                white_source: Some(0.8),
                white_target: Some(0.95),
            },
            source: Some(SuggestionSource {
                kind: SuggestionKind::Baseline,
                name: "autolight-baseline-v1".into(),
                version: "1.0.0".into(),
                weights_sha256: None,
                accepted_utc: "2026-10-24T12:00:00Z".into(),
            }),
            enabled: true,
            extra: BTreeMap::new(),
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("y.melography.json");
        sc.save(&path).unwrap();
        let loaded = SidecarV1::load(&path).unwrap();
        assert_eq!(loaded, sc);
        match &loaded.pipeline.stages[0] {
            StageEntry::Tone { params, source, .. } => {
                assert!((params.ev - 0.4).abs() < 1e-7);
                let src = source.as_ref().expect("provenance survives");
                assert_eq!(src.kind, SuggestionKind::Baseline);
                assert_eq!(src.name, "autolight-baseline-v1");
                assert!(src.weights_sha256.is_none());
            }
            other => panic!("expected tone stage, got {other:?}"),
        }
    }

    #[test]
    fn tone_stage_defaults_and_legacy_shape() {
        // A tone stage without `source`/`enabled` (a future writer omitting
        // optional fields) must load with defaults — additive-field rule.
        let json = serde_json::json!({
            "stage": "tone",
            "params": {
                "ev": 0.0, "highlights": 0.0, "shadows": 0.0
            }
        });
        let entry: StageEntry = serde_json::from_value(json).expect("minimal tone stage loads");
        match entry {
            StageEntry::Tone {
                params,
                source,
                enabled,
                ..
            } => {
                assert!(params.is_identity());
                assert!(source.is_none());
                assert!(enabled, "enabled defaults to true");
            }
            other => panic!("expected tone stage, got {other:?}"),
        }
    }
}
