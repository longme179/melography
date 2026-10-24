//! Standalone export path (session 08, #23 part 2): ONE export code
//! path shared by the single-image export command and the S6 batch
//! worker. Input is sidecar-shaped [`StageEntry`] stages — the same
//! single serialization `current_stages()` builds and user presets
//! store — so a batch render and a single export can never drift.
//!
//! Routing is PREVIEW-CONSISTENT (this module fixes two real M2/M3
//! single-export inconsistencies, see session log 08):
//!
//! * a film stock with `enabled == false` now routes to the LUT/tone
//!   leg, exactly like `EditorState::film_leg()` (the old export took
//!   the film path, which silently dropped the LUT stack);
//! * a disabled LUT entry is skipped, exactly like `resolve_stack()`
//!   (the old export applied the first LUT even when disabled).
//!
//! When the film chain IS active, LUT stages cannot ride along yet
//! (insertion-point plumbing is future work) — that is surfaced as the
//! [`WARN_LUT_SKIPPED_BY_FILM`] warning, never a silent drop.

use crate::error::AppError;
use melography_cli::{ApplyFormat, ApplyOptions, FilmApplyOptions};
use melography_film::{Adjustments, LensParams, StockProfile};
use melography_lut::stage::LutApply;
use melography_presets::sidecar::StageEntry;
use std::path::{Path, PathBuf};

/// Honest warning (UX_SPEC §9): LUT stages present while the film chain
/// is active — they are skipped by the film apply path today. The UI
/// catalogs localize the code; the batch queue shows it per row.
pub const WARN_LUT_SKIPPED_BY_FILM: &str = "export.lut_skipped_by_film";

/// Export settings a job carries (retry keeps them — UX_SPEC §7).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExportSpec {
    /// "png" | "jpeg" | "tiff" (parsed by the CLI's `ApplyFormat`).
    pub format: String,
    /// 8 or 16 (JPEG is always 8 — per-format factual, never silent).
    pub depth: u32,
    pub quality: u8,
}

impl ExportSpec {
    pub fn extension(&self) -> &'static str {
        match self.format.as_str() {
            "jpeg" => "jpg",
            "tiff" => "tiff",
            _ => "png",
        }
    }
}

/// The film leg of a resolved recipe (id + recipe-level knobs).
#[derive(Clone)]
pub struct FilmSpec {
    pub stock_id: String,
    pub adjustments: Adjustments,
    pub lens: LensParams,
}

/// Stages resolved to their live form (pure — unit-tested).
#[derive(Default)]
pub struct ResolvedRecipe {
    /// Present when a FilmStock stage exists (even disabled — the flag
    /// rides along and routing decides).
    pub film: Option<FilmSpec>,
    /// Present when an enabled Tone stage exists.
    pub tone: Option<melography_color::tone::ToneParams>,
    /// All LUT entries in stack order (enabled flags preserved).
    pub luts: Vec<LutApply>,
}

/// Resolve sidecar-shaped stages to the live recipe form. Pure: no I/O,
/// no state — the same stages a preset or a sidecar resume carries.
pub fn resolve_stages(stages: &[StageEntry]) -> ResolvedRecipe {
    let mut r = ResolvedRecipe::default();
    for s in stages {
        match s {
            StageEntry::FilmStock {
                stock,
                adjustments,
                lens,
                enabled,
                ..
            } => {
                if r.film.is_none() {
                    // The STAGE's enabled flag is the effective one —
                    // the same override `apply_pipeline_stages` writes
                    // into `adjustments.enabled` (one apply path).
                    let mut adj = adjustments.clone();
                    adj.enabled = *enabled;
                    r.film = Some(FilmSpec {
                        stock_id: stock.clone(),
                        adjustments: adj,
                        lens: *lens,
                    });
                }
            }
            StageEntry::Tone {
                params, enabled, ..
            } => {
                if *enabled && r.tone.is_none() {
                    r.tone = Some(*params);
                }
            }
            StageEntry::Lut { .. } => {
                if let Ok(apply) = <&StageEntry as TryInto<LutApply>>::try_into(s) {
                    r.luts.push(apply);
                }
            }
            StageEntry::FilmIdentity { .. } => {}
        }
    }
    r
}

/// Does this recipe reference a stock id that is not in the registry?
/// Batch add surfaces this per job (honest error, never a wrong-color
/// render) — the same honesty `apply_pipeline_stages` gives the editor.
pub fn missing_stock_id(stages: &[StageEntry], registry: &[StockProfile]) -> Option<String> {
    let r = resolve_stages(stages);
    match &r.film {
        Some(f) if !registry.iter().any(|p| p.id == f.stock_id) => Some(f.stock_id.clone()),
        _ => None,
    }
}

/// A self-contained export request (no editor session required — the
/// batch worker's input).
pub struct StandaloneExport<'a> {
    pub source: &'a Path,
    pub dest: &'a Path,
    pub stages: &'a [StageEntry],
    pub spec: &'a ExportSpec,
    pub library_root: &'a Path,
    pub profiles_dir: &'a Path,
}

/// What a finished export reports back (report + honest warnings +
/// measured facts for the queue row).
pub struct ExportOutcome {
    pub report: serde_json::Value,
    pub warnings: Vec<&'static str>,
    pub dims: (u32, u32),
    pub bytes: u64,
    pub total_ms: f64,
}

/// Run the export through the shared headless pipeline (the CLI lib —
/// the same code the CLI/CI exercise, ARCHITECTURE §2).
pub fn run_export(e: &StandaloneExport) -> Result<ExportOutcome, AppError> {
    let format = ApplyFormat::parse(&e.spec.format)?;
    let r = resolve_stages(e.stages);
    // Film leg runs only when a stock is selected AND enabled — the
    // exact `film_leg()` semantics (fixes the disabled-film export lie).
    let film_active = r.film.as_ref().is_some_and(|f| f.adjustments.enabled);

    let mut warnings = Vec::new();
    let report = if film_active {
        let f = r.film.as_ref().expect("film_active checked Some");
        if r.luts.iter().any(|l| l.enabled) {
            warnings.push(WARN_LUT_SKIPPED_BY_FILM);
        }
        let opts = FilmApplyOptions {
            input: e.source.to_path_buf(),
            output: e.dest.to_path_buf(),
            stock: f.stock_id.clone(),
            profiles_dir: Some(e.profiles_dir.to_path_buf()),
            adjustments: f.adjustments.clone(),
            lens: f.lens,
            format,
            depth: e.spec.depth,
            quality: e.spec.quality,
            strip_h: 512,
            // Provenance rides with the file (P2: "apply with
            // provenance") — consistent with the LUT leg below.
            write_sidecar: true,
            tone: r.tone,
        };
        melography_cli::cmd_film_apply(&opts)
    } else {
        // LUT leg: the FIRST ENABLED entry — `resolve_stack()` semantics
        // (fixes the disabled-LUT export lie).
        let lut = r.luts.iter().find(|l| l.enabled);
        let lut_path = match lut {
            Some(a) => {
                let blob = e
                    .library_root
                    .join("luts")
                    .join(format!("{}.cube", a.lut_hash));
                if blob.exists() {
                    Some(blob)
                } else {
                    return Err(AppError::new(
                        "lut.missing",
                        format!("LUT {} is missing from the library", a.lut_hash),
                    )
                    .with_context(a.lut_hash.clone()));
                }
            }
            None => None,
        };
        let mut opts = ApplyOptions::simple(e.source.to_path_buf(), e.dest.to_path_buf());
        opts.format = format;
        opts.depth = e.spec.depth;
        opts.quality = e.spec.quality;
        opts.library_root = e.library_root.to_path_buf();
        opts.tone = r.tone;
        if let (Some(a), Some(path)) = (lut, lut_path.as_ref()) {
            opts.lut = Some(path.to_path_buf());
            opts.lut_in = a.declared_input;
            opts.lut_out = a.declared_output;
            opts.intensity = a.intensity;
            opts.insertion = a.insertion;
        }
        melography_cli::cmd_apply(&opts)
    };

    if let Some(err) = report.get("error") {
        let msg = err.as_str().unwrap_or("export failed").to_string();
        return Err(map_export_error(&msg));
    }
    let dims = report
        .get("dims")
        .and_then(|d| d.as_array())
        .and_then(|a| {
            let w = a.first()?.as_u64()?;
            let h = a.get(1)?.as_u64()?;
            Some((w as u32, h as u32))
        })
        .unwrap_or((0, 0));
    let bytes = std::fs::metadata(e.dest).map(|m| m.len()).unwrap_or(0);
    let total_ms = report
        .pointer("/timings_ms/total")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Ok(ExportOutcome {
        report,
        warnings,
        dims,
        bytes,
        total_ms,
    })
}

/// Map a pipeline error string to the stable code + actionable hint the
/// UI catalogs localize (ARCHITECTURE §8 taxonomy). String-based because
/// the CLI report is the common denominator both legs share.
pub fn map_export_error(msg: &str) -> AppError {
    let m = msg.to_ascii_lowercase();
    if m.contains("no space left") {
        AppError::new("io.disk_full", msg.to_string()).with_context("hint.chooseFolder")
    } else if m.contains("permission denied") || m.contains("read-only") {
        AppError::new("io.permission", msg.to_string()).with_context("hint.checkPermissions")
    } else if m.contains("unknown stock")
        // Stock-registry failures: profile load/parse/validation errors
        // name a .json file; a missing/unreadable registry dir names the
        // profiles path. Both mean "the recipe's stock cannot render".
        || m.contains(".json")
        || (m.contains("profiles") && m.contains("i/o"))
    {
        AppError::new("stock.missing", msg.to_string()).with_context("hint.restoreStock")
    } else if m.contains("lut") && (m.contains("missing") || m.contains("checksum")) {
        AppError::new("lut.missing", msg.to_string()).with_context("hint.reimportLut")
    } else if m.contains("decode") || m.contains("unsupported") || m.contains("i/o error reading") {
        AppError::new("io.decode", msg.to_string()).with_context("hint.fileUnsupported")
    } else {
        AppError::new("export.failed", msg.to_string())
    }
}

/// Collision-safe output path inside `dir`: `<stem>-melography.<ext>`,
/// then `-2`, `-3`, … A file is NEVER overwritten silently (P2's top
/// fear — "unexplained clipping" of someone else's work included).
/// The worker is strictly sequential, so probe-then-write is race-free.
pub fn reserve_output_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}-melography.{ext}"));
    if !first.exists() {
        return first;
    }
    for i in 2..100u32 {
        let cand = dir.join(format!("{stem}-melography-{i}.{ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    // Pathological stem (99 collisions): time suffix, still no overwrite.
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("{stem}-melography-{t}.{ext}"))
}

/// Output stem from a source file name (extension stripped).
pub fn output_stem(source: &Path) -> String {
    source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_stage(enabled: bool) -> StageEntry {
        StageEntry::FilmStock {
            stock: "warm-400-negative".into(),
            stock_version: 1,
            enabled,
            adjustments: Adjustments {
                enabled,
                ..Default::default()
            },
            lens: LensParams::default(),
            extra: Default::default(),
        }
    }

    #[test]
    fn resolve_film_disabled_keeps_flag() {
        let stages = vec![stock_stage(false)];
        let r = resolve_stages(&stages);
        let f = r.film.expect("film stage present");
        assert!(!f.adjustments.enabled, "flag must ride along");
    }

    #[test]
    fn film_active_only_when_enabled() {
        let r = resolve_stages(&[stock_stage(false)]);
        assert!(!r.film.as_ref().is_some_and(|f| f.adjustments.enabled));
        let r = resolve_stages(&[stock_stage(true)]);
        assert!(r.film.as_ref().is_some_and(|f| f.adjustments.enabled));
    }

    #[test]
    fn missing_stock_detected() {
        let stages = vec![stock_stage(true)];
        let reg: Vec<StockProfile> = Vec::new(); // empty registry
        assert_eq!(
            missing_stock_id(&stages, &reg),
            Some("warm-400-negative".to_string())
        );
    }

    #[test]
    fn error_mapping_is_actionable() {
        assert_eq!(
            map_export_error("write failed: No space left on device").code,
            "io.disk_full"
        );
        assert_eq!(
            map_export_error("open(/x/y.png): Permission denied").code,
            "io.permission"
        );
        assert_eq!(
            map_export_error("unknown stock id foo (known: bar)").code,
            "stock.missing"
        );
        assert_eq!(map_export_error("LUT abc is missing").code, "lut.missing");
        assert_eq!(
            map_export_error("decode failed: unsupported bit depth").code,
            "io.decode"
        );
        assert_eq!(map_export_error("something odd").code, "export.failed");
        // Hints ride in `context` (the UI catalog keys on them).
        assert_eq!(
            map_export_error("No space left on device")
                .context
                .as_deref(),
            Some("hint.chooseFolder")
        );
    }

    #[test]
    fn reserved_paths_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let stem = "photo";
        let a = reserve_output_path(dir.path(), stem, "png");
        assert_eq!(
            a.file_name().unwrap().to_string_lossy(),
            "photo-melography.png"
        );
        std::fs::write(&a, b"x").unwrap();
        let b = reserve_output_path(dir.path(), stem, "png");
        assert_eq!(
            b.file_name().unwrap().to_string_lossy(),
            "photo-melography-2.png"
        );
        std::fs::write(&b, b"x").unwrap();
        let c = reserve_output_path(dir.path(), stem, "png");
        assert_eq!(
            c.file_name().unwrap().to_string_lossy(),
            "photo-melography-3.png"
        );
        assert_ne!(a, c, "never the same path as an existing file");
    }

    #[test]
    fn output_stem_strips_extension() {
        assert_eq!(output_stem(Path::new("/tmp/IMG_001.JPG")), "IMG_001");
        assert_eq!(output_stem(Path::new("weird")), "weird");
    }

    /// End-to-end on a real tiny PNG through the shared pipeline: no
    /// film, no LUT — the identity convert the queue must handle.
    #[test]
    fn run_export_identity_png() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.png");
        // 3×2 sRGB PNG via the io encoder (one encoder, one code path).
        let rgb: Vec<u8> = vec![128; 3 * 2 * 3];
        melography_io::encode::encode_png8(&src, 3, 2, &rgb, None).unwrap();
        let dest = dir.path().join("out.png");
        let spec = ExportSpec {
            format: "png".into(),
            depth: 8,
            quality: 92,
        };
        let lib = dir.path().join("library");
        let profiles = dir.path().join("profiles");
        let out = run_export(&StandaloneExport {
            source: &src,
            dest: &dest,
            stages: &[StageEntry::FilmIdentity {
                enabled: true,
                extra: Default::default(),
            }],
            spec: &spec,
            library_root: &lib,
            profiles_dir: &profiles,
        })
        .expect("identity export");
        assert!(dest.exists(), "output written");
        assert_eq!(out.dims, (3, 2));
        assert!(out.bytes > 0);
        assert!(out.total_ms >= 0.0);
        assert!(out.warnings.is_empty());
        // Provenance rides along: the sidecar lands next to the output.
        assert!(dest.with_extension("melography.json").exists());
    }
}
