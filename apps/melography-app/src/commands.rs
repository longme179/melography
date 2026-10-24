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
use crate::render;
use crate::state::EditorState;
use melography_color::tone::ToneParams;
use melography_lut::stage::LutApply;
use melography_lut::{LutEncoding, LutRecord};
use melography_presets::sidecar::{Fingerprint, SidecarV1, SuggestionKind, SuggestionSource};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

/// Inline in every command signature (type aliases cannot carry the
/// anonymous lifetime through tauri's macro expansion).
type St<'a> = State<'a, Mutex<EditorState>>;

/// Decode a source file to the full-resolution AP1-linear working base
/// (post S1). ONE decode path shared by `open_image` and the batch
/// queue's lazy thumbnails (session 08).
pub(crate) fn load_ap1(path: &Path) -> Result<melography_core::ImageF32, AppError> {
    let decoded = melography_io::decode::decode_file(path)?;
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
    if fastpath {
        Ok(melography_color::srgb_to_ap1_fast(&decoded.pixels))
    } else {
        melography_color::to_ap1_lcms(&decoded.pixels, &encoding)
            .map_err(|e| AppError::new("color.transform", format!("{e}")))
    }
}

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
    let ap1 = {
        // Same one decode path (load_ap1) — open_image additionally
        // needs the source encoding for the fingerprint path below.
        let encoding = match &decoded.encoding {
            melography_core::ColorEncoding::Icc { profile } => {
                melography_core::ColorEncoding::Icc {
                    profile: profile.clone(),
                }
            }
            _ => melography_core::ColorEncoding::Srgb { linear: false },
        };
        let fastpath = matches!(
            decoded.encoding,
            melography_core::ColorEncoding::Srgb { .. } | melography_core::ColorEncoding::Untagged
        );
        if fastpath {
            melography_color::srgb_to_ap1_fast(&decoded.pixels)
        } else {
            melography_color::to_ap1_lcms(&decoded.pixels, &encoding)
                .map_err(|e| AppError::new("color.transform", format!("{e}")))?
        }
    };

    let fingerprint = Fingerprint::compute(&file_bytes, &decoded.pixels);
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Existing sidecar next to the original? (non-destructive resume)
    let sidecar_path = path.with_extension("melography.json");
    // Crash-safety recovery (M4 session model, issue #24): a killed save
    // can leave `.{name}.melography.json.tmp-*` litter — clean it before
    // any new save can collide; never fatal (a wedged temp file must not
    // block opening the image).
    let _litter = melography_core::durable::clean_stale_tmp(&sidecar_path);
    let (sidecar, sidecar_loaded) = if sidecar_path.exists() {
        let sc = SidecarV1::load(&sidecar_path)?;
        (sc, true)
    } else {
        let sc = SidecarV1::new(name.clone(), Some(fingerprint.clone()));
        (sc, false)
    };

    let mut st = state.lock().unwrap();
    // The ONE stage→state path (state.rs `apply_pipeline_stages`) — also
    // fixes the M2/M3 gap where FilmStock stages were serialized on save
    // but never applied back on re-open (the film chain silently reset).
    let stages = sidecar.pipeline.stages.clone();
    st.apply_pipeline_stages(&stages);

    st.session = Some(crate::state::OpenSession {
        source_path: path.clone(),
        original_name: name.clone(),
        width: decoded.pixels.width(),
        height: decoded.pixels.height(),
        ap1,
        fingerprint: fingerprint.clone(),
    });
    st.sidecar = Some(sidecar);
    st.sidecar_path = Some(sidecar_path);
    st.dirty = false;
    st.preview_cache.slot = None; // keep budget + counters (#25)
    st.reset_history(); // history belongs to this image's session (M4 #24)

    // S1 Home (issue #23): record the open + refresh the list thumbnail
    // from the ORIGINAL (a glyph of the photo, not a render of the recipe).
    // Best-effort — a thumb failure downgrades to the previous one, never
    // blocks the open.
    let (sw, sh) = st
        .session
        .as_ref()
        .map(|s| (s.width, s.height))
        .unwrap_or((0, 0));
    let thumb = st.session.as_ref().map(|s| {
        let f = render::compose_original(&s.ap1, 160);
        (f.width, f.height, f.rgba)
    });
    let lib = st.library_root();
    let path_str = path.to_string_lossy().into_owned();
    st.recents.record_open(
        &lib.join("recents.json"),
        &lib.join("thumbs"),
        crate::recents::RecordOpen {
            path: &path_str,
            name: &name,
            width: sw,
            height: sh,
            has_recipe: sidecar_loaded,
            thumb_rgba: thumb.as_ref().map(|(w, h, rgba)| (*w, *h, rgba.as_slice())),
        },
    );

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
    st.preview_cache.slot = None; // keep budget + counters (#25)
    st.dirty = false;
    st.reset_history(); // no session → no history (M4 #24)
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
    let film_leg = st.film_leg();
    let tone_params = st.tone.params.filter(|p| !p.is_identity());
    let frame = render::compose_preview(
        &base,
        &mut st.preview_cache,
        &stack,
        film_leg.as_ref(),
        tone_params.as_ref(),
        max_dim,
    )?;
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
    /// Undo availability (M4 #24 — toolbar + keyboard state).
    pub can_undo: bool,
    pub can_redo: bool,
    /// Current film stock id (null = film chain off) — panel refresh
    /// after history jumps and preset applies (M4 #23).
    pub film_stock: Option<String>,
    /// A recipe referenced a stock id missing from the registry (amber
    /// badge — never silently skipped, UX_SPEC §9).
    pub missing_stock: Option<String>,
    /// Absolute path of the open original (S6 "add to batch" — the
    /// queue renders from the source file, not the session).
    pub source_path: Option<String>,
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
    st.push_history(); // recipe mutation → undo-able (M4 #24)
    st.lut_stack = parsed;
    st.dirty = true; // cache key changes → next render recomputes
    Ok(())
}

#[tauri::command]
pub fn get_state(state: St) -> Result<StateJson, AppError> {
    let st = state.lock().unwrap();
    Ok(state_json(&st))
}

/// Snapshot of editor state for the UI (shared by get_state/undo/redo so
/// every path returns the same shape).
fn state_json(st: &crate::state::EditorState) -> StateJson {
    let last_clamped = st
        .preview_cache
        .slot
        .as_ref()
        .map(|c| c.clamped_pixels)
        .unwrap_or(0);
    StateJson {
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
        can_undo: !st.history.is_empty(),
        can_redo: !st.redo.is_empty(),
        film_stock: st.film.stock.as_ref().map(|p| p.id.clone()),
        missing_stock: st.missing_stock.clone(),
        source_path: st
            .session
            .as_ref()
            .map(|s| s.source_path.display().to_string()),
    }
}

/// Undo the last recipe mutation (UX_SPEC §4 History, M4 issue #24).
/// Ctrl+Z — returns the refreshed state; an empty stack is an explicit
/// error (the UI keeps its buttons disabled, so this is a contract
/// violation, not a normal flow).
#[tauri::command]
pub fn undo(state: St) -> Result<StateJson, AppError> {
    let mut st = state.lock().unwrap();
    let Some(prev) = st.history.pop() else {
        return Err(AppError::new("session.history", "nothing to undo"));
    };
    let current = st.snapshot();
    st.redo.push(current);
    st.restore(prev);
    Ok(state_json(&st))
}

/// Redo the last undone mutation (Ctrl+Shift+Z / Ctrl+Y).
#[tauri::command]
pub fn redo(state: St) -> Result<StateJson, AppError> {
    let mut st = state.lock().unwrap();
    let Some(next) = st.redo.pop() else {
        return Err(AppError::new("session.history", "nothing to redo"));
    };
    let current = st.snapshot();
    st.history.push(current);
    st.restore(next);
    Ok(state_json(&st))
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
    // The stage list comes from `current_stages` — the same serialization
    // `save_preset` uses, so recipes and presets can never drift.
    let stages = st.current_stages();
    let mut sc = st
        .sidecar
        .take()
        .ok_or_else(|| AppError::new("session.none", "no sidecar to save"))?;
    sc.pipeline.stages = stages;
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
    // ONE export code path (session 08): the standalone path the S6
    // batch worker also runs — stages in, report + honest warnings out.
    // Routing is preview-consistent (disabled film → LUT leg; disabled
    // LUTs skipped) — see export.rs for the two M2/M3 inconsistencies
    // this fixed.
    let st = state.lock().unwrap();
    let Some(session) = st.session.as_ref() else {
        return Err(AppError::new("session.none", "no image is open"));
    };
    let stages = st.current_stages();
    let spec = crate::export::ExportSpec {
        format: args.format.clone(),
        depth: args.depth,
        quality: args.quality,
    };
    let out = crate::export::run_export(&crate::export::StandaloneExport {
        source: &session.source_path,
        dest: Path::new(&dest),
        stages: &stages,
        spec: &spec,
        library_root: &st.library_root(),
        profiles_dir: &st.stocks_dir,
    })?;
    let mut report = out.report;
    if !out.warnings.is_empty() {
        // Honest state (UX_SPEC §9): e.g. LUT stages skipped because the
        // film chain is active — surfaced, never silent.
        report["warnings"] = serde_json::json!(out.warnings);
    }
    Ok(report)
}

// EditorState helper (single place that knows the root).
impl EditorState {
    pub fn library_root(&self) -> PathBuf {
        // The LutLibrary was constructed with this root; mirror it.
        self.library.root().to_path_buf()
    }

    /// Build the preview film leg from the current FilmState.
    pub fn film_leg(&self) -> Option<render::FilmLeg> {
        let stock = self.film.stock.as_ref()?;
        if !self.film.adjustments.enabled {
            return None;
        }
        let params = melography_film::chain::FilmChainParams::new(stock.clone())
            .ok()?
            .with_adjustments(self.film.adjustments.clone(), self.film.lens)
            .ok()?;
        Some(render::FilmLeg {
            params,
            key: render::film_leg_key(&stock.id, &self.film.adjustments),
        })
    }
}

// ─── Film Lab (M2, issues #10/#11) ──────────────────────────────────────────

#[derive(Serialize)]
pub struct StockJson {
    pub id: String,
    pub display_name: String,
    pub provenance: String,
    pub confidence: String,
    pub polarity: String,
    pub layers: String,
    pub default_path: String,
    /// Honest label per UX_SPEC §5: artist-authored stocks say so
    /// prominently ("no measured calibration").
    pub provenance_text: String,
}

#[tauri::command]
pub fn list_stocks(state: St) -> Result<Vec<StockJson>, AppError> {
    let st = state.lock().unwrap();
    Ok(st
        .stocks
        .iter()
        .map(|p| StockJson {
            id: p.id.clone(),
            display_name: p.display_name.clone(),
            provenance: format!("{:?}", p.provenance),
            confidence: format!("{:?}", p.confidence),
            polarity: format!("{:?}", p.polarity),
            layers: format!("{:?}", p.layers),
            default_path: format!("{:?}", p.print.default_path),
            provenance_text: p.provenance_text.clone(),
        })
        .collect())
}

#[derive(Serialize)]
pub struct FilmStateJson {
    pub stock: Option<String>,
    pub enabled: bool,
    pub adjustments: melography_film::Adjustments,
    /// M4 #23: the Lens panel surface (Vignette / Veiling; CA +
    /// distortion stay declared-rejected — FILM_MODEL §B honesty).
    pub lens: melography_film::LensParams,
}

#[tauri::command]
pub fn get_film(state: St) -> Result<FilmStateJson, AppError> {
    let st = state.lock().unwrap();
    Ok(FilmStateJson {
        stock: st.film.stock.as_ref().map(|p| p.id.clone()),
        enabled: st.film.adjustments.enabled,
        adjustments: st.film.adjustments.clone(),
        lens: st.film.lens,
    })
}

#[derive(Deserialize)]
pub struct SetFilmArgs {
    /// Stock id; `null` disables the film chain.
    pub stock: Option<String>,
    #[serde(default)]
    pub adjustments: Option<melography_film::Adjustments>,
    /// Lens stage (M4 #23): vignette / veiling / fov / σ. Absent = keep.
    #[serde(default)]
    pub lens: Option<melography_film::LensParams>,
}

#[tauri::command]
pub fn set_film(state: St, args: SetFilmArgs) -> Result<FilmStateJson, AppError> {
    let mut st = state.lock().unwrap();
    st.push_history(); // recipe mutation → undo-able (M4 #24)
    match &args.stock {
        None => st.film.stock = None,
        Some(id) => {
            let stock = st
                .stocks
                .iter()
                .find(|p| &p.id == id)
                .cloned()
                .ok_or_else(|| {
                    AppError::new("film.unknown_stock", format!("unknown stock {id:?}"))
                })?;
            st.film.stock = Some(stock);
        }
    }
    if let Some(adj) = args.adjustments {
        adj.validate()
            .map_err(|e| AppError::new("film.invalid_adjustments", e))?;
        st.film.adjustments = adj;
    }
    if let Some(lens) = args.lens {
        lens.validate()
            .map_err(|e| AppError::new("film.invalid_lens", e))?;
        st.film.lens = lens;
    }
    st.dirty = true;
    Ok(FilmStateJson {
        stock: st.film.stock.as_ref().map(|p| p.id.clone()),
        enabled: st.film.adjustments.enabled,
        adjustments: st.film.adjustments.clone(),
        lens: st.film.lens,
    })
}

// ─── Tone stage + auto-lighting (M3, issue #13) ─────────────────────────────

#[derive(Serialize, Clone)]
pub struct ToneStateJson {
    /// None = no tone stage (identity).
    pub params: Option<ToneParams>,
    pub source: Option<SuggestionSource>,
    /// Display estimates for the Color panel's Temp/Tint sliders (the
    /// parameters stay cone gains — Kelvin is a UI projection;
    /// `color::tone` documents the round-trip tolerances).
    pub cct_estimate_k: Option<f32>,
    pub tint_estimate: Option<f32>,
}

fn tone_state_json(tone: &crate::state::ToneState) -> ToneStateJson {
    ToneStateJson {
        params: tone.params,
        source: tone.source.clone(),
        cct_estimate_k: tone
            .params
            .as_ref()
            .and_then(|p| p.wb.as_ref())
            .and_then(melography_color::tone::estimate_cct_k),
        tint_estimate: tone
            .params
            .as_ref()
            .and_then(|p| p.wb.as_ref())
            .and_then(melography_color::tone::estimate_tint),
    }
}

#[tauri::command]
pub fn get_tone(state: St) -> Result<ToneStateJson, AppError> {
    let st = state.lock().unwrap();
    Ok(tone_state_json(&st.tone))
}

#[derive(Deserialize)]
pub struct SetToneArgs {
    /// `None` removes the tone stage.
    pub params: Option<ToneParams>,
    /// Provenance when the params come from an accepted suggestion.
    #[serde(default)]
    pub source: Option<SuggestionSource>,
}

#[tauri::command]
pub fn set_tone(state: St, args: SetToneArgs) -> Result<ToneStateJson, AppError> {
    let mut st = state.lock().unwrap();
    st.push_history(); // recipe mutation → undo-able (M4 #24)
    let params = args.params.map(|p| p.sanitized());
    st.tone = crate::state::ToneState {
        params,
        source: args.source,
    };
    st.dirty = true;
    Ok(tone_state_json(&st.tone))
}

/// Color panel (M4 #23): (Temperature K, Tint −100..100) → WB cone gains
/// — the UX_SPEC §6 contract, synthesized in Rust so the UI never
/// reimplements color math (ADR-0001 invariant, parameters included).
#[tauri::command]
pub fn wb_from_cct_tint(cct_k: f32, tint: f32) -> melography_color::tone::WbGains {
    melography_color::tone::gains_from_cct_tint(cct_k, tint)
}

/// WB eyedropper (UX_SPEC §6 Color panel, session 08): pick a pixel in
/// the viewport → gains that make that region neutral. Coordinates are
/// NORMALIZED (0..1, top-left origin) so the UI never needs to know the
/// preview's working resolution. The gains come from the same
/// `analysis_gains_for_gray` math as auto-lighting — one WB path, and
/// the pick is undo-able like every recipe mutation.
#[tauri::command]
pub fn wb_pick(state: St, nx: f64, ny: f64) -> Result<ToneStateJson, AppError> {
    let mut st = state.lock().unwrap();
    let Some(session) = st.session.as_ref() else {
        return Err(AppError::new("session.none", "no image is open"));
    };
    let (w, h) = (session.width.max(1), session.height.max(1));
    let x = (nx.clamp(0.0, 1.0) * f64::from(w - 1)).round() as u32;
    let y = (ny.clamp(0.0, 1.0) * f64::from(h - 1)).round() as u32;
    let gains = crate::render::wb_gains_for_region(&session.ap1, x, y, 2);
    st.push_history(); // recipe mutation → undo-able (M4 #24)
    let mut params = st.tone.params.unwrap_or_default();
    params.wb = Some(gains);
    st.tone = crate::state::ToneState {
        params: Some(params),
        source: None, // a hand pick is manual provenance
    };
    st.dirty = true;
    Ok(tone_state_json(&st.tone))
}

/// Baseline auto-lighting suggestion (deterministic, model-free — the
/// quality bar, ADR-0004 §1a). Analysis runs on a ≤512 px proxy.
#[derive(Serialize)]
pub struct SuggestionJson {
    pub params: ToneParams,
    /// Machine-readable notes the UI localizes.
    pub notes: Vec<String>,
    pub wb_cct_estimate_k: Option<f32>,
    pub engine: String,
    pub engine_version: String,
    pub analysis_ms: f64,
}

#[tauri::command]
pub fn auto_light_suggest(state: St) -> Result<SuggestionJson, AppError> {
    let st = state.lock().unwrap();
    let session = st
        .session
        .as_ref()
        .ok_or_else(|| AppError::new("session.none", "no image is open"))?;
    let t = std::time::Instant::now();
    let s = melography_ai::auto_light::suggest_proxy(&session.ap1, 512);
    Ok(SuggestionJson {
        params: s.params,
        notes: s.notes.iter().map(|n| format!("{n:?}")).collect(),
        wb_cct_estimate_k: s.wb_cct_estimate_k,
        engine: melography_ai::auto_light::BASELINE_ID.to_string(),
        engine_version: melography_ai::auto_light::BASELINE_VERSION.to_string(),
        analysis_ms: t.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Fresh provenance block for an accepted suggestion (called by the UI on
/// Accept; the timestamp is taken here so it is honest server-side).
#[tauri::command]
pub fn suggestion_provenance(kind: String, name: String, version: String) -> SuggestionSource {
    SuggestionSource {
        kind: if kind == "model" {
            SuggestionKind::Model
        } else {
            SuggestionKind::Baseline
        },
        name,
        version,
        weights_sha256: None,
        accepted_utc: now_rfc3339(),
    }
}

pub(crate) fn now_rfc3339() -> String {
    // Minimal UTC RFC3339 without a chrono dependency (days-from-civil
    // algorithm; the provenance stamp needs second precision only).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
    let z = (days as i64) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

// ─── model manager (M3, issues #13/#14) ─────────────────────────────────────

fn models_root(st: &EditorState) -> PathBuf {
    st.library_root().join("models")
}

#[tauri::command]
pub fn list_models(state: St) -> Result<Vec<melography_ai::manager::ModelInfo>, AppError> {
    let st = state.lock().unwrap();
    let mgr = melography_ai::manager::ModelManager::open(models_root(&st));
    Ok(mgr.list())
}

/// Offline install from a local manifest + weights file (the owner-side
/// evaluation path; the network downloader ships with the first
/// gate-passing model — see manager.rs docs).
#[tauri::command]
pub fn install_model_from_file(
    state: St,
    manifest_path: String,
    weights_path: String,
) -> Result<Vec<melography_ai::manager::ModelInfo>, AppError> {
    let st = state.lock().unwrap();
    let manifest: melography_ai::ModelManifest = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path)
            .map_err(|e| AppError::new("model.manifest_read", e.to_string()))?,
    )
    .map_err(|e| AppError::new("model.manifest_parse", e.to_string()))?;
    let mut mgr = melography_ai::manager::ModelManager::open(models_root(&st));
    let cancel = melography_core::CancelToken::default();
    let mut fetch = melography_ai::manager::FileFetch {
        path: PathBuf::from(weights_path),
    };
    mgr.install(manifest, &mut fetch, &cancel)
        .map_err(|e| AppError::new("model.install", e.to_string()))?;
    Ok(mgr.list())
}

#[tauri::command]
pub fn delete_model(
    state: St,
    id: String,
) -> Result<Vec<melography_ai::manager::ModelInfo>, AppError> {
    let st = state.lock().unwrap();
    let mut mgr = melography_ai::manager::ModelManager::open(models_root(&st));
    mgr.delete(&id)
        .map_err(|e| AppError::new("model.delete", e.to_string()))?;
    Ok(mgr.list())
}

// Keep the Path import used (install path strings come from the dialog).
#[allow(dead_code)]
fn _path_anchor(_p: &Path) {}

// ─── Settings (M4 #23, UX_SPEC §7 S7) ───────────────────────────────────────

#[tauri::command]
pub fn get_settings(state: St) -> Result<crate::settings::Settings, AppError> {
    let st = state.lock().unwrap();
    Ok(st.settings.clone())
}

/// Partial update: the payload is the same `Settings` shape, but any
/// field the UI omits keeps its current value (serde default → sentinel
/// via `merge`), then the whole document persists durably.
#[tauri::command]
pub fn set_settings(
    state: St,
    settings: crate::settings::Settings,
) -> Result<crate::settings::Settings, AppError> {
    let mut st = state.lock().unwrap();
    st.settings = settings;
    st.settings
        .save(&st.library_root().join("settings.json"))
        .map_err(|e| AppError::new("settings.save", e.to_string()))?;
    // Cache budgets are live (#25): a changed cache_budget_mb applies to
    // both caches immediately (evicting/dropping as needed — cache.rs).
    let total = u64::from(st.settings.cache_budget_mb) * 1024 * 1024;
    st.preview_cache.set_budget(total);
    st.luts_loaded
        .set_budget(crate::cache::lut_budget_from_total(total));
    Ok(st.settings.clone())
}

/// Cache stats for the S2 status bar + Settings diagnostics (#25):
/// hit-rates, budgets, residency — every number measured, none faked.
#[tauri::command]
pub fn cache_stats(state: St) -> serde_json::Value {
    let st = state.lock().unwrap();
    let p = &st.preview_cache;
    let l = &st.luts_loaded;
    let rate = |hits: u64, misses: u64| -> f64 {
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    };
    let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
    serde_json::json!({
        "preview": {
            "hits": p.hits,
            "misses": p.misses,
            "hit_rate_pct": rate(p.hits, p.misses),
            "budget_mb": mib(p.budget_bytes),
            "cached_mb": mib(p.cached_bytes()),
            "cached": p.slot.is_some(),
        },
        "luts": {
            "hits": l.hits,
            "misses": l.misses,
            "hit_rate_pct": rate(l.hits, l.misses),
            "budget_mb": mib(l.budget_bytes()),
            "resident_mb": mib(l.resident_bytes()),
            "entries": l.len(),
        },
    })
}

// ─── Recents (M4 #23, UX_SPEC §3 S1 Home) ───────────────────────────────────

#[derive(Serialize)]
pub struct RecentJson {
    pub path: String,
    pub name: String,
    pub last_opened_utc: String,
    pub width: u32,
    pub height: u32,
    pub has_recipe: bool,
    /// Computed at list time (not stored — files move between runs):
    /// UX_SPEC §3 "broken file = inline error row".
    pub missing: bool,
    /// `thumb://`-style inline data URL the webview can bind directly.
    pub thumb_data_url: Option<String>,
}

#[tauri::command]
pub fn list_recents(state: St) -> Result<Vec<RecentJson>, AppError> {
    let st = state.lock().unwrap();
    let thumbs = st.library_root().join("thumbs");
    Ok(st
        .recents
        .entries
        .iter()
        .map(|e| {
            let thumb_data_url = e
                .thumb
                .as_ref()
                .and_then(|n| crate::recents::Recents::thumb_bytes(&thumbs, n))
                .map(|png| {
                    let mut url = String::with_capacity(png.len() * 4 / 3 + 32);
                    url.push_str("data:image/png;base64,");
                    base64_encode(&png, &mut url);
                    url
                });
            RecentJson {
                path: e.path.clone(),
                name: e.name.clone(),
                last_opened_utc: e.last_opened_utc.clone(),
                width: e.width,
                height: e.height,
                has_recipe: e.has_recipe,
                missing: !Path::new(&e.path).is_file(),
                thumb_data_url,
            }
        })
        .collect())
}

/// Remove recents whose originals are gone (the "clear broken" action;
/// returns how many were dropped).
#[tauri::command]
pub fn purge_recents(state: St) -> Result<u32, AppError> {
    let mut st = state.lock().unwrap();
    let store = st.library_root().join("recents.json");
    let n = st.recents.purge_missing(&store);
    Ok(n as u32)
}

/// Tiny standard base64 (no dependency for ~30 lines; encode-only — the
/// app never decodes). Used for inline thumb data URLs.
pub(crate) fn base64_encode(data: &[u8], out: &mut String) {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
}

// ─── User presets (M4 #23, UX_SPEC §2 S5 Preset manager) ────────────────────

pub(crate) fn presets_dir(st: &EditorState) -> PathBuf {
    st.library_root().join("presets")
}

#[derive(Serialize)]
pub struct PresetJson {
    pub id: String,
    pub name: String,
    pub created_utc: String,
    pub modified_utc: String,
    pub source: String,
    /// Honest summary line (e.g. "warm-400-negative + tone + 1 LUT") —
    /// built from the stages, not user-typed marketing.
    pub summary: String,
}

fn preset_summary(stages: &[melography_presets::sidecar::StageEntry]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for s in stages {
        match s {
            melography_presets::sidecar::StageEntry::FilmStock { stock, .. } => {
                parts.push(stock.clone())
            }
            melography_presets::sidecar::StageEntry::Tone { .. } => parts.push("tone".into()),
            melography_presets::sidecar::StageEntry::Lut { .. } => parts.push("LUT".into()),
            melography_presets::sidecar::StageEntry::FilmIdentity { .. } => {}
        }
    }
    if parts.is_empty() {
        "identity".to_string()
    } else {
        parts.join(" + ")
    }
}

fn presets_list(st: &EditorState) -> Vec<PresetJson> {
    crate::user_presets::UserPreset::list(&presets_dir(st))
        .into_iter()
        .map(|p| PresetJson {
            id: p.id,
            name: p.name,
            created_utc: p.created_utc,
            modified_utc: p.modified_utc,
            source: p.source,
            summary: preset_summary(&p.stages),
        })
        .collect()
}

#[tauri::command]
pub fn list_presets(state: St) -> Result<Vec<PresetJson>, AppError> {
    let st = state.lock().unwrap();
    Ok(presets_list(&st))
}

/// Save the CURRENT recipe as a named preset (S5 "save from current").
#[tauri::command]
pub fn save_preset(state: St, name: String) -> Result<PresetJson, AppError> {
    let name = crate::user_presets::sanitize_name(&name);
    if name == "preset" || name.trim().is_empty() {
        return Err(AppError::new("preset.name", "a real name is required"));
    }
    let st = state.lock().unwrap();
    let preset = crate::user_presets::UserPreset::new(&name, st.current_stages())
        .store_new(&presets_dir(&st));
    let preset = preset.map_err(|e| AppError::new("preset.save", e.to_string()))?;
    Ok(PresetJson {
        id: preset.id,
        name: preset.name,
        created_utc: preset.created_utc,
        modified_utc: preset.modified_utc,
        source: preset.source,
        summary: preset_summary(&preset.stages),
    })
}

#[tauri::command]
pub fn rename_preset(state: St, id: String, name: String) -> Result<Vec<PresetJson>, AppError> {
    let name = crate::user_presets::sanitize_name(&name);
    let st = state.lock().unwrap();
    let dir = presets_dir(&st);
    let mut p = crate::user_presets::UserPreset::find(&dir, &id)
        .ok_or_else(|| AppError::new("preset.missing", format!("no preset {id:?}")))?;
    p.name = name;
    p.modified_utc = melography_presets::utc_now_rfc3339();
    p.save(&dir)
        .map_err(|e| AppError::new("preset.save", e.to_string()))?;
    Ok(presets_list(&st))
}

#[tauri::command]
pub fn delete_preset(state: St, id: String) -> Result<Vec<PresetJson>, AppError> {
    let st = state.lock().unwrap();
    if !crate::user_presets::UserPreset::delete(&presets_dir(&st), &id) {
        return Err(AppError::new("preset.missing", format!("no preset {id:?}")));
    }
    Ok(presets_list(&st))
}

/// Apply a preset to the open session (undo-able like every recipe
/// mutation; the whole preset = one history entry).
#[tauri::command]
pub fn apply_preset(state: St, id: String) -> Result<StateJson, AppError> {
    let mut st = state.lock().unwrap();
    if st.session.is_none() {
        return Err(AppError::new("session.none", "open an image first"));
    }
    let p = crate::user_presets::UserPreset::find(&presets_dir(&st), &id)
        .ok_or_else(|| AppError::new("preset.missing", format!("no preset {id:?}")))?;
    st.push_history(); // one preset = one undo step (M4 #24 semantics)
    st.apply_pipeline_stages(&p.stages);
    st.dirty = true;
    st.preview_cache.slot = None; // keep budget + counters (#25)
    Ok(state_json(&st))
}

// ─── Filmstrip (M4 #23, UX_SPEC §4) ─────────────────────────────────────────

/// One stock thumbnail: the CURRENT photo through THAT stock's pipeline
/// (default adjustments), as an inline PNG data URL — the filmstrip's
/// "live stock thumbnails" contract. Small enough (≤ ~160 px) that base64
/// in one JSON payload beats N binary round-trips.
#[derive(Serialize)]
pub struct StockThumbJson {
    pub stock_id: String,
    pub thumb_data_url: String,
}

#[tauri::command]
pub fn stock_thumbs(state: St, max_dim: u32) -> Result<Vec<StockThumbJson>, AppError> {
    let st = state.lock().unwrap();
    let Some(session) = st.session.as_ref() else {
        return Ok(Vec::new()); // no session = empty filmstrip, not an error
    };
    let max_dim = max_dim.clamp(48, 256);
    let mut out = Vec::with_capacity(st.stocks.len());
    for stock in &st.stocks {
        // Default-adjustments film leg for this stock (the same resolve
        // path the preview uses, with factory knobs).
        let defaults = melography_film::Adjustments::default();
        let leg = melography_film::chain::FilmChainParams::new(stock.clone())
            .ok()
            .and_then(|c| {
                c.with_adjustments(defaults.clone(), melography_film::LensParams::default())
                    .ok()
            })
            .map(|params| render::FilmLeg {
                params,
                key: render::film_leg_key(&stock.id, &defaults),
            });
        let Some(leg) = leg else {
            continue; // honest skip: a stock that cannot resolve has no thumb
        };
        let frame = render::compose_preview(
            &session.ap1,
            // Never touch the live preview cache — a zero-budget slot
            // renders without retaining anything (#25).
            &mut crate::cache::PreviewCacheState::new(0),
            &[],
            Some(&leg),
            st.tone.params.as_ref(),
            max_dim,
        )
        .map_err(AppError::from)?;
        let mut url = String::with_capacity(frame.rgba.len() * 4 / 3 + 32);
        url.push_str("data:image/png;base64,");
        // RGBA → RGB over the chrome dark (same composite as recents).
        let mut rgb = Vec::with_capacity(frame.rgba.len() / 4 * 3);
        for px in frame.rgba.as_chunks::<4>().0 {
            let a = f32::from(px[3]) / 255.0;
            for v in &px[..3] {
                let v = f32::from(*v) * a + 14.0 * (1.0 - a);
                rgb.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
        // Encode via a temp file (the one encoder code path), then read back.
        let tmp = std::env::temp_dir().join(format!("melography-thumb-{}.png", std::process::id()));
        if melography_io::encode::encode_png8(&tmp, frame.width, frame.height, &rgb, None).is_ok() {
            if let Ok(bytes) = std::fs::read(&tmp) {
                base64_encode(&bytes, &mut url);
                out.push(StockThumbJson {
                    stock_id: stock.id.clone(),
                    thumb_data_url: url,
                });
            }
            let _ = std::fs::remove_file(&tmp);
        }
    }
    Ok(out)
}
