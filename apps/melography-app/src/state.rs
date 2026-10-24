//! Editor session state (single-window M1: one open image + its recipe +
//! the LUT library). Behind a `Mutex` managed by Tauri; commands lock,
//! mutate, release — renders are fast enough at M1 scale to stay
//! synchronous (async + worker pool arrives with tiled M2 renders).

use melography_core::ImageF32;
use melography_film::profile::StockProfile;
use melography_film::{Adjustments, LensParams};
use melography_lut::stage::LutApply;
use melography_lut::{CubeLut, LutLibrary};
use melography_presets::sidecar::{Fingerprint, SidecarV1};
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

pub struct EditorState {
    pub session: Option<OpenSession>,
    pub library: LutLibrary,
    /// Shipped stock registry (loaded once at startup, strict — a broken
    /// profile file fails app startup loudly, never silently).
    pub stocks: Vec<StockProfile>,
    pub stocks_dir: PathBuf,
    pub film: FilmState,
    /// Loaded-and-verified LUTs by content hash (resolution cache).
    pub luts_loaded: HashMap<String, Arc<CubeLut>>,
    /// The user's current LUT stack (recipe serialization mirrors this).
    pub lut_stack: Vec<LutApply>,
    pub sidecar: Option<SidecarV1>,
    pub sidecar_path: Option<PathBuf>,
    /// True when the stack changed since the last sidecar save.
    pub dirty: bool,
    pub preview_cache: Option<RenderCache>,
}

impl EditorState {
    pub fn new(library_root: PathBuf) -> Self {
        Self {
            session: None,
            library: LutLibrary::open(&library_root),
            stocks: Vec::new(),
            stocks_dir: PathBuf::new(),
            film: FilmState::default(),
            luts_loaded: HashMap::new(),
            lut_stack: Vec::new(),
            sidecar: None,
            sidecar_path: None,
            dirty: false,
            preview_cache: None,
        }
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
