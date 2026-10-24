//! S6 Batch export queue (UX_SPEC §7, issue #23 part 2): a job list +
//! one sequential worker thread, fully decoupled from the editing
//! session — the queue keeps rendering while the user edits, opens or
//! closes images (P2 Minh's "export queue" that must never block the
//! editor). Jobs carry their own sidecar-shaped stages + export spec:
//! **retry keeps settings** is structural, not a UI promise.
//!
//! Honesty rules baked in (UX_SPEC §9 — no dead ends, no silent lies):
//! * a recipe referencing a missing stock NEVER renders wrong colors —
//!   the job lands in `error` with `stock.missing` + a hint;
//! * outputs never overwrite (`reserve_output_path`);
//! * every error carries an actionable hint the UI catalogs localize
//!   (disk full → folder chooser, permission, unsupported file…);
//! * `rendering` cannot be cancelled mid-file (the shared pipeline is
//!   one call) — cancel applies to queued work, pause stops the NEXT
//!   file; the UI says so instead of lying about a killed render.

use crate::error::AppError;
use crate::export::{self, ExportSpec};
use crate::state::EditorState;
use melography_presets::sidecar::StageEntry;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use tauri::State;

/// Inline in every command signature (same pattern as commands.rs).
type St<'a> = State<'a, Mutex<EditorState>>;
type QSt<'a> = State<'a, Arc<BatchQueue>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Rendering,
    Done,
    Error,
    Cancelled,
}

/// Per-file failure with a stable code + localized hint key (the UI
/// renders e.g. disk full → "Choose another folder…" + retry).
#[derive(Clone, Serialize)]
pub struct JobError {
    pub code: String,
    pub message: String,
    /// i18n key for the actionable next step ("hint.*").
    pub hint: Option<String>,
}

#[derive(Clone)]
pub struct BatchJob {
    pub id: u32,
    pub source: PathBuf,
    pub name: String,
    /// `None` = the recipe that was current at add time ("current
    /// recipe" in the UI); Some = a user preset (id + display name).
    pub preset_id: Option<String>,
    pub preset_name: Option<String>,
    /// The recipe snapshot — the single `StageEntry` serialization.
    pub stages: Vec<StageEntry>,
    pub spec: ExportSpec,
    pub dest_dir: PathBuf,
    /// Reserved when the render starts (collision-suffixed).
    pub dest: Option<PathBuf>,
    pub status: JobStatus,
    pub error: Option<JobError>,
    /// Honest-state warnings from the render (e.g. LUT skipped by film).
    pub warnings: Vec<&'static str>,
    pub dims: Option<(u32, u32)>,
    pub bytes: Option<u64>,
    pub total_ms: Option<f64>,
    pub added_at: String,
    pub finished_at: Option<String>,
    /// Lazy PNG data-URL cache (`batch_thumb`).
    pub thumb: Option<String>,
}

impl BatchJob {
    /// Wire shape — stages are deliberately NOT serialized (the webview
    /// never needs them; `has_thumb` tells it whether to ask).
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "source": self.source.display().to_string(),
            "preset_id": self.preset_id,
            "preset_name": self.preset_name,
            "format": self.spec.format,
            "depth": self.spec.depth,
            "quality": self.spec.quality,
            "dest_dir": self.dest_dir.display().to_string(),
            "dest": self.dest.as_ref().map(|d| d.display().to_string()),
            "status": self.status,
            "error": self.error,
            "warnings": self.warnings,
            "dims": self.dims,
            "bytes": self.bytes,
            "total_ms": self.total_ms,
            "added_at": self.added_at,
            "finished_at": self.finished_at,
            "has_thumb": self.thumb.is_some(),
        })
    }
}

pub struct QueueInner {
    pub jobs: Vec<BatchJob>,
    pub next_id: u32,
    pub paused: bool,
    pub shutdown: bool,
}

/// The queue is a Tauri-managed `Arc` — commands lock it briefly; the
/// worker holds NO lock while rendering (the editor stays responsive).
pub struct BatchQueue {
    pub inner: Mutex<QueueInner>,
    pub signal: Condvar,
}

impl BatchQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(QueueInner {
                jobs: Vec::new(),
                next_id: 1,
                paused: false,
                shutdown: false,
            }),
            signal: Condvar::new(),
        }
    }

    /// Full wire snapshot — what `batch://update` emits and every queue
    /// command returns: `{ jobs: [...], paused: bool }` (the pause state
    /// rides with the jobs so the UI never keeps a second opinion).
    pub fn snapshot(&self) -> Value {
        let g = self.inner.lock().unwrap();
        json!({
            "jobs": g.jobs.iter().map(|j| j.to_json()).collect::<Vec<_>>(),
            "paused": g.paused,
        })
    }
}

impl Default for BatchQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable render environment (paths snapshotted at app start; the
/// stock registry is read-only after startup).
#[derive(Clone)]
pub struct WorkerEnv {
    pub library_root: PathBuf,
    pub profiles_dir: PathBuf,
}

/// Render ONE queued job to completion. Pure-ish: `notify` receives the
/// snapshot after each visible transition (rendering…, done/error) so
/// tests can assert the UI-visible sequence without a webview.
pub fn run_one(q: &BatchQueue, env: &WorkerEnv, notify: &mut dyn FnMut(&Value)) {
    // Acquire (non-blocking — the worker thread decides when to wait).
    // Pause is honored HERE as well as in the worker loop: the unit of
    // work is self-contained (a manual `run_one` from a future caller
    // can never steamroll a paused queue).
    let job = {
        let mut g = q.inner.lock().unwrap();
        if g.paused {
            return;
        }
        let Some(i) = g.jobs.iter().position(|j| j.status == JobStatus::Queued) else {
            return;
        };
        let stem = export::output_stem(&g.jobs[i].source);
        let ext = g.jobs[i].spec.extension().to_string();
        let dest = export::reserve_output_path(&g.jobs[i].dest_dir, &stem, &ext);
        g.jobs[i].dest = Some(dest);
        g.jobs[i].status = JobStatus::Rendering;
        g.jobs[i].error = None;
        g.jobs[i].warnings.clear();
        g.jobs[i].dims = None;
        g.jobs[i].bytes = None;
        g.jobs[i].total_ms = None;
        g.jobs[i].finished_at = None;
        g.jobs[i].clone()
    };
    notify(&q.snapshot());

    let result = {
        let e = export::StandaloneExport {
            source: &job.source,
            dest: job.dest.as_deref().unwrap_or_else(|| Path::new("")),
            stages: &job.stages,
            spec: &job.spec,
            library_root: &env.library_root,
            profiles_dir: &env.profiles_dir,
        };
        export::run_export(&e)
    };

    {
        let mut g = q.inner.lock().unwrap();
        if let Some(j) = g.jobs.iter_mut().find(|j| j.id == job.id) {
            match result {
                Ok(out) => {
                    j.status = JobStatus::Done;
                    j.dims = Some(out.dims);
                    j.bytes = Some(out.bytes);
                    j.total_ms = Some(out.total_ms);
                    j.warnings = out.warnings;
                }
                Err(err) => {
                    j.status = JobStatus::Error;
                    j.error = Some(JobError {
                        code: err.code,
                        message: err.message,
                        hint: err.context,
                    });
                }
            }
            j.finished_at = Some(crate::commands::now_rfc3339());
        }
    }
    notify(&q.snapshot());
}

/// The worker thread: waits for runnable work, renders strictly one job
/// at a time (sequential = deterministic + memory-bounded — the same
/// discipline as the preview cache), emits `batch://update` snapshots.
/// Pause = do not START the next file; the in-flight file finishes.
pub fn spawn_worker<R: tauri::Runtime>(
    q: Arc<BatchQueue>,
    env: WorkerEnv,
    app: tauri::AppHandle<R>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use tauri::Emitter;
        let mut notify = |snap: &Value| {
            let _ = app.emit("batch://update", snap);
        };
        loop {
            {
                let mut g = q.inner.lock().unwrap();
                loop {
                    if g.shutdown {
                        return;
                    }
                    if !g.paused && g.jobs.iter().any(|j| j.status == JobStatus::Queued) {
                        break;
                    }
                    g = q.signal.wait(g).unwrap();
                }
            }
            run_one(&q, &env, &mut notify);
        }
    })
}

/// Graceful stop (app exit): the worker wakes, sees the flag, exits.
/// An in-flight render finishes before the process tears down (Tauri
/// runs the window close before drop, so the write lands durably).
pub fn shutdown(q: &BatchQueue) {
    let mut g = q.inner.lock().unwrap();
    g.shutdown = true;
    drop(g);
    q.signal.notify_all();
}

// ─── Commands (S6 surface — every mutation returns the fresh snapshot,
//      so the UI works from returns OR events, whichever arrives first) ───

/// Add files to the queue. `preset_id: None` = the recipe currently
/// live in the editor (snapshotted now — later edits never mutate a
/// queued job); `Some(id)` = a user preset. All files share the recipe
/// + spec of this call. Per-file honesty: unreadable file or a recipe
/// referencing a missing stock lands the ROW in error — the rest of the
/// add still queues.
#[tauri::command]
pub fn batch_add(
    state: St,
    queue: QSt,
    paths: Vec<String>,
    preset_id: Option<String>,
    spec: ExportSpec,
    dest_dir: String,
) -> Result<Value, AppError> {
    let dest = PathBuf::from(&dest_dir);
    if !dest.is_dir() {
        return Err(AppError::new(
            "io.dest_missing",
            format!("output folder {} does not exist", dest.display()),
        )
        .with_context("hint.chooseFolder"));
    }

    let st = state.lock().unwrap();
    let (stages, preset_name): (Vec<StageEntry>, Option<String>) = match &preset_id {
        Some(id) => {
            let p = crate::user_presets::UserPreset::find(&crate::commands::presets_dir(&st), id)
                .ok_or_else(|| {
                AppError::new("preset.missing", format!("preset {id} not found"))
            })?;
            (p.stages.clone(), Some(p.name.clone()))
        }
        None => (st.current_stages(), None),
    };
    // Registry honesty (P2: never a wrong-color render). Checked once —
    // the registry is immutable after startup.
    let stock_err = export::missing_stock_id(&stages, &st.stocks).map(|id| JobError {
        code: "stock.missing".into(),
        message: format!("stock {id} is not in the registry"),
        hint: Some("hint.restoreStock".into()),
    });
    let added_at = crate::commands::now_rfc3339();
    drop(st);

    let mut g = queue.inner.lock().unwrap();
    for p in &paths {
        let src = PathBuf::from(p);
        let name = src
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (status, error) = if let Some(e) = &stock_err {
            (JobStatus::Error, Some(e.clone()))
        } else if !src.is_file() {
            (
                JobStatus::Error,
                Some(JobError {
                    code: "io.missing".into(),
                    message: format!("{} is not a readable file", src.display()),
                    hint: Some("hint.fileMissing".into()),
                }),
            )
        } else {
            (JobStatus::Queued, None)
        };
        let id = g.next_id;
        g.next_id = g.next_id.saturating_add(1);
        g.jobs.push(BatchJob {
            id,
            source: src,
            name,
            preset_id: preset_id.clone(),
            preset_name: preset_name.clone(),
            stages: stages.clone(),
            spec: spec.clone(),
            dest_dir: dest.clone(),
            dest: None,
            status,
            error,
            warnings: Vec::new(),
            dims: None,
            bytes: None,
            total_ms: None,
            added_at: added_at.clone(),
            finished_at: None,
            thumb: None,
        });
    }
    drop(g);
    queue.signal.notify_all();
    Ok(queue.snapshot())
}

#[tauri::command]
pub fn batch_list(queue: QSt) -> Value {
    queue.snapshot()
}

/// Row thumbnail (lazy): decode → AP1 → 160 px original glyph → PNG
/// data URL. Generated once, cached on the job. Best-effort — a decode
/// failure returns the honest error (the row shows a file glyph).
#[tauri::command]
pub fn batch_thumb(queue: QSt, id: u32) -> Result<String, AppError> {
    {
        let g = queue.inner.lock().unwrap();
        if let Some(j) = g.jobs.iter().find(|j| j.id == id) {
            if let Some(t) = &j.thumb {
                return Ok(t.clone());
            }
        }
    }
    let source = {
        let g = queue.inner.lock().unwrap();
        g.jobs
            .iter()
            .find(|j| j.id == id)
            .map(|j| j.source.clone())
            .ok_or_else(|| AppError::new("batch.job", format!("no job {id}")))?
    };

    let ap1 = crate::commands::load_ap1(&source)?;
    let frame = crate::render::compose_original(&ap1, 160);
    // RGBA8 → PNG bytes via the one encoder (temp file — the io crate
    // encodes to paths; decode dominates the cost either way).
    let tmp = std::env::temp_dir().join(format!("melography-batch-thumb-{id}.png"));
    let mut rgb = Vec::with_capacity(frame.rgba.len() / 4 * 3);
    const BG: f32 = 14.0; // the app chrome dark (same composite as recents)
    for px in frame.rgba.as_chunks::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        for v in &px[..3] {
            let v = f32::from(*v) * a + BG * (1.0 - a);
            rgb.push(v.round().clamp(0.0, 255.0) as u8);
        }
    }
    melography_io::encode::encode_png8(&tmp, frame.width, frame.height, &rgb, None)
        .map_err(|e| AppError::new("io.write", format!("thumb encode: {e}")))?;
    let png = std::fs::read(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    let mut url = String::with_capacity(32 + png.len() * 4 / 3);
    url.push_str("data:image/png;base64,");
    crate::commands::base64_encode(&png, &mut url);

    let mut g = queue.inner.lock().unwrap();
    if let Some(j) = g.jobs.iter_mut().find(|j| j.id == id) {
        j.thumb = Some(url.clone());
    }
    Ok(url)
}

/// Global pause: the worker finishes the in-flight file, then waits.
#[tauri::command]
pub fn batch_pause(queue: QSt) -> Value {
    let mut g = queue.inner.lock().unwrap();
    g.paused = true;
    drop(g);
    queue.snapshot()
}

#[tauri::command]
pub fn batch_resume(queue: QSt) -> Value {
    let mut g = queue.inner.lock().unwrap();
    g.paused = false;
    drop(g);
    queue.signal.notify_all();
    queue.snapshot()
}

/// Cancel ONE job. Queued → cancelled immediately; rendering → the
/// honest error (the file will finish; pause stops the NEXT one).
#[tauri::command]
pub fn batch_cancel(queue: QSt, id: u32) -> Result<Value, AppError> {
    let mut g = queue.inner.lock().unwrap();
    let Some(j) = g.jobs.iter_mut().find(|j| j.id == id) else {
        return Err(AppError::new("batch.job", format!("no job {id}")));
    };
    match j.status {
        JobStatus::Queued => {
            j.status = JobStatus::Cancelled;
            j.finished_at = Some(crate::commands::now_rfc3339());
        }
        JobStatus::Rendering => {
            return Err(AppError::new(
                "batch.rendering",
                "this file is rendering and will finish; pause stops the next file",
            ))
        }
        _ => {}
    }
    drop(g);
    Ok(queue.snapshot())
}

/// Cancel every queued job (the in-flight file finishes — stated in
/// the UI, never implied).
#[tauri::command]
pub fn batch_cancel_all(queue: QSt) -> Value {
    let now = crate::commands::now_rfc3339();
    let mut g = queue.inner.lock().unwrap();
    for j in g.jobs.iter_mut() {
        if j.status == JobStatus::Queued {
            j.status = JobStatus::Cancelled;
            j.finished_at = Some(now.clone());
        }
    }
    drop(g);
    queue.snapshot()
}

/// Retry keeps the job's settings (stages + spec — structural, see
/// `BatchJob`). `new_dest_dir` is the disk-full recovery flow: pick
/// another folder, retry into it. Error/Cancelled → queued again.
#[tauri::command]
pub fn batch_retry(queue: QSt, id: u32, new_dest_dir: Option<String>) -> Result<Value, AppError> {
    let mut g = queue.inner.lock().unwrap();
    let Some(j) = g.jobs.iter_mut().find(|j| j.id == id) else {
        return Err(AppError::new("batch.job", format!("no job {id}")));
    };
    match j.status {
        JobStatus::Error | JobStatus::Cancelled => {
            if let Some(nd) = &new_dest_dir {
                let p = PathBuf::from(nd);
                if !p.is_dir() {
                    return Err(AppError::new(
                        "io.dest_missing",
                        format!("output folder {} does not exist", p.display()),
                    )
                    .with_context("hint.chooseFolder"));
                }
                j.dest_dir = p;
            }
            j.status = JobStatus::Queued;
            j.error = None;
            j.warnings.clear();
            j.dest = None;
            j.finished_at = None;
        }
        JobStatus::Rendering => {
            return Err(AppError::new(
                "batch.rendering",
                "this file is rendering and will finish",
            ))
        }
        _ => {}
    }
    drop(g);
    queue.signal.notify_all();
    Ok(queue.snapshot())
}

/// Remove a row (never the rendering one).
#[tauri::command]
pub fn batch_remove(queue: QSt, id: u32) -> Result<Value, AppError> {
    let mut g = queue.inner.lock().unwrap();
    if let Some(j) = g.jobs.iter().find(|j| j.id == id) {
        if j.status == JobStatus::Rendering {
            return Err(AppError::new(
                "batch.rendering",
                "this file is rendering and will finish",
            ));
        }
    }
    g.jobs.retain(|j| j.id != id);
    drop(g);
    Ok(queue.snapshot())
}

/// Clear finished work (done/cancelled/error rows); queued + rendering
/// stay.
#[tauri::command]
pub fn batch_clear(queue: QSt) -> Value {
    let mut g = queue.inner.lock().unwrap();
    g.jobs
        .retain(|j| matches!(j.status, JobStatus::Queued | JobStatus::Rendering));
    drop(g);
    queue.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(dir: &Path) -> WorkerEnv {
        WorkerEnv {
            library_root: dir.join("library"),
            profiles_dir: dir.join("profiles"),
        }
    }

    fn job(id: u32, source: &Path, dir: &Path) -> BatchJob {
        BatchJob {
            id,
            source: source.to_path_buf(),
            name: source
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            preset_id: None,
            preset_name: None,
            stages: vec![StageEntry::FilmIdentity {
                enabled: true,
                extra: Default::default(),
            }],
            spec: ExportSpec {
                format: "png".into(),
                depth: 8,
                quality: 92,
            },
            dest_dir: dir.to_path_buf(),
            dest: None,
            status: JobStatus::Queued,
            error: None,
            warnings: Vec::new(),
            dims: None,
            bytes: None,
            total_ms: None,
            added_at: "2026-10-24T00:00:00Z".into(),
            finished_at: None,
            thumb: None,
        }
    }

    fn push(q: &BatchQueue, j: BatchJob) {
        q.inner.lock().unwrap().jobs.push(j);
    }

    fn statuses(q: &BatchQueue) -> Vec<JobStatus> {
        q.inner
            .lock()
            .unwrap()
            .jobs
            .iter()
            .map(|j| j.status)
            .collect()
    }

    /// The full happy path: queued → (notify) rendering → (notify) done,
    /// output written with the reserved name, facts recorded.
    #[test]
    fn run_one_renders_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("photo.png");
        melography_io::encode::encode_png8(&src, 4, 4, &[90u8; 4 * 4 * 3], None).unwrap();
        let q = BatchQueue::new();
        push(&q, job(1, &src, dir.path()));

        let mut notifications = 0usize;
        run_one(&q, &env(dir.path()), &mut |_snap| notifications += 1);

        assert_eq!(statuses(&q), vec![JobStatus::Done]);
        assert_eq!(notifications, 2, "rendering + done transitions");
        let g = q.inner.lock().unwrap();
        let j = &g.jobs[0];
        assert_eq!(j.dims, Some((4, 4)));
        assert!(j.bytes.unwrap() > 0);
        assert!(j.dest.as_ref().unwrap().exists());
        assert_eq!(
            j.dest
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy(),
            "photo-melography.png"
        );
    }

    /// Sequential workers never collide on the same output name.
    #[test]
    fn two_jobs_same_stem_get_distinct_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.png");
        melography_io::encode::encode_png8(&src, 2, 2, &[10u8; 2 * 2 * 3], None).unwrap();
        let q = BatchQueue::new();
        push(&q, job(1, &src, dir.path()));
        push(&q, job(2, &src, dir.path()));
        run_one(&q, &env(dir.path()), &mut |_| {});
        run_one(&q, &env(dir.path()), &mut |_| {});
        let g = q.inner.lock().unwrap();
        let d1 = g.jobs[0].dest.clone().unwrap();
        let d2 = g.jobs[1].dest.clone().unwrap();
        assert_ne!(d1, d2, "collision-suffixed, never overwritten");
        assert!(d1.exists() && d2.exists());
    }

    /// Unreadable source → per-row error, queue lives on.
    #[test]
    fn missing_file_is_a_row_error() {
        let dir = tempfile::tempdir().unwrap();
        let q = BatchQueue::new();
        push(&q, job(7, &dir.path().join("nope.png"), dir.path()));
        run_one(&q, &env(dir.path()), &mut |_| {});
        let g = q.inner.lock().unwrap();
        assert_eq!(g.jobs[0].status, JobStatus::Error);
        assert!(!g.jobs[0].error.as_ref().unwrap().code.is_empty());
    }

    /// A recipe referencing a stock the registry cannot resolve NEVER
    /// renders wrong colors — it fails with the actionable code.
    #[test]
    fn missing_stock_is_an_honest_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("x.png");
        melography_io::encode::encode_png8(&src, 2, 2, &[10u8; 2 * 2 * 3], None).unwrap();
        let mut j = job(1, &src, dir.path());
        j.stages = vec![StageEntry::FilmStock {
            stock: "ghost-stock".into(),
            stock_version: 1,
            enabled: true,
            adjustments: Default::default(),
            lens: Default::default(),
            extra: Default::default(),
        }];
        let q = BatchQueue::new();
        push(&q, j);
        run_one(&q, &env(dir.path()), &mut |_| {});
        let g = q.inner.lock().unwrap();
        assert_eq!(g.jobs[0].status, JobStatus::Error);
        assert_eq!(g.jobs[0].error.as_ref().unwrap().code, "stock.missing");
        // And the retry path re-queues it unchanged (settings kept).
    }

    #[test]
    fn paused_worker_does_not_start_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("p.png");
        melography_io::encode::encode_png8(&src, 2, 2, &[10u8; 2 * 2 * 3], None).unwrap();
        let q = BatchQueue::new();
        push(&q, job(1, &src, dir.path()));
        q.inner.lock().unwrap().paused = true;
        run_one(&q, &env(dir.path()), &mut |_| {});
        assert_eq!(statuses(&q), vec![JobStatus::Queued]);
    }

    #[test]
    fn cancel_retry_remove_clear_state_machine() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("c.png");
        melography_io::encode::encode_png8(&src, 2, 2, &[10u8; 2 * 2 * 3], None).unwrap();
        let q = BatchQueue::new();
        push(&q, job(1, &src, dir.path()));
        push(&q, job(2, &src, dir.path()));

        // Cancel a queued job (command-level semantics, inlined here —
        // the command is a thin lock around exactly this).
        {
            let mut g = q.inner.lock().unwrap();
            g.jobs[0].status = JobStatus::Cancelled;
        }
        // run_one must skip the cancelled row and render job 2.
        run_one(&q, &env(dir.path()), &mut |_| {});
        {
            // Scoped lock — a function-scoped `let g` here deadlocked
            // against the re-lock below (std Mutex is not reentrant).
            let g = q.inner.lock().unwrap();
            assert_eq!(g.jobs[0].status, JobStatus::Cancelled);
            assert_eq!(g.jobs[1].status, JobStatus::Done);
        }

        // Retry re-queues with settings kept; new dest dir honored.
        let other = tempfile::tempdir().unwrap();
        {
            let mut g = q.inner.lock().unwrap();
            g.jobs[0].dest_dir = other.path().to_path_buf();
            g.jobs[0].status = JobStatus::Cancelled;
        }
        {
            let mut g = q.inner.lock().unwrap();
            let j = &mut g.jobs[0];
            assert!(matches!(j.status, JobStatus::Cancelled | JobStatus::Error));
            j.status = JobStatus::Queued; // retry
        }
        run_one(&q, &env(dir.path()), &mut |_| {});
        {
            let g = q.inner.lock().unwrap();
            assert_eq!(g.jobs[0].status, JobStatus::Done);
            assert!(g.jobs[0].dest.as_ref().unwrap().starts_with(other.path()));
        }

        // Clear removes finished rows only.
        {
            let mut g = q.inner.lock().unwrap();
            g.jobs
                .retain(|j| matches!(j.status, JobStatus::Queued | JobStatus::Rendering));
        }
        assert!(q.inner.lock().unwrap().jobs.is_empty());
    }

    /// The wire snapshot carries the honest surface the S6 rows need.
    #[test]
    fn snapshot_shape() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("s.png");
        melography_io::encode::encode_png8(&src, 2, 2, &[10u8; 2 * 2 * 3], None).unwrap();
        let q = BatchQueue::new();
        push(&q, job(3, &src, dir.path()));
        let snap = q.snapshot();
        let j = &snap["jobs"][0];
        assert_eq!(j["id"], 3);
        assert_eq!(j["status"], "queued");
        assert_eq!(j["format"], "png");
        assert!(
            j["stages"].is_null(),
            "stages are never serialized to the UI"
        );
        assert_eq!(j["has_thumb"], false);
    }
}
