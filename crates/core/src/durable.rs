//! Durable atomic writes — the single battle-tested primitive every
//! MELOGRAPHY persistence point uses (LUT blobs + index, model weights +
//! manifests, recipe sidecars, session/autosave files).
//!
//! Contract (safe to kill the process at ANY point):
//! * The file at `dest` is always either the complete previous content or
//!   the complete new content — never torn. Writes go to a uniquely-named
//!   temp sibling, are fsynced, then renamed over the target.
//! * The fsync runs on the **write-mode handle** (`File::create` →
//!   `write_all` → `sync_all` → `drop` → `rename`). Two Windows traps this
//!   ordering avoids: (a) `FlushFileBuffers` on a read-only handle fails
//!   with `ERROR_ACCESS_DENIED` — the old `presets::atomic_write` silently
//!   ignored that failure and shipped without any real sync on Windows,
//!   and the M3 model manager turned the same mistake into CI failures;
//!   (b) renaming a file whose handle is still open trips the Windows
//!   sharing violation — hence the explicit `drop` before `rename`.
//! * POSIX additionally fsyncs the parent directory after the rename so
//!   the rename itself survives an OS crash. On Windows a directory
//!   cannot be opened via `std::fs::File::open` at all (`CreateFileW`
//!   needs `FILE_FLAG_BACKUP_SEMANTICS`, not expressible through std) and
//!   NTFS journals the metadata ordering itself — an honest no-op there,
//!   the same durability model the M2 LUT library shipped green on
//!   windows-latest.
//! * A crashed writer can leave `.{name}.tmp-{pid}-{nanos}` litter; it is
//!   never ambiguous with real data, and [`clean_stale_tmp`] removes it
//!   at open time (recovery contract: litter is cleaned, published files
//!   are untouched).
//!
//! Kill-safety is enforced by a real battery: `tests/durable_kill.rs`
//! spawns this test binary as a child that loops writes + read-back
//! verification, SIGKILLs it at randomized points, and re-opens the
//! files — the PLAN M4 "kill during save" exit criterion, run on every
//! OS in CI.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Parent directory that works for bare filenames (`"x.json".parent()` is
/// `Some("")`, which does not resolve as a directory).
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// Collision-free temp sibling: `.{file}.tmp-{pid}-{nanos}`. Two writers
/// targeting the same dest (concurrent saves, a crashed writer's leftover)
/// never share a temp name — the pid separates processes, the nanos
/// separate threads/retries within one.
fn tmp_sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("melography");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    parent_dir(dest).join(format!(".{name}.tmp-{}-{nanos}", std::process::id()))
}

/// Atomically replace `dest` with `bytes`, durably (see module docs).
pub fn atomic_write_durable(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = tmp_sibling(dest);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    } // handle dropped here — before the rename (Windows sharing violation)
    match std::fs::rename(&tmp, dest) {
        Ok(()) => {}
        Err(e) => {
            // Best-effort litter removal; the failure being reported is
            // the rename's, and the tmp file is recoverable litter either
            // way (clean_stale_tmp at open time).
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    sync_dir(parent_dir(dest))
}

/// Directory fsync — makes a just-completed rename durable on POSIX.
/// No-op on Windows (see module docs).
#[cfg(not(windows))]
pub fn sync_dir(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

/// Directory fsync — POSIX-only concept; honest no-op on Windows
/// (see module docs).
#[cfg(windows)]
pub fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Remove stale `.{file_name}.tmp-*` siblings of `dest` left behind by
/// crashed writers (recovery contract). Returns how many were removed.
/// Individual removal failures are skipped, not fatal: a wedged temp file
/// must not block opening the user's image — the count lets callers log
/// honestly. Never touches `dest` itself or any other real file.
pub fn clean_stale_tmp(dest: &Path) -> io::Result<usize> {
    let Some(name) = dest.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Ok(0);
    };
    let prefix = format!(".{name}.tmp-");
    let dir = parent_dir(dest);
    let mut removed = 0usize;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // No directory (yet) → nothing to clean. Not an error: callers
        // run this speculatively at open time.
        Err(_) => return Ok(0),
    };
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let Some(fname) = fname.to_str() else {
            continue;
        };
        if fname.starts_with(&prefix) && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_roundtrip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.melography.json");
        atomic_write_durable(&p, b"{\"v\":1}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"v\":1}");
        atomic_write_durable(&p, b"{\"v\":2}").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"{\"v\":2}");
    }

    #[test]
    fn no_litter_after_successful_writes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.melography.json");
        for i in 0..5 {
            atomic_write_durable(&p, format!("{{\"v\":{i}}}").as_bytes()).unwrap();
        }
        let litter: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".b.melography.json.tmp-")
            })
            .collect();
        assert!(litter.is_empty(), "tmp files must not linger: {litter:?}");
    }

    #[test]
    fn clean_stale_tmp_removes_only_matching_litter() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("c.melography.json");
        atomic_write_durable(&dest, b"real").unwrap();
        let litter1 = dir.path().join(".c.melography.json.tmp-123-456");
        let litter2 = dir.path().join(".c.melography.json.tmp-999-1");
        let other = dir.path().join(".d.melography.json.tmp-123-456");
        let keep = dir.path().join("c.melography.json.bak");
        std::fs::write(&litter1, b"partial").unwrap();
        std::fs::write(&litter2, b"partial").unwrap();
        std::fs::write(&other, b"partial").unwrap();
        std::fs::write(&keep, b"keep").unwrap();

        let removed = clean_stale_tmp(&dest).unwrap();
        assert_eq!(removed, 2, "exactly the two matching litter files");
        assert!(!litter1.exists() && !litter2.exists());
        assert!(other.exists(), "other files' litter is not ours to touch");
        assert!(keep.exists(), "real sibling files are never touched");
        assert_eq!(std::fs::read(&dest).unwrap(), b"real");
        assert_eq!(clean_stale_tmp(&dest).unwrap(), 0, "second run is a no-op");
    }

    #[test]
    fn missing_parent_is_a_clear_error() {
        let e = atomic_write_durable(Path::new("/nonexistent-dir-xyz/a.json"), b"{}").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }
}
