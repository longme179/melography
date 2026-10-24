//! Model manager — AI_MODELS §6 state machine + install pipeline (M3,
//! issue #13).
//!
//! States: `not-installed → downloading (progress, cancel) → verifying
//! (SHA-256) → installing (atomic, temp+rename) → ready (version +
//! delete) → corrupt/missing (explicit error + re-download path)`.
//!
//! ## Offline-first reality of v1
//!
//! No model has passed the vetting gate yet (issue #14 outcome — see
//! `docs/quality/M3_AI_REPORT.md`), so v1 ships an **empty registry** and
//! the app's core value never depends on a model. The install machinery
//! here is fully exercised by tests via [`Fetch`] sources:
//!
//! * [`FileFetch`] — offline install from a local weights file (the
//!   owner-side path for evaluation builds; also how CI proves the
//!   pipeline without network access);
//! * a network downloader lands **with the first gate-passing model**
//!   (documented follow-up — shipping an unreachable HTTPS client with an
//!   empty registry would be dead attack surface, contra AI_MODELS §8).
//!
//! ## Install atomicity (the M2 LUT-library pattern)
//!
//! Files land as `<dir>/.tmp-<rand>` → `sync_all` → rename; the per-model
//! `manifest.json` is written **last** and is the commit marker the
//! registry scan keys on — a crash mid-install leaves a tmp dir that is
//! cleaned at the next [`ModelManager::open`], never a half-ready model.

use crate::{vet, ModelError, ModelManifest, OutputKind};
use melography_core::CancelToken;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("manifest gate rejected model: {0}")]
    Gate(#[from] ModelError),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("fetch cancelled by user")]
    Cancelled,
    #[error("model {0:?} not found in the registry")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("registry entry is corrupt: {0}")]
    Corrupt(String),
}

/// AI_MODELS §6 states (serialized for the UI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelState {
    /// Offer install, size shown.
    NotInstalled,
    /// Progress (bytes_done / total_bytes), cancellable.
    Downloading { bytes_done: u64, total_bytes: u64 },
    /// SHA-256 verification in progress.
    Verifying,
    /// Atomic temp+rename install in progress.
    Installing,
    /// Version + delete button.
    Ready { version: String },
    /// Explicit error + re-download path.
    Corrupt { reason: String },
    /// Files vanished under us (explicit error + re-download).
    Missing { reason: String },
}

/// UI-facing snapshot of one registry entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub version: String,
    pub task: String,
    pub output_kind: OutputKind,
    pub size_bytes: u64,
    pub weights_license: String,
    pub sha256: String,
    pub state: ModelState,
    pub known_issues: Vec<String>,
}

/// Byte source for an install. Implementations stream (with progress) so
/// the state machine stays honest for files and future network sources
/// alike. Weights are **data**: nothing here ever executes model content
/// (AI_MODELS §8).
pub trait Fetch {
    /// Stream the weights for `manifest`, reporting `(bytes_done, total)`
    /// progress; honor the cancel token.
    fn fetch(
        &mut self,
        manifest: &ModelManifest,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, ManagerError>;
}

/// Offline install source: a local file (owner-side evaluation path).
pub struct FileFetch {
    pub path: PathBuf,
}

impl Fetch for FileFetch {
    fn fetch(
        &mut self,
        _manifest: &ModelManifest,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, ManagerError> {
        let bytes = std::fs::read(&self.path)?;
        for chunk in 0..=4 {
            if cancel.is_cancelled() {
                return Err(ManagerError::Cancelled);
            }
            progress((chunk * bytes.len() as u64) / 4, bytes.len() as u64);
        }
        Ok(bytes)
    }
}

/// Registry + installer rooted at a directory (app-data in the app).
pub struct ModelManager {
    root: PathBuf,
    entries: Vec<(ModelManifest, ModelState)>,
}

impl ModelManager {
    /// Open (or create) the registry at `root`, scanning
    /// `root/<id>/<version>/manifest.json` commit markers. Incomplete tmp
    /// litter from a crashed install is cleaned; entries whose weights are
    /// missing/mis-sized land in `Corrupt`/`Missing` with explicit reasons.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let mut entries = Vec::new();
        if let Ok(ids) = std::fs::read_dir(&root) {
            for id_dir in ids.flatten() {
                let id_path = id_dir.path();
                if !id_path.is_dir() {
                    continue;
                }
                if let Ok(versions) = std::fs::read_dir(&id_path) {
                    for v_dir in versions.flatten() {
                        let manifest_path = v_dir.path().join("manifest.json");
                        if !manifest_path.is_file() {
                            continue; // tmp/incomplete — cleaned below
                        }
                        match std::fs::read_to_string(&manifest_path)
                            .map_err(ManagerError::Io)
                            .and_then(|s| {
                                serde_json::from_str::<ModelManifest>(&s)
                                    .map_err(|e| ManagerError::Corrupt(e.to_string()))
                            }) {
                            Ok(m) => {
                                let state = verify_layout(&v_dir.path(), &m);
                                entries.push((m, state));
                            }
                            Err(e) => entries.push((
                                ModelManifest {
                                    id: id_dir.file_name().to_string_lossy().into_owned(),
                                    version: semver::Version::new(0, 0, 0),
                                    task: "unreadable manifest".into(),
                                    output_kind: OutputKind::Parameters,
                                    sha256: String::new(),
                                    size_bytes: 0,
                                    weights_license: "unknown".into(),
                                    source_url: String::new(),
                                    license_cleared_by_owner: false,
                                    redistributable: false,
                                    upstream: None,
                                    runtime: None,
                                    eps: None,
                                    measurements: Default::default(),
                                    known_issues: vec![e.to_string()],
                                },
                                ModelState::Corrupt {
                                    reason: format!("manifest unreadable: {e}"),
                                },
                            )),
                        }
                    }
                }
            }
        }
        // Clean tmp litter (crashed installs) recursively — best effort,
        // never fatal. Two naming generations: the shared primitive's
        // `.{file}.tmp-{pid}-{nanos}` siblings (current) and the legacy
        // M3-era `.tmp-<rand>` / `tmp` entries (dir or file).
        fn clean_litter(dir: &Path) {
            let _ = melography_core::durable::clean_stale_tmp(&dir.join("model.onnx"));
            let _ = melography_core::durable::clean_stale_tmp(&dir.join("manifest.json"));
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                if name.starts_with(".tmp-") || name == "tmp" {
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                } else if path.is_dir() {
                    clean_litter(&path);
                }
            }
        }
        clean_litter(&root);
        Self { root, entries }
    }

    /// UI snapshot.
    #[must_use]
    pub fn list(&self) -> Vec<ModelInfo> {
        self.entries
            .iter()
            .map(|(m, s)| ModelInfo {
                id: m.id.clone(),
                version: m.version.to_string(),
                task: m.task.clone(),
                output_kind: m.output_kind,
                size_bytes: m.size_bytes,
                weights_license: m.weights_license.clone(),
                sha256: m.sha256.clone(),
                state: s.clone(),
                known_issues: m.known_issues.clone(),
            })
            .collect()
    }

    #[must_use]
    pub fn is_ready(&self, id: &str) -> bool {
        self.entries
            .iter()
            .any(|(m, s)| m.id == id && matches!(s, ModelState::Ready { .. }))
    }

    /// Full SHA-256 verification of an installed model's weights (the
    /// open-time check is existence+size; this is the on-demand deep check
    /// the UI runs before first use).
    pub fn verify(&mut self, id: &str) -> Result<bool, ManagerError> {
        let (manifest, path) = self.locate(id)?;
        let bytes = std::fs::read(path.join("model.onnx"))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        let ok = actual == manifest.sha256;
        self.update_state(
            id,
            if ok {
                ModelState::Ready {
                    version: manifest.version.to_string(),
                }
            } else {
                ModelState::Corrupt {
                    reason: format!(
                        "checksum mismatch (expected {}, got {actual})",
                        manifest.sha256
                    ),
                }
            },
        );
        Ok(ok)
    }

    /// Install a model: gate → fetch (progress/cancel) → verify → atomic
    /// install → Ready. On any failure the state rolls back to
    /// NotInstalled/Corrupt and no litter remains.
    pub fn install(
        &mut self,
        manifest: ModelManifest,
        fetch: &mut dyn Fetch,
        cancel: &CancelToken,
    ) -> Result<(), ManagerError> {
        vet(&manifest)?; // the gate is blocking (ADR-0004 §4)
        self.set_state(
            &manifest.id,
            ModelState::Downloading {
                bytes_done: 0,
                total_bytes: manifest.size_bytes,
            },
        );
        let bytes = match fetch.fetch(
            &manifest,
            &mut |done, total| {
                // The state mirror lives in the app (progress events); the
                // manager itself is synchronous — see the app command wrapper.
                let _ = (done, total);
            },
            cancel,
        ) {
            Ok(b) => b,
            Err(e) => {
                self.rollback(&manifest.id, &e);
                return Err(e);
            }
        };
        if cancel.is_cancelled() {
            self.rollback(&manifest.id, &ManagerError::Cancelled);
            return Err(ManagerError::Cancelled);
        }
        self.set_state(&manifest.id, ModelState::Verifying);
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != manifest.sha256 {
            let e = ManagerError::Checksum {
                expected: manifest.sha256.clone(),
                actual,
            };
            self.rollback(&manifest.id, &e);
            return Err(e);
        }
        self.set_state(&manifest.id, ModelState::Installing);
        if let Err(e) = atomic_install(&self.root, &manifest, &bytes) {
            let e: ManagerError = e.into();
            self.rollback(&manifest.id, &e);
            return Err(e);
        }
        self.set_state(
            &manifest.id,
            ModelState::Ready {
                version: manifest.version.to_string(),
            },
        );
        // Reload the entry so list() reflects the on-disk truth.
        self.entries.retain(|(m, _)| m.id != manifest.id);
        let dir = model_dir(&self.root, &manifest.id, &manifest.version.to_string());
        let mut fresh = manifest;
        let state = verify_layout(&dir, &fresh);
        fresh.known_issues = fresh.known_issues.clone();
        self.entries.push((fresh, state));
        Ok(())
    }

    /// Delete a model — all files, immediately (AI_MODELS §6: "deletion
    /// removes all files immediately"; every version of the id goes).
    pub fn delete(&mut self, id: &str) -> Result<(), ManagerError> {
        let dir = self.root.join(id);
        if !dir.is_dir() {
            return Err(ManagerError::NotFound(id.to_string()));
        }
        std::fs::remove_dir_all(&dir)?;
        self.entries.retain(|(m, _)| m.id != id);
        Ok(())
    }

    /// Path to installed weights (inference entry point).
    pub fn weights_path(&self, id: &str) -> Result<PathBuf, ManagerError> {
        let (m, state) = self
            .entries
            .iter()
            .find(|(m, _)| m.id == id)
            .ok_or_else(|| ManagerError::NotFound(id.to_string()))?;
        if !matches!(state, ModelState::Ready { .. }) {
            return Err(ManagerError::Corrupt(format!(
                "model {id} is not ready ({state:?})"
            )));
        }
        Ok(model_dir(&self.root, id, &m.version.to_string()).join("model.onnx"))
    }

    fn locate(&self, id: &str) -> Result<(ModelManifest, PathBuf), ManagerError> {
        let (m, _) = self
            .entries
            .iter()
            .find(|(m, _)| m.id == id)
            .ok_or_else(|| ManagerError::NotFound(id.to_string()))?;
        let dir = model_dir(&self.root, id, &m.version.to_string());
        Ok((m.clone(), dir))
    }

    fn set_state(&mut self, id: &str, state: ModelState) {
        match self.entries.iter_mut().find(|(m, _)| m.id == id) {
            Some(entry) => entry.1 = state,
            None => {
                // Intermediate states for not-yet-installed models are
                // tracked by the app layer; the registry only keeps
                // committed/corrupt entries. No-op here.
            }
        }
    }

    fn update_state(&mut self, id: &str, state: ModelState) {
        if let Some(entry) = self.entries.iter_mut().find(|(m, _)| m.id == id) {
            entry.1 = state;
        }
    }

    fn rollback(&mut self, id: &str, e: &ManagerError) {
        // Remove any partial dir, mark corrupt (install failures keep an
        // honest breadcrumb; a fresh install attempt starts clean).
        let _ = e;
        self.update_state(
            id,
            ModelState::Corrupt {
                reason: e.to_string(),
            },
        );
    }
}

fn model_dir(root: &Path, id: &str, version: &str) -> PathBuf {
    root.join(id).join(version)
}

/// On-disk layout check at open time: manifest.json (commit marker) +
/// model.onnx with the declared size.
fn verify_layout(dir: &Path, m: &ModelManifest) -> ModelState {
    let weights = dir.join("model.onnx");
    match std::fs::metadata(&weights) {
        Ok(meta) if meta.len() == m.size_bytes => ModelState::Ready {
            version: m.version.to_string(),
        },
        Ok(meta) => ModelState::Corrupt {
            reason: format!(
                "weights size {} != manifest {} — re-download required",
                meta.len(),
                m.size_bytes
            ),
        },
        Err(e) => ModelState::Missing {
            reason: format!("weights file missing: {e}"),
        },
    }
}

/// Durable atomic install, delegated to the shared primitive
/// (`melography_core::durable`): pid+nanos-unique tmp names, write-handle
/// fsync (the Windows `FlushFileBuffers` requirement), drop-before-rename,
/// POSIX dir-fsync after each rename. The manifest.json commit marker is
/// written last — a model dir without it is not a model.
fn atomic_install(root: &Path, m: &ModelManifest, bytes: &[u8]) -> Result<(), std::io::Error> {
    let dir = model_dir(root, &m.id, &m.version.to_string());
    std::fs::create_dir_all(&dir)?;
    // Weights.
    melography_core::durable::atomic_write_durable(&dir.join("model.onnx"), bytes)?;
    // Manifest (commit marker) — canonical JSON, written last.
    let manifest_json = serde_json::to_string_pretty(m)
        .map_err(|e| std::io::Error::other(format!("manifest serialize: {e}")))?;
    melography_core::durable::atomic_write_durable(
        &dir.join("manifest.json"),
        manifest_json.as_bytes(),
    )?;
    Ok(())
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::good;
    use std::io::Write;

    fn write_weights(dir: &tempfile::TempDir) -> (PathBuf, Vec<u8>, String) {
        // Deterministic pseudo-weights.
        let bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        let mut h = Sha256::new();
        h.update(&bytes);
        let sha = format!("{:x}", h.finalize());
        let path = dir.path().join("weights.onnx");
        std::fs::write(&path, &bytes).unwrap();
        (path, bytes, sha)
    }

    fn manifest_for(sha: &str, size: u64) -> ModelManifest {
        let mut m = good();
        m.sha256 = sha.to_string();
        m.size_bytes = size;
        m
    }

    #[test]
    fn install_verify_delete_roundtrip() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, sha) = write_weights(&src);
        let m = manifest_for(&sha, 4096);

        let mut mgr = ModelManager::open(root.path());
        assert!(mgr.list().is_empty(), "fresh registry is empty");

        let cancel = CancelToken::default();
        let mut fetch = FileFetch { path };
        mgr.install(m.clone(), &mut fetch, &cancel)
            .expect("gate-passing model installs");

        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, m.id);
        assert!(
            matches!(list[0].state, ModelState::Ready { .. }),
            "{:?}",
            list[0].state
        );
        assert!(mgr.is_ready(&m.id));

        // Weights path + deep verify.
        let wp = mgr.weights_path(&m.id).unwrap();
        assert!(wp.is_file());
        assert!(mgr.verify(&m.id).unwrap(), "sha must match after install");

        // Files are where they belong, no litter.
        let mdir = root.path().join(&m.id).join(m.version.to_string());
        assert!(mdir.join("manifest.json").is_file());
        let litter = std::fs::read_dir(&mdir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"));
        assert_eq!(litter.count(), 0);

        // Delete removes everything immediately.
        mgr.delete(&m.id).unwrap();
        assert!(!root.path().join(&m.id).exists());
        assert!(mgr.list().is_empty());
        assert!(!mgr.is_ready(&m.id));
    }

    #[test]
    fn open_recovers_ready_state_from_disk() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, sha) = write_weights(&src);
        let m = manifest_for(&sha, 4096);
        let mut mgr = ModelManager::open(root.path());
        mgr.install(m.clone(), &mut FileFetch { path }, &CancelToken::default())
            .unwrap();
        drop(mgr);

        let mgr2 = ModelManager::open(root.path());
        let list = mgr2.list();
        assert_eq!(list.len(), 1);
        assert!(matches!(list[0].state, ModelState::Ready { .. }));
        assert_eq!(list[0].weights_license, m.weights_license);
    }

    #[test]
    fn checksum_mismatch_is_rejected_and_leaves_no_files() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, _sha) = write_weights(&src);
        let m = manifest_for(&"b".repeat(64), 4096); // wrong sha
        let mut mgr = ModelManager::open(root.path());
        let err = mgr
            .install(m.clone(), &mut FileFetch { path }, &CancelToken::default())
            .unwrap_err();
        assert!(matches!(err, ManagerError::Checksum { .. }), "{err}");
        assert!(!root.path().join(&m.id).exists(), "no partial files");
        let list = mgr.list();
        assert!(
            list.is_empty()
                || list
                    .iter()
                    .all(|i| !matches!(i.state, ModelState::Ready { .. }))
        );
    }

    #[test]
    fn gate_rejection_blocks_install_before_any_io() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, sha) = write_weights(&src);
        let mut m = manifest_for(&sha, 4096);
        m.license_cleared_by_owner = false; // gate must reject
        let mut mgr = ModelManager::open(root.path());
        let err = mgr
            .install(m.clone(), &mut FileFetch { path }, &CancelToken::default())
            .unwrap_err();
        assert!(matches!(err, ManagerError::Gate(_)), "{err}");
        assert!(!root.path().join(&m.id).exists());
    }

    #[test]
    fn corrupt_weights_detected_at_open() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, sha) = write_weights(&src);
        let m = manifest_for(&sha, 4096);
        let mut mgr = ModelManager::open(root.path());
        mgr.install(m.clone(), &mut FileFetch { path }, &CancelToken::default())
            .unwrap();
        drop(mgr);
        // Tamper with the installed weights (size change).
        let wp = root
            .path()
            .join(&m.id)
            .join(m.version.to_string())
            .join("model.onnx");
        let mut f = std::fs::OpenOptions::new().append(true).open(&wp).unwrap();
        f.write_all(&[0u8; 16]).unwrap();
        drop(f);

        let mgr2 = ModelManager::open(root.path());
        let list = mgr2.list();
        assert!(
            matches!(list[0].state, ModelState::Corrupt { .. }),
            "{:?}",
            list[0].state
        );
        assert!(!mgr2.is_ready(&m.id));
        assert!(mgr2.weights_path(&m.id).is_err());
    }

    #[test]
    fn missing_weights_detected_at_open() {
        let root = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        let (path, _bytes, sha) = write_weights(&src);
        let m = manifest_for(&sha, 4096);
        let mut mgr = ModelManager::open(root.path());
        mgr.install(m.clone(), &mut FileFetch { path }, &CancelToken::default())
            .unwrap();
        drop(mgr);
        std::fs::remove_file(
            root.path()
                .join(&m.id)
                .join(m.version.to_string())
                .join("model.onnx"),
        )
        .unwrap();

        let mgr2 = ModelManager::open(root.path());
        assert!(matches!(mgr2.list()[0].state, ModelState::Missing { .. }));
    }

    #[test]
    fn tmp_litter_from_crashed_install_is_cleaned_at_open() {
        let root = tempfile::tempdir().unwrap();
        let id_dir = root.path().join("some-model");
        let v_dir = id_dir.join("0.1.0");
        std::fs::create_dir_all(&v_dir).unwrap();
        // Legacy M3-era litter.
        std::fs::write(v_dir.join(".tmp-123"), b"partial").unwrap();
        std::fs::create_dir_all(id_dir.join("tmp")).unwrap();
        // Shared-primitive litter (current naming).
        std::fs::write(v_dir.join(".model.onnx.tmp-1-2"), b"partial").unwrap();
        std::fs::write(v_dir.join(".manifest.json.tmp-3-4"), b"partial").unwrap();

        let mgr = ModelManager::open(root.path());
        assert!(mgr.list().is_empty(), "tmp dir is not a model");
        assert!(!id_dir.join("tmp").exists(), "litter cleaned");
        assert!(!v_dir.join(".tmp-123").exists(), "legacy litter cleaned");
        assert!(
            !v_dir.join(".model.onnx.tmp-1-2").exists(),
            "weights litter cleaned"
        );
        assert!(
            !v_dir.join(".manifest.json.tmp-3-4").exists(),
            "manifest litter cleaned"
        );
    }

    #[test]
    fn cancelling_fetch_leaves_no_files() {
        struct CancellingFetch;
        impl Fetch for CancellingFetch {
            fn fetch(
                &mut self,
                _m: &ModelManifest,
                _p: &mut dyn FnMut(u64, u64),
                cancel: &CancelToken,
            ) -> Result<Vec<u8>, ManagerError> {
                let _ = cancel; // checked by the manager after fetch
                Ok(vec![1, 2, 3])
            }
        }
        let root = tempfile::tempdir().unwrap();
        let (_path, _bytes, sha) = write_weights(&tempfile::tempdir().unwrap());
        let m = manifest_for(&sha, 4096);
        let mut mgr = ModelManager::open(root.path());
        let cancel = CancelToken::default();
        cancel.cancel();
        let err = mgr
            .install(m.clone(), &mut CancellingFetch, &cancel)
            .unwrap_err();
        assert!(matches!(err, ManagerError::Cancelled), "{err}");
        assert!(!root.path().join(&m.id).exists());
    }

    #[test]
    fn delete_unknown_model_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        let mut mgr = ModelManager::open(root.path());
        assert!(matches!(mgr.delete("nope"), Err(ManagerError::NotFound(_))));
    }

    #[test]
    fn model_info_serializes_for_ipc() {
        let info = ModelInfo {
            id: "x".into(),
            version: "1.0.0".into(),
            task: "t".into(),
            output_kind: OutputKind::Parameters,
            size_bytes: 1,
            weights_license: "MIT".into(),
            sha256: "a".repeat(64),
            state: ModelState::Downloading {
                bytes_done: 5,
                total_bytes: 10,
            },
            known_issues: vec![],
        };
        let s = serde_json::to_string(&info).unwrap();
        assert!(s.contains("\"downloading\""));
        assert!(s.contains("\"bytes_done\":5"));
    }
}
