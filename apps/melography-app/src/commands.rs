//! Tauri commands — the ONLY bridge between the webview and the Rust
//! pixel pipeline (ARCHITECTURE §1 invariant: the webview never computes
//! pixels; it receives preview frames and sends parameters/events).
//!
//! Command surface (M1, UX_SPEC §4/§6 slices):
//! * `open_image` / `close_image` — session lifecycle (original untouched)
//! * `render_preview` / `original_frame` — binary preview transport
//! * `import_lut` / `list_luts` / `remove_lut` — library (D1 flow)
//! * `set_lut_stack` / `get_state` — parameter surface (panel ↔ recipe)
//! * `save_recipe` / `export_image` — persistence + export (reuses the
//!   CLI's headless pipeline for byte-identical behavior)

use crate::error::AppError;
use crate::render::{self, PreviewFrame};
use crate::state::EditorState;
use melography_cli::{ApplyFormat, ApplyOptions};
use melography_lut::stage::LutApply;
use melography_lut::{LutEncoding, LutRecord};
use melography_presets::sidecar::{Fingerprint, SidecarV1, StageEntry};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

/// Inline in every command signature (type aliases cannot carry the
/// anonymous lifetime through tauri's macro expansion).
type St<'a> = State<'a, Mutex<EditorState>>;

// ─── session ────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ImageInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Sidecar discovered next to the original (recipe present?).
    pub sidecar_loaded: bool,
    pub fingerprint_sha256: String,
}

#[tauri::command]
pub fn open_image(state: St, path: String) -> Result<ImageInfo, AppError> {
    let path = PathBuf::from(path);
    let file_bytes = std::fs::read(&path)
        .map_err(|e| AppError::new("io.read", format!("cannot open {}: {e}", path.display())))?;

    let decoded = melography_io::decode::decode_file(&path)?;
    let encoding = match &decoded.encoding {
        melography_core::ColorEncoding::Icc { profile } => melography_core::ColorEncoding::Icc {
            profile: profile.clone(),
        },
        _ => melography_core::ColorEncoding::Srgb { linear: false },
    };
    let fastpath = matches!(
        decoded.encoding,
        melography_core::ColorEncoding::Srgb { .. } | melography_core::ColorEncoding::Untagged
    );
    let ap1 = if fastpath {
        melography_color::srgb_to_ap1_fast(&decoded.pixels)
    } else {
        melography_color::to_ap1_lcms(&decoded.pixels, &encoding)
            .map_err(|e| AppError::new("color.transform", format!("{e}")))?
    };

    let fingerprint = Fingerprint::compute(&file_bytes, &decoded.pixels);
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Existing sidecar next to the original? (non-destructive resume)
    let sidecar_path = path.with_extension("melography.json");
    let (sidecar, sidecar_loaded) = if sidecar_path.exists() {
        let sc = SidecarV1::load(&sidecar_path)?;
        (sc, true)
    } else {
        let sc = SidecarV1::new(name.clone(), Some(fingerprint.clone()));
        (sc, false)
    };

    let mut st = state.lock().unwrap();
    let lut_stack: Vec<LutApply> = sidecar
        .pipeline
        .stages
        .iter()
        .filter_map(|s| match s {
            StageEntry::Lut { .. } => <&StageEntry as TryInto<LutApply>>::try_into(s).ok(),
            StageEntry::FilmIdentity { .. } => None,
        })
        .collect();

    st.session = Some(crate::state::OpenSession {
        source_path: path.clone(),
        original_name: name.clone(),
        width: decoded.pixels.width(),
        height: decoded.pixels.height(),
        ap1,
        fingerprint: fingerprint.clone(),
    });
    st.lut_stack = lut_stack;
    st.sidecar = Some(sidecar);
    st.sidecar_path = Some(sidecar_path);
    st.dirty = false;
    st.preview_cache = None;

    Ok(ImageInfo {
        name,
        width: st.session.as_ref().unwrap().width,
        height: st.session.as_ref().unwrap().height,
        sidecar_loaded,
        fingerprint_sha256: fingerprint.sha256_file,
    })
}

#[tauri::command]
pub fn close_image(state: St) {
    let mut st = state.lock().unwrap();
    st.session = None;
    st.sidecar = None;
    st.sidecar_path = None;
    st.lut_stack.clear();
    st.preview_cache = None;
    st.dirty = false;
}

// ─── preview transport ──────────────────────────────────────────────────────

#[tauri::command]
pub fn render_preview(state: St, max_dim: u32) -> Result<tauri::ipc::Response, AppError> {
    let mut st = state.lock().unwrap();
    let base = st
        .session
        .as_ref()
        .ok_or_else(|| AppError::new("session.none", "no image is open"))?
        .ap1
        .clone();
    let stack = st.resolve_stack()?;
    let frame: PreviewFrame =
        render::compose_preview(&base, &mut st.preview_cache, &stack, max_dim)?;
    Ok(tauri::ipc::Response::new(frame.to_wire()))
}

#[tauri::command]
pub fn original_frame(state: St, max_dim: u32) -> Result<tauri::ipc::Response, AppError> {
    let st = state.lock().unwrap();
    let session = st
        .session
        .as_ref()
        .ok_or_else(|| AppError::new("session.none", "no image is open"))?;
    let frame = render::compose_original(&session.ap1, max_dim);
    Ok(tauri::ipc::Response::new(frame.to_wire()))
}

// ─── library (D1 import flow) ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct LutRecordJson {
    pub content_hash: String,
    pub original_filename: String,
    pub title: Option<String>,
    pub kind: String,
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub declared_input: String,
    pub declared_output: String,
    pub source_url: Option<String>,
    pub license_note: Option<String>,
    pub imported_utc: String,
    pub warnings: Vec<melography_lut::CubeWarning>,
    pub implemented: bool,
}

impl From<&LutRecord> for LutRecordJson {
    fn from(r: &LutRecord) -> Self {
        Self {
            content_hash: r.content_hash.clone(),
            original_filename: r.original_filename.clone(),
            title: r.title.clone(),
            kind: format!("{:?}", r.kind),
            size: r.size,
            domain_min: r.domain_min,
            domain_max: r.domain_max,
            declared_input: r.declared_input.as_str().to_string(),
            declared_output: r.declared_output.as_str().to_string(),
            source_url: r.source_url.clone(),
            license_note: r.license_note.clone(),
            imported_utc: r.imported_utc.clone(),
            warnings: r.warnings.clone(),
            implemented: r.declared_input.implemented() && r.declared_output.implemented(),
        }
    }
}

#[derive(Deserialize)]
pub struct ImportLutArgs {
    pub path: String,
    /// Fixed vocabulary (luts.md §3); UI only offers implemented entries.
    pub declared_input: String,
    pub declared_output: String,
    pub source_url: Option<String>,
    pub license_note: Option<String>,
}

#[tauri::command]
pub fn import_lut(state: St, args: ImportLutArgs) -> Result<LutRecordJson, AppError> {
    let declared_input = LutEncoding::from_str_fixed(&args.declared_input)?;
    let declared_output = LutEncoding::from_str_fixed(&args.declared_output)?;
    let st = state.lock().unwrap();
    let record = st.library.import(
        std::path::Path::new(&args.path),
        declared_input,
        declared_output,
        melography_lut::ImportProvenance {
            source_url: args.source_url,
            license_note: args.license_note,
        },
    )?;
    Ok(LutRecordJson::from(&record))
}

#[tauri::command]
pub fn list_luts(state: St) -> Result<Vec<LutRecordJson>, AppError> {
    let st = state.lock().unwrap();
    Ok(st.library.list()?.iter().map(LutRecordJson::from).collect())
}

#[tauri::command]
pub fn remove_lut(state: St, hash: String) -> Result<(), AppError> {
    let mut st = state.lock().unwrap();
    st.library.remove(&hash)?;
    st.luts_loaded.remove(&hash);
    // Recipes referencing the hash keep it — the missing-file error state
    // surfaces at resolve time (by design, luts.md §4).
    Ok(())
}

// ─── parameter surface (LUT panel ↔ recipe) ────────────────────────────────

#[derive(Deserialize, Clone)]
pub struct LutStackEntry {
    pub lut_hash: String,
    pub insertion: String,
    pub intensity: f32,
    pub declared_input: String,
    pub declared_output: String,
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct StateJson {
    pub image_open: bool,
    pub image_name: Option<String>,
    pub image_dims: Option<[u32; 2]>,
    pub lut_stack: Vec<LutStackJson>,
    pub dirty: bool,
    pub sidecar_path: Option<String>,
    /// Aggregate clamped-pixel count from the last render (badge input).
    pub last_clamped_pixels: u64,
    pub library_count: usize,
}

#[derive(Serialize)]
pub struct LutStackJson {
    pub lut_hash: String,
    pub insertion: String,
    pub intensity: f32,
    pub declared_input: String,
    pub declared_output: String,
    pub enabled: bool,
}

#[tauri::command]
pub fn set_lut_stack(state: St, stack: Vec<LutStackEntry>) -> Result<(), AppError> {
    let mut st = state.lock().unwrap();
    let mut parsed = Vec::with_capacity(stack.len());
    for e in stack {
        let insertion = render::parse_insertion(&e.insertion)?;
        let declared_input = LutEncoding::from_str_fixed(&e.declared_input)?;
        let declared_output = LutEncoding::from_str_fixed(&e.declared_output)?;
        parsed.push(LutApply {
            lut_hash: e.lut_hash,
            insertion,
            intensity: e.intensity.clamp(0.0, 1.0),
            declared_input,
            declared_output,
            enabled: e.enabled,
        });
    }
    st.lut_stack = parsed;
    st.dirty = true; // cache key changes → next render recomputes
    Ok(())
}

#[tauri::command]
pub fn get_state(state: St) -> Result<StateJson, AppError> {
    let st = state.lock().unwrap();
    let last_clamped = st
        .preview_cache
        .as_ref()
        .map(|c| c.clamped_pixels)
        .unwrap_or(0);
    Ok(StateJson {
        image_open: st.session.is_some(),
        image_name: st.session.as_ref().map(|s| s.original_name.clone()),
        image_dims: st.session.as_ref().map(|s| [s.width, s.height]),
        lut_stack: st
            .lut_stack
            .iter()
            .map(|a| LutStackJson {
                lut_hash: a.lut_hash.clone(),
                insertion: format!("{:?}", a.insertion).to_lowercase(),
                intensity: a.intensity,
                declared_input: a.declared_input.as_str().to_string(),
                declared_output: a.declared_output.as_str().to_string(),
                enabled: a.enabled,
            })
            .collect(),
        dirty: st.dirty,
        sidecar_path: st.sidecar_path.as_ref().map(|p| p.display().to_string()),
        last_clamped_pixels: last_clamped,
        library_count: st.library.list().map(|l| l.len()).unwrap_or(0),
    })
}

// ─── persistence + export ───────────────────────────────────────────────────

#[tauri::command]
pub fn save_recipe(state: St) -> Result<(), AppError> {
    let mut st = state.lock().unwrap();
    let Some(path) = st.sidecar_path.clone() else {
        return Err(AppError::new("session.none", "no sidecar to save"));
    };
    // Take the sidecar out of the state while rebuilding its pipeline
    // block from the live stack (+ identity film) — single-owner writes.
    let stack_snapshot = st.lut_stack.clone();
    let mut sc = st
        .sidecar
        .take()
        .ok_or_else(|| AppError::new("session.none", "no sidecar to save"))?;
    sc.pipeline.stages.clear();
    sc.pipeline.stages.push(StageEntry::FilmIdentity {
        enabled: true,
        extra: Default::default(),
    });
    for a in &stack_snapshot {
        sc.pipeline.stages.push(StageEntry::from(a));
    }
    sc.touch();
    let save_result = sc.save(&path);
    st.sidecar = Some(sc);
    st.dirty = false;
    save_result.map_err(AppError::from)
}

#[derive(Deserialize)]
pub struct ExportArgs {
    pub format: String,
    pub depth: u32,
    pub quality: u8,
}

#[tauri::command]
pub fn export_image(
    state: St,
    args: ExportArgs,
    dest: String,
) -> Result<serde_json::Value, AppError> {
    let st = state.lock().unwrap();
    let Some(session) = st.session.as_ref() else {
        return Err(AppError::new("session.none", "no image is open"));
    };
    let format = ApplyFormat::parse(&args.format)?;
    let lut_path = if let Some(first) = st.lut_stack.first() {
        // Export re-imports the source LUT via the library (the CLI
        // pipeline expects a file; the library holds the content-addressed
        // copy) — byte-identical to the CLI path.
        let blob = st
            .library_root()
            .join("luts")
            .join(format!("{}.cube", first.lut_hash));
        if blob.exists() {
            Some(blob)
        } else {
            return Err(AppError::new(
                "lut.missing",
                format!("LUT {} is missing from the library", first.lut_hash),
            )
            .with_context(first.lut_hash.clone()));
        }
    } else {
        None
    };
    let apply = st.lut_stack.first().map(|first| LutApply {
        lut_hash: first.lut_hash.clone(),
        insertion: first.insertion,
        intensity: first.intensity,
        declared_input: first.declared_input,
        declared_output: first.declared_output,
        enabled: first.enabled,
    });

    let mut opts = ApplyOptions::simple(session.source_path.clone(), PathBuf::from(dest));
    opts.format = format;
    opts.depth = args.depth;
    opts.quality = args.quality;
    opts.lut = lut_path;
    opts.library_root = st.library_root();
    if let Some(a) = apply {
        opts.lut_in = a.declared_input;
        opts.lut_out = a.declared_output;
        opts.intensity = a.intensity;
        opts.insertion = a.insertion;
    }

    let report = melography_cli::cmd_apply(&opts);
    if let Some(err) = report.get("error") {
        return Err(AppError::new(
            "export.failed",
            err.as_str().unwrap_or("export failed").to_string(),
        ));
    }
    Ok(report)
}

// EditorState helper (single place that knows the root).
impl EditorState {
    pub fn library_root(&self) -> PathBuf {
        // The LutLibrary was constructed with this root; mirror it.
        self.library.root().to_path_buf()
    }
}
