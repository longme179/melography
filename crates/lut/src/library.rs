//! Content-addressed LUT library (luts.md §4, ARCHITECTURE §7, issue #9).
//!
//! **Storage model:** every imported LUT is copied into
//! `<root>/luts/<sha256>.cube` — the *content address* is the file name —
//! and indexed in `<root>/luts/index.json` with provenance metadata
//! (original filename, optional source URL / license note, declared
//! encodings, parse-derived facts). Human-readable names are *metadata
//! only*; recipes reference the hash, so renaming and moving libraries
//! never breaks a sidecar.
//!
//! **Integrity:** the stored bytes' SHA-256 is re-verified on every load —
//! a corrupted/mismatched file is an explicit [`LutError::Missing` /
//! `ChecksumMismatch`] error state (UI: "locate the file" flow), never a
//! silent skip (luts.md §4).
//!
//! **Atomicity:** the index is written temp+rename (same discipline as
//! sidecars — ARCHITECTURE §7); a crash mid-import leaves the previous
//! index intact and at worst one orphaned hash-named file (harmless,
//! self-healing on next import of the same content).

use crate::cube::{self, CubeError, CubeKind, CubeLut};
use crate::encoding::LutEncoding;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Library record for one imported LUT (index.json entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutRecord {
    /// SHA-256 of the stored file bytes (content address + integrity check).
    pub content_hash: String,
    /// File name at import time (provenance; display only).
    pub original_filename: String,
    /// `TITLE` from the file, if present.
    pub title: Option<String>,
    pub kind: CubeKind,
    /// Grid size (3D side / 1D rows).
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// Declared encodings (import dialog; luts.md §3).
    pub declared_input: LutEncoding,
    pub declared_output: LutEncoding,
    /// Free-form provenance (optional, user-entered).
    pub source_url: Option<String>,
    pub license_note: Option<String>,
    /// UTC import timestamp (RFC 3339).
    pub imported_utc: String,
    /// Parse warnings preserved from import (CubeWarning list).
    pub warnings: Vec<cube::CubeWarning>,
}

#[must_use]
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[must_use]
fn utc_now_rfc3339() -> String {
    // No chrono dependency: days-from-epoch → Y-M-D civil conversion
    // (Howard Hinnant's algorithm, public domain) + hh:mm::ss from secs.
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

/// Errors from library operations (mapped into ARCHITECTURE §8 `LutError`
/// classes; kept concrete here for the UI layer).
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("LUT {hash} is not in the library (missing or removed)")]
    Missing { hash: String },
    #[error(
        "stored LUT {hash} failed integrity check (expected {expected}, found {found}) — \
         the file changed on disk; re-import the original"
    )]
    ChecksumMismatch {
        hash: String,
        expected: String,
        found: String,
    },
    #[error("stored LUT {hash} no longer parses: {source}")]
    Corrupt { hash: String, source: CubeError },
    #[error("I/O error: {0}")]
    Io(String),
    #[error("index schema error: {0}")]
    IndexSchema(String),
}

impl From<LibraryError> for crate::LutError {
    fn from(e: LibraryError) -> Self {
        match &e {
            LibraryError::Missing { hash } => crate::LutError::Missing { hash: hash.clone() },
            LibraryError::ChecksumMismatch { hash, .. } => {
                crate::LutError::ChecksumMismatch { hash: hash.clone() }
            }
            LibraryError::Corrupt { hash, source } => crate::LutError::Malformed {
                detail: format!("stored LUT {hash} no longer parses: {source}"),
            },
            LibraryError::Io(detail) => crate::LutError::LibraryIo {
                detail: detail.clone(),
            },
            LibraryError::IndexSchema(detail) => crate::LutError::LibraryIo {
                detail: format!("index: {detail}"),
            },
        }
    }
}

/// The on-disk index (versioned; unknown fields preserved inert).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryIndex {
    pub schema: String,
    pub version: u32,
    /// content_hash → record (BTreeMap: deterministic serialization order —
    /// cross-platform byte-stable index files, luts.md test matrix).
    pub luts: BTreeMap<String, LutRecord>,
    /// Unknown fields from newer schema versions — kept inert, re-serialized
    /// (deny-by-default; ARCHITECTURE §7).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self {
            schema: "melography.lut-library".to_string(),
            version: 1,
            luts: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

/// A LUT library rooted at a user-data directory (`<root>/luts/…`).
pub struct LutLibrary {
    root: PathBuf,
}

/// Write-then-flush-then-rename: the flush (`sync_all`) guarantees the
/// data is on disk *before* the rename makes it visible, so an immediate
/// read-back (verify-at-load) never races an unflushed file — the Windows
/// CI flake this fixed hashed an empty blob (run 40077447188).
fn atomic_write_durable(tmp: &Path, bytes: &[u8], dest: &Path) -> Result<(), LibraryError> {
    use std::io::Write;
    let mut f = std::fs::File::create(tmp)
        .map_err(|e| LibraryError::Io(format!("create {}: {e}", tmp.display())))?;
    f.write_all(bytes)
        .map_err(|e| LibraryError::Io(format!("write {}: {e}", tmp.display())))?;
    f.sync_all()
        .map_err(|e| LibraryError::Io(format!("sync {}: {e}", tmp.display())))?;
    drop(f);
    std::fs::rename(tmp, dest)
        .map_err(|e| LibraryError::Io(format!("rename → {}: {e}", dest.display())))
}

impl LutLibrary {
    /// Open (or lazily create) a library under `root`. Does no I/O until an
    /// operation needs it (empty index is created on first import/save).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The library root directory (app layers display/configure it).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn luts_dir(&self) -> PathBuf {
        self.root.join("luts")
    }
    fn index_path(&self) -> PathBuf {
        self.luts_dir().join("index.json")
    }
    fn blob_path(&self, hash: &str) -> PathBuf {
        self.luts_dir().join(format!("{hash}.cube"))
    }

    /// Load the index (empty if absent). Corrupt index = explicit error
    /// (never silently reset — the user's provenance data is precious).
    pub fn load_index(&self) -> Result<LibraryIndex, LibraryError> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(LibraryIndex::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| LibraryError::Io(format!("read {}: {e}", path.display())))?;
        serde_json::from_str(&text).map_err(|e| LibraryError::IndexSchema(e.to_string()))
    }

    /// Atomic index write (temp + rename, ARCHITECTURE §7).
    pub fn save_index(&self, index: &LibraryIndex) -> Result<(), LibraryError> {
        let dir = self.luts_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| LibraryError::Io(format!("mkdir {}: {e}", dir.display())))?;
        let json = serde_json::to_string_pretty(index)
            .map_err(|e| LibraryError::IndexSchema(e.to_string()))?;
        let tmp = dir.join(".index.json.tmp");
        atomic_write_durable(&tmp, json.as_bytes(), &self.index_path())?;
        Ok(())
    }

    /// Import (or re-import) a `.cube` file: validate strictly, copy under
    /// the content address, upsert the record. Returns the record.
    /// Re-import of identical content is idempotent (metadata refreshed).
    pub fn import(
        &self,
        path: &Path,
        declared_input: LutEncoding,
        declared_output: LutEncoding,
        provenance: ImportProvenance,
    ) -> Result<LutRecord, LibraryError> {
        let bytes = std::fs::read(path)
            .map_err(|e| LibraryError::Io(format!("read {}: {e}", path.display())))?;
        let hash = sha256_hex(&bytes);
        let src = String::from_utf8_lossy(&bytes);
        let lut = cube::parse_cube(&src).map_err(|e| LibraryError::Corrupt {
            hash: hash.clone(),
            source: e,
        })?;

        let dir = self.luts_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| LibraryError::Io(format!("mkdir {}: {e}", dir.display())))?;
        let blob = self.blob_path(&hash);
        // Write only if absent or bytes differ (idempotent re-import).
        let need_write = match std::fs::read(&blob) {
            Ok(existing) => existing != bytes,
            Err(_) => true,
        };
        if need_write {
            let tmp = dir.join(format!(".{hash}.tmp"));
            atomic_write_durable(&tmp, &bytes, &blob)?;
        }

        let record = LutRecord {
            content_hash: hash,
            original_filename: path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            title: lut.title.clone(),
            kind: lut.kind,
            size: lut.size,
            domain_min: lut.domain_min,
            domain_max: lut.domain_max,
            declared_input,
            declared_output,
            source_url: provenance.source_url,
            license_note: provenance.license_note,
            imported_utc: utc_now_rfc3339(),
            warnings: lut.warnings.clone(),
        };
        let mut index = self.load_index()?;
        index
            .luts
            .insert(record.content_hash.clone(), record.clone());
        self.save_index(&index)?;
        Ok(record)
    }

    /// Load + integrity-check + parse the LUT for `hash`. This is the only
    /// path the render pipeline may use to obtain [`CubeLut`]s (luts.md §4:
    /// checksum verified at load; missing = explicit error).
    pub fn load(&self, hash: &str) -> Result<CubeLut, LibraryError> {
        let blob = self.blob_path(hash);
        let bytes = std::fs::read(&blob).map_err(|_| LibraryError::Missing {
            hash: hash.to_string(),
        })?;
        let found = sha256_hex(&bytes);
        if found != hash {
            return Err(LibraryError::ChecksumMismatch {
                hash: hash.to_string(),
                expected: hash.to_string(),
                found,
            });
        }
        let src = String::from_utf8_lossy(&bytes);
        cube::parse_cube(&src).map_err(|e| LibraryError::Corrupt {
            hash: hash.to_string(),
            source: e,
        })
    }

    /// Remove a LUT (index entry + blob). Recipes still referencing the
    /// hash surface the explicit Missing error state at load (by design).
    pub fn remove(&self, hash: &str) -> Result<(), LibraryError> {
        let mut index = self.load_index()?;
        if index.luts.remove(hash).is_none() {
            return Err(LibraryError::Missing {
                hash: hash.to_string(),
            });
        }
        self.save_index(&index)?;
        let _ = std::fs::remove_file(self.blob_path(hash)); // best effort
        Ok(())
    }

    /// List records sorted by original filename (stable UI ordering).
    pub fn list(&self) -> Result<Vec<LutRecord>, LibraryError> {
        let mut records: Vec<LutRecord> = self.load_index()?.luts.into_values().collect();
        records.sort_by(|a, b| a.original_filename.cmp(&b.original_filename));
        Ok(records)
    }
}

/// Optional provenance fields supplied by the import dialog (D1, UX_SPEC).
#[derive(Debug, Clone, Default)]
pub struct ImportProvenance {
    pub source_url: Option<String>,
    pub license_note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_cube_2d() -> String {
        let mut s = String::from("TITLE \"test identity\"\nLUT_3D_SIZE 2\n");
        for k in 0..2 {
            for j in 0..2 {
                for i in 0..2 {
                    s.push_str(&format!("{} {} {}\n", i as f32, j as f32, k as f32));
                }
            }
        }
        s
    }

    /// Unique, panic-safe temp dir. Returns the guard (keep it alive for
    /// the test body; cleanup is automatic on drop — even on panic) plus a
    /// PathBuf so existing `dir.join(...)` call sites stay unchanged.
    /// Replaces the hand-rolled nanos-clock naming that collided on macOS
    /// (µs clock granularity → shared dir → rename ENOENT flake,
    /// PR #22 run 40086829632).
    fn tmpdir() -> (tempfile::TempDir, PathBuf) {
        let guard = tempfile::Builder::new()
            .prefix("melography-lut-test-")
            .tempdir()
            .unwrap();
        let path = guard.path().to_path_buf();
        (guard, path)
    }

    #[test]
    fn import_load_verify_roundtrip() {
        let (_td, dir) = tmpdir();
        let src = dir.join("my_lut.cube");
        std::fs::write(&src, identity_cube_2d()).unwrap();

        let lib = LutLibrary::open(&dir);
        let rec = lib
            .import(
                &src,
                LutEncoding::SrgbDisplay,
                LutEncoding::SrgbDisplay,
                ImportProvenance::default(),
            )
            .unwrap();
        assert_eq!(rec.size, 2);
        assert_eq!(rec.title.as_deref(), Some("test identity"));

        // Content-addressed blob exists under the hash name.
        assert!(lib.blob_path(&rec.content_hash).exists());

        // Load re-verifies and parses.
        let lut = lib.load(&rec.content_hash).unwrap();
        assert_eq!(lut.size, 2);

        // Index lists it; deterministic serialization.
        let list = lib.list().unwrap();
        assert_eq!(list.len(), 1);
        let a = serde_json::to_string(&lib.load_index().unwrap()).unwrap();
        let b = serde_json::to_string(&lib.load_index().unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn missing_and_corrupt_error_states() {
        let (_td, dir) = tmpdir();
        let lib = LutLibrary::open(&dir);
        let err = lib.load("deadbeef").unwrap_err();
        assert!(matches!(err, LibraryError::Missing { .. }));

        // Corrupt the blob after import → ChecksumMismatch, never silent.
        let src = dir.join("l.cube");
        std::fs::write(&src, identity_cube_2d()).unwrap();
        let rec = lib
            .import(
                &src,
                LutEncoding::SrgbDisplay,
                LutEncoding::SrgbDisplay,
                Default::default(),
            )
            .unwrap();
        std::fs::write(lib.blob_path(&rec.content_hash), "tampered").unwrap();
        let err = lib.load(&rec.content_hash).unwrap_err();
        assert!(matches!(err, LibraryError::ChecksumMismatch { .. }));
    }

    #[test]
    fn utc_format_sanity() {
        let s = utc_now_rfc3339();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[10], b'T');
    }
}
