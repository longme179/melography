//! Editor session state (single-window M1: one open image + its recipe +
//! the LUT library). Behind a `Mutex` managed by Tauri; commands lock,
//! mutate, release — renders are fast enough at M1 scale to stay
//! synchronous (async + worker pool arrives with tiled M2 renders).

use melography_color::tone::ToneParams;
use melography_core::ImageF32;
use melography_film::profile::StockProfile;
use melography_film::{Adjustments, LensParams};
use melography_lut::stage::LutApply;
use melography_lut::LutLibrary;
use melography_presets::sidecar::{Fingerprint, SidecarV1, StageEntry, SuggestionSource};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cache::{lut_budget_from_total, LutCache, PreviewCacheState};
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
    /// Loaded-and-verified LUTs by content hash — LRU with a byte budget
    /// (#25; previously an unbounded HashMap). Budget = quarter of
    /// `Settings.cache_budget_mb`, enforced by eviction.
    pub luts_loaded: LutCache,
    /// The user's current LUT stack (recipe serialization mirrors this).
    pub lut_stack: Vec<LutApply>,
    pub sidecar: Option<SidecarV1>,
    pub sidecar_path: Option<PathBuf>,
    /// True when the stack changed since the last sidecar save.
    pub dirty: bool,
    /// Full-res render slot + budget + hit/miss counters (#25; the
    /// budget comes from `Settings.cache_budget_mb` and is enforced —
    /// an image larger than the budget is rendered but not retained).
    pub preview_cache: PreviewCacheState,
    /// Undo stack (pre-mutation snapshots, bounded — see UNDO_LIMIT).
    pub history: Vec<RecipeSnapshot>,
    /// Redo stack (cleared by any new mutation — standard undo semantics).
    pub redo: Vec<RecipeSnapshot>,
    /// App settings (UX_SPEC §7) — loaded at startup, saved on change.
    pub settings: crate::settings::Settings,
    /// A2.4 GPU runtime: the preview film-leg dispatch + the honest
    /// degradation state (synced from `settings` at load and on change).
    pub gpu: crate::gpu::GpuRuntime,
    /// Recent files (UX_SPEC §3 S1 Home) — loaded at startup.
    pub recents: crate::recents::Recents,
    /// Warning surface: a sidecar/preset referenced a stock id that is
    /// not in the registry (downgrade or missing profiles). Surfaced as
    /// an amber badge — errors are never silently skipped (UX_SPEC §9).
    pub missing_stock: Option<String>,
}

impl EditorState {
    pub fn new(library_root: PathBuf) -> Self {
        let settings_path = library_root.join("settings.json");
        let (settings, settings_warning) = crate::settings::Settings::load(&settings_path);
        if let Some(w) = settings_warning {
            eprintln!("melography: {w} — using defaults");
        }
        let recents = crate::recents::Recents::load(&library_root.join("recents.json"));
        // A2.4: the GPU runtime mirrors the loaded settings (enabled +
        // adapter policy) before the struct takes ownership.
        let mut gpu = crate::gpu::GpuRuntime::new();
        gpu.sync_from_settings(&settings);
        // Cache budgets (#25): preview slot = the setting; LUT LRU = a
        // quarter share (see cache.rs for the policy + rationale).
        let total_budget = u64::from(settings.cache_budget_mb) * 1024 * 1024;
        Self {
            session: None,
            library: LutLibrary::open(&library_root),
            stocks: Vec::new(),
            stocks_dir: PathBuf::new(),
            film: FilmState::default(),
            tone: ToneState::default(),
            luts_loaded: LutCache::new(lut_budget_from_total(total_budget)),
            lut_stack: Vec::new(),
            sidecar: None,
            sidecar_path: None,
            dirty: false,
            preview_cache: PreviewCacheState::new(total_budget),
            history: Vec::new(),
            redo: Vec::new(),
            settings,
            gpu,
            recents,
            missing_stock: None,
        }
    }

    /// The recipe as sidecar-shaped stages — the single serialization of
    /// "what is applied right now" (save_recipe and save_preset both
    /// build from this; presets and sidecars can never drift).
    pub fn current_stages(&self) -> Vec<StageEntry> {
        let mut stages = Vec::with_capacity(3 + self.lut_stack.len());
        match &self.film.stock {
            Some(stock) => stages.push(StageEntry::FilmStock {
                stock: stock.id.clone(),
                stock_version: stock.profile_version,
                enabled: self.film.adjustments.enabled,
                adjustments: self.film.adjustments.clone(),
                lens: self.film.lens,
                extra: Default::default(),
            }),
            None => stages.push(StageEntry::FilmIdentity {
                enabled: true,
                extra: Default::default(),
            }),
        }
        if let Some(tp) = self.tone.params {
            stages.push(StageEntry::Tone {
                params: tp,
                source: self.tone.source.clone(),
                enabled: true,
                extra: Default::default(),
            });
        }
        for a in &self.lut_stack {
            stages.push(StageEntry::from(a));
        }
        stages
    }

    /// Apply sidecar-shaped stages to the live state — the ONE stage→state
    /// code path, shared by sidecar resume (`open_image`), preset apply
    /// (`apply_preset`) and any future import. A missing stock id sets
    /// `missing_stock` (amber badge) and leaves the film chain off rather
    /// than failing the whole apply — the rest of the recipe still lands.
    pub fn apply_pipeline_stages(&mut self, stages: &[StageEntry]) {
        self.missing_stock = None;
        self.film = FilmState::default();
        self.tone = ToneState::default();
        self.luts_loaded.clear(); // resolve_stack re-loads lazily
        let mut lut_stack = Vec::new();
        for s in stages {
            match s {
                StageEntry::FilmIdentity { .. } => {}
                StageEntry::FilmStock {
                    stock,
                    adjustments,
                    lens,
                    enabled,
                    ..
                } => match self.stocks.iter().find(|p| &p.id == stock) {
                    Some(profile) => {
                        self.film = FilmState {
                            stock: Some(profile.clone()),
                            adjustments: adjustments.clone(),
                            lens: *lens,
                        };
                        self.film.adjustments.enabled = *enabled;
                    }
                    None => {
                        self.missing_stock = Some(stock.clone());
                    }
                },
                StageEntry::Tone {
                    params,
                    source,
                    enabled,
                    ..
                } => {
                    self.tone = crate::state::ToneState {
                        params: if *enabled { Some(*params) } else { None },
                        source: source.clone(),
                    };
                }
                StageEntry::Lut { .. } => {
                    if let Ok(apply) = <&StageEntry as TryInto<LutApply>>::try_into(s) {
                        lut_stack.push(apply);
                    }
                }
            }
        }
        self.lut_stack = lut_stack;
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
    /// Loaded LUTs land in the budgeted LRU (`cache.rs`, #25).
    pub fn resolve_stack(&mut self) -> Result<Vec<render::ResolvedLut>, AppError> {
        let mut out = Vec::with_capacity(self.lut_stack.len());
        for apply in &self.lut_stack {
            if !apply.enabled {
                continue;
            }
            let lut = match self.luts_loaded.get(&apply.lut_hash) {
                Some(l) => l,
                None => {
                    let l = Arc::new(self.library.load(&apply.lut_hash)?);
                    self.luts_loaded.insert(apply.lut_hash.clone(), l.clone())
                }
            };
            out.push(render::ResolvedLut {
                apply: apply.clone(),
                lut,
            });
        }
        Ok(out)
    }
}
