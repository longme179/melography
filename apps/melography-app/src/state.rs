//! Editor session state (single-window M1: one open image + its recipe +
//! the LUT library). Behind a `Mutex` managed by Tauri; commands lock,
//! mutate, release — renders are fast enough at M1 scale to stay
//! synchronous (async + worker pool arrives with tiled M2 renders).

use melography_color::tone::ToneParams;
use melography_core::ImageF32;
use melography_film::profile::StockProfile;
use melography_film::{Adjustments, LensParams};
use melography_lut::stage::LutApply;
use melography_lut::{CubeLut, LutLibrary};
use melography_presets::sidecar::{Fingerprint, SidecarV1, SuggestionSource};
use render::RenderCache;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::AppError;
use crate::render;

/// Everything about the currently open original (immutable source — the
/// non-destructive contract: the file is never written to).
pub struct OpenSession {
    pub source_path: PathBuf,
    pub original_name: String,
    /// Full-resolution AP1-linear working base (post S1).
    pub ap1: ImageF32,
    pub width: u32,
    pub height: u32,
    pub fingerprint: Fingerprint,
}

/// Film Lab state (M2, issue #10/#11 UX legs): the selected stock (data,
/// not code — loaded from the shipped registry) + recipe-level knobs.
/// `None` stock = film chain off (identity), mirroring the CLI default.
#[derive(Clone, Default)]
pub struct FilmState {
    pub stock: Option<StockProfile>,
    pub adjustments: Adjustments,
    pub lens: LensParams,
}

/// Tone stage state (M3, issue #13): scene-referred WB/EV/HL-SH/shaping
/// — the landing zone for auto-lighting suggestions. `None` = no stage.
#[derive(Clone, Default)]
pub struct ToneState {
    pub params: Option<ToneParams>,
    /// Provenance of the last accepted suggestion (sidecar-persisted).
    pub source: Option<SuggestionSource>,
}

/// A recipe-level snapshot for undo/redo (UX_SPEC §4 History: "infinite
/// within session, bounded persistence" — here: bounded in-memory stack,
/// UX polish M4 issue #24). Everything `save_recipe` rebuilds the sidecar
/// from is captured; the library and the loaded-LUT cache are NOT recipe
/// state (library mutations are their own explicit actions).
#[derive(Clone)]
pub struct RecipeSnapshot {
    pub film: FilmState,
    pub tone: ToneState,
    pub lut_stack: Vec<LutApply>,
}

/// Undo depth. "Infinite within session" is approximated by a bound far
/// beyond realistic interactive editing; the oldest entry is dropped
/// (bounded memory — the same honesty as the LRU caches).
pub const UNDO_LIMIT: usize = 100;

pub struct EditorState {
    pub session: Option<OpenSession>,
    pub library: LutLibrary,
    /// Shipped stock registry (loaded once at startup, strict — a broken
    /// profile file fails app startup loudly, never silently).
    pub stocks: Vec<StockProfile>,
    pub stocks_dir: PathBuf,
    pub film: FilmState,
    /// M3 tone stage (issue #13).
    pub tone: ToneState,
    /// Loaded-and-verified LUTs by content hash (resolution cache).
    pub luts_loaded: HashMap<String, Arc<CubeLut>>,
    /// The user's current LUT stack (recipe serialization mirrors this).
    pub lut_stack: Vec<LutApply>,
    pub sidecar: Option<SidecarV1>,
    pub sidecar_path: Option<PathBuf>,
    /// True when the stack changed since the last sidecar save.
    pub dirty: bool,
    pub preview_cache: Option<RenderCache>,
    /// Undo stack (pre-mutation snapshots, bounded — see UNDO_LIMIT).
    pub history: Vec<RecipeSnapshot>,
    /// Redo stack (cleared by any new mutation — standard undo semantics).
    pub redo: Vec<RecipeSnapshot>,
}

impl EditorState {
    pub fn new(library_root: PathBuf) -> Self {
        Self {
            session: None,
            library: LutLibrary::open(&library_root),
            stocks: Vec::new(),
            stocks_dir: PathBuf::new(),
            film: FilmState::default(),
            tone: ToneState::default(),
            luts_loaded: HashMap::new(),
            lut_stack: Vec::new(),
            sidecar: None,
            sidecar_path: None,
            dirty: false,
            preview_cache: None,
            history: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Capture the current recipe state.
    pub fn snapshot(&self) -> RecipeSnapshot {
        RecipeSnapshot {
            film: self.film.clone(),
            tone: self.tone.clone(),
            lut_stack: self.lut_stack.clone(),
        }
    }

    /// Record the pre-mutation state. Call BEFORE applying a recipe
    /// change; any new mutation invalidates the redo branch.
    pub fn push_history(&mut self) {
        self.redo.clear();
        self.history.push(self.snapshot());
        if self.history.len() > UNDO_LIMIT {
            self.history.remove(0);
        }
    }

    /// Restore a snapshot (undo/redo path). Marks dirty so the next
    /// render recomputes; the preview cache key changes with the recipe.
    pub fn restore(&mut self, s: RecipeSnapshot) {
        self.film = s.film;
        self.tone = s.tone;
        self.lut_stack = s.lut_stack;
        self.dirty = true;
    }

    /// Session-scope reset (open/close image): history belongs to the
    /// editing session of one image.
    pub fn reset_history(&mut self) {
        self.history.clear();
        self.redo.clear();
    }

    /// Resolve the stack against the library (hash-verified loads; missing
    /// or corrupted entries surface as explicit errors — never skipped).
    pub fn resolve_stack(&mut self) -> Result<Vec<render::ResolvedLut>, AppError> {
        let mut out = Vec::with_capacity(self.lut_stack.len());
        for apply in &self.lut_stack {
            if !apply.enabled {
                continue;
            }
            let lut = if let Some(l) = self.luts_loaded.get(&apply.lut_hash) {
                l.clone()
            } else {
                let l = Arc::new(self.library.load(&apply.lut_hash)?);
                self.luts_loaded.insert(apply.lut_hash.clone(), l.clone());
                l
            };
            out.push(render::ResolvedLut {
                apply: apply.clone(),
                lut,
            });
        }
        Ok(out)
    }
}
