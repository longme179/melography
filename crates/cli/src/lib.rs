//! `melography-cli` — headless pipeline (ARCHITECTURE §1: "the CLI is a
//! first-class citizen — benchmark, golden tests, CI").
//!
//! M1 adds the LUT workflow E2E slice (issue #7/#8/#9 exit criteria in
//! scriptable form):
//! * [`cmd_lut_parse`] — strict `.cube` validation report,
//! * [`cmd_apply`] — decode → AP1 → (library-import → hash-verified load →
//!   encoding-wrapped LUT application with intensity/insertion) → export
//!   JPEG/PNG/TIFF (+ICC where the format supports it) → sidecar v1 with
//!   fingerprint + LUT-by-hash,
//! * [`cmd_sidecar`] — load/migrate/inspect a sidecar.
//!
//! Commands are library functions (not buried in `main.rs`) so integration
//! tests and CI drive the exact user-facing paths.

use melography_color as mcolor;
use melography_color::tone::ToneParams;
use melography_core::{Channels, ImageF32};
use melography_film::FilmChain;
use melography_io as mio;
use melography_lut as mlut;
use melography_lut::stage::{InsertionPoint, LutApply};
use melography_lut::{LutEncoding, LutLibrary};
use melography_presets as mpresets;
use melography_presets::sidecar::{Fingerprint, SidecarV1, StageEntry};
use std::path::{Path, PathBuf};
use std::time::Instant;

// ── shared helpers ──────────────────────────────────────────────────────────

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

pub fn srgb_icc_bytes() -> Vec<u8> {
    mcolor::icc_bytes_deterministic(&mcolor::srgb_profile())
}

fn luma_to_u8(img: &ImageF32) -> Vec<u8> {
    // Apply the sRGB OETF (display convention for 8-bit outputs) then
    // quantize; dithering is an app-layer option (CP §6), not CLI default.
    img.data()
        .iter()
        .map(|&c| {
            let v = mcolor::srgb_tf::linear_to_srgb(c.clamp(0.0, 1.0));
            (v * 255.0).round() as u8
        })
        .collect()
}

// ── chart / roundtrip / golden / bench / codecs (M0 commands, unchanged
//    behavior except: roundtrip now records sidecar v1) ─────────────────────

pub fn cmd_chart(out: &Path) -> serde_json::Value {
    let (w, h, px) = mio::chart::synthetic_chart_u16();
    mio::encode::encode_png16(out, w, h, &px, Some(&srgb_icc_bytes())).expect("chart encode");
    serde_json::json!({
        "command": "chart",
        "out": out.display().to_string(),
        "width": w,
        "height": h,
        "sha256_png": sha256_hex(&std::fs::read(out).expect("read back")),
    })
}

/// S0+S1: decode a file to AP1-linear (fast path for sRGB-untagged, lcms2
/// otherwise). Shared by `roundtrip` and `apply`.
fn decode_to_ap1(
    input: &Path,
    via_lcms: bool,
) -> Result<(mio::decode::DecodedImage, ImageF32, bool, f64, f64), String> {
    let t = Instant::now();
    let decoded = mio::decode::decode_file(input).map_err(|e| format!("decode: {e}"))?;
    let decode_ms = t.elapsed().as_secs_f64() * 1000.0;

    let encoding = match &decoded.encoding {
        melography_core::ColorEncoding::Icc { profile } => melography_core::ColorEncoding::Icc {
            profile: profile.clone(),
        },
        _ => melography_core::ColorEncoding::Srgb { linear: false },
    };
    let fastpath_ok = matches!(
        encoding,
        melography_core::ColorEncoding::Srgb { .. } | melography_core::ColorEncoding::Untagged
    );
    let use_lcms = via_lcms || !fastpath_ok;

    let t = Instant::now();
    let ap1 = if use_lcms {
        mcolor::to_ap1_lcms(&decoded.pixels, &encoding).map_err(|e| format!("lcms S1: {e}"))?
    } else {
        mcolor::srgb_to_ap1_fast(&decoded.pixels)
    };
    let s1_ms = t.elapsed().as_secs_f64() * 1000.0;
    Ok((decoded, ap1, use_lcms, decode_ms, s1_ms))
}

pub fn cmd_roundtrip(input: &Path, out: &Path, via_lcms: bool) -> serde_json::Value {
    let t_total = Instant::now();
    let (decoded, ap1, use_lcms, decode_ms, s1_ms) =
        decode_to_ap1(input, via_lcms).expect("roundtrip decode");

    let t = Instant::now();
    let chain = FilmChain::new();
    let film_out = chain.apply(&ap1).expect("film chain");
    let film_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let u16px = if use_lcms {
        let back = mcolor::ap1_to_srgb_lcms(&film_out).expect("lcms S5");
        back.data()
            .iter()
            .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
            .collect()
    } else {
        mcolor::ap1_to_srgb_u16_fast(&film_out)
    };
    let s5_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    mio::encode::encode_png16(
        out,
        film_out.width(),
        film_out.height(),
        &u16px,
        Some(&srgb_icc_bytes()),
    )
    .expect("encode");
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;

    let input_u16: Vec<u16> = decoded
        .pixels
        .data()
        .iter()
        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
        .collect();
    let max_err_16 = input_u16
        .iter()
        .zip(&u16px)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .unwrap_or(0);
    let passed = max_err_16 <= 128;

    // M1: sidecar v1 (fingerprint + identity film stage; ARCHITECTURE §7).
    let file_bytes = std::fs::read(input).expect("read input bytes");
    let fingerprint = Fingerprint::compute(&file_bytes, &decoded.pixels);
    let mut sidecar = SidecarV1::new(
        input
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        Some(fingerprint),
    );
    sidecar.image.input_encoding = match &decoded.encoding {
        melography_core::ColorEncoding::Icc { profile } => mpresets::InputEncodingRecord::Icc {
            sha256: sha256_hex(profile),
        },
        _ => mpresets::InputEncodingRecord::UntaggedSrgb,
    };
    sidecar.pipeline.stages.push(StageEntry::FilmIdentity {
        enabled: true,
        extra: Default::default(),
    });
    let sidecar_path = input.with_extension("melography.json");
    sidecar.save(&sidecar_path).expect("sidecar write");

    serde_json::json!({
        "command": "roundtrip",
        "input": input.display().to_string(),
        "output": out.display().to_string(),
        "via_lcms": use_lcms,
        "dims": [decoded.pixels.width(), decoded.pixels.height()],
        "timings_ms": {
            "decode": decode_ms,
            "s1_to_ap1": s1_ms,
            "film_identity": film_ms,
            "s5_to_srgb": s5_ms,
            "encode_png16": encode_ms,
            "total": t_total.elapsed().as_secs_f64() * 1000.0,
        },
        "hashes": {
            "ap1_f32": mcolor::hash_image(&ap1),
            "output_png": sha256_hex(&std::fs::read(out).expect("read back")),
            "sidecar_v1": sha256_hex(sidecar.to_json().expect("sidecar json").as_bytes()),
        },
        "sidecar": sidecar_path.display().to_string(),
        "verify": {
            "max_abs_err_16bit": max_err_16,
            "threshold_16bit": 128,
            "passed": passed,
        },
    })
}

pub fn cmd_golden() -> serde_json::Value {
    let chart = mio::chart::synthetic_chart();
    let fast_ap1 = mcolor::srgb_to_ap1_fast(&chart);
    let lcms_ap1 = mcolor::to_ap1_lcms(
        &chart,
        &melography_core::ColorEncoding::Srgb { linear: false },
    )
    .expect("lcms S1 on chart");
    let roundtrip = mcolor::ap1_to_srgb_u16_fast(&fast_ap1);
    let _ = lcms_ap1;
    serde_json::json!({
        "command": "golden",
        "fast_ap1_chart": mcolor::hash_image(&fast_ap1),
        "roundtrip_chart": sha256_hex(bytemuck::cast_slice(&roundtrip)),
    })
}

pub fn cmd_bench(mp: u32) -> serde_json::Value {
    let chart = mio::chart::synthetic_chart();
    let _ = mp;
    serde_json::json!({
        "command": "bench",
        "note": "see PLAN.md §4 for the M0-measured numbers; per-mp bench returns at M2",
        "chart_hash": mcolor::hash_image(&chart),
    })
}

// ── M1: LUT commands ────────────────────────────────────────────────────────

/// `melography lut parse <file>` — strict validation report (no library I/O).
pub fn cmd_lut_parse(path: &Path) -> serde_json::Value {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return serde_json::json!({
                "command": "lut-parse",
                "file": path.display().to_string(),
                "valid": false,
                "error": {"line": 0, "kind": format!("cannot read file: {e}")},
            })
        }
    };
    let src = String::from_utf8_lossy(&bytes);
    match mlut::cube::parse_cube(&src) {
        Ok(lut) => serde_json::json!({
            "command": "lut-parse",
            "file": path.display().to_string(),
            "valid": true,
            "content_hash": sha256_hex(&bytes),
            "kind": format!("{:?}", lut.kind),
            "size": lut.size,
            "entries": lut.entries(),
            "domain_min": lut.domain_min,
            "domain_max": lut.domain_max,
            "title": lut.title,
            "warnings": lut.warnings,
        }),
        Err(e) => serde_json::json!({
            "command": "lut-parse",
            "file": path.display().to_string(),
            "valid": false,
            "error": { "line": e.line, "kind": format!("{}", e.kind) },
        }),
    }
}

/// Default library root: `$MELOGRAPHY_DATA`, else `<home>/.melography`
/// (portable, documented; the app uses the OS data dir — Tauri resolves it).
pub fn default_library_root() -> PathBuf {
    if let Ok(dir) = std::env::var("MELOGRAPHY_DATA") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".melography")
}

/// Output format for `apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyFormat {
    Png,
    Jpeg,
    Tiff,
}

impl ApplyFormat {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            "tiff" | "tif" => Ok(Self::Tiff),
            other => Err(format!("unknown format {other:?} (png|jpeg|tiff)")),
        }
    }
}

/// Full option set for [`cmd_apply`] (also drives the app's export dialog).
#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// `.cube` path; when absent the apply is a plain identity export.
    pub lut: Option<PathBuf>,
    pub lut_in: LutEncoding,
    pub lut_out: LutEncoding,
    pub intensity: f32,
    pub insertion: InsertionPoint,
    pub format: ApplyFormat,
    /// 8 or 16 (JPEG is always 8 — per-format factual, never silent).
    pub depth: u32,
    pub quality: u8,
    pub library_root: PathBuf,
    pub via_lcms: bool,
    /// Write the sidecar next to the output (default true).
    pub write_sidecar: bool,
    /// M3 tone stage (scene-referred WB/EV/HL-SH/shaping), applied after
    /// decode→AP1 and before LUT/film. `None` = no tone stage.
    pub tone: Option<ToneParams>,
}

impl ApplyOptions {
    pub fn simple(input: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        Self {
            input: input.into(),
            output: output.into(),
            lut: None,
            lut_in: LutEncoding::SrgbDisplay,
            lut_out: LutEncoding::SrgbDisplay,
            intensity: 1.0,
            insertion: InsertionPoint::Finishing,
            format: ApplyFormat::Png,
            depth: 16,
            quality: 92,
            library_root: default_library_root(),
            via_lcms: false,
            write_sidecar: true,
            tone: None,
        }
    }
}

/// The M1 end-to-end slice: import → verify → wrap-encode → LUT → mix →
/// export + sidecar. Returns the JSON report (also printed by the CLI).
pub fn cmd_apply(opts: &ApplyOptions) -> serde_json::Value {
    let t_total = Instant::now();
    let (decoded, ap1, use_lcms, decode_ms, s1_ms) = match decode_to_ap1(&opts.input, opts.via_lcms)
    {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"command": "apply", "error": e});
        }
    };

    // M3 tone stage (scene-referred) — first thing after S1, before LUT/film.
    let (ap1, tone_report, tone_stage) = match &opts.tone {
        Some(t) => {
            let t0 = Instant::now();
            let toned = mcolor::tone::apply(&ap1, t);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let report = serde_json::json!({"params": t, "apply_ms": ms});
            (
                toned,
                report,
                Some(StageEntry::Tone {
                    params: *t,
                    source: None,
                    enabled: true,
                    extra: Default::default(),
                }),
            )
        }
        None => (ap1.clone(), serde_json::json!(null), None),
    };

    // Optional LUT: strict-parse the file, import content-addressed, then
    // load it back BY HASH (the library re-verifies the checksum — this is
    // the exact path the app uses, never a shortcut).
    let mut lut_report = serde_json::json!(null);
    let mut sidecar_lut_stage: Option<StageEntry> = None;
    let working = if let Some(lut_path) = &opts.lut {
        let lib = LutLibrary::open(&opts.library_root);
        let record = match lib.import(lut_path, opts.lut_in, opts.lut_out, Default::default()) {
            Ok(r) => r,
            Err(e) => {
                return serde_json::json!({
                    "command": "apply",
                    "error": format!("LUT import {}: {e}", lut_path.display()),
                    "lut_file": lut_path.display().to_string(),
                });
            }
        };
        let lut = match lib.load(&record.content_hash) {
            Ok(l) => l,
            Err(e) => {
                return serde_json::json!({
                    "command": "apply", "error": format!("LUT load: {e}"),
                });
            }
        };
        let stage = LutApply::new(
            record.content_hash.clone(),
            opts.insertion,
            opts.intensity,
            opts.lut_in,
            opts.lut_out,
        );
        let t = Instant::now();
        let (out_img, report) = match stage.apply(&lut, &ap1) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::json!({"command": "apply", "error": format!("LUT apply: {e}")});
            }
        };
        let lut_ms = t.elapsed().as_secs_f64() * 1000.0;
        lut_report = serde_json::json!({
            "file": lut_path.display().to_string(),
            "content_hash": record.content_hash,
            "kind": format!("{:?}", lut.kind),
            "size": lut.size,
            "insertion": format!("{:?}", opts.insertion),
            "intensity": opts.intensity,
            "declared": {
                "input": opts.lut_in.as_str(),
                "output": opts.lut_out.as_str(),
            },
            "clamped_pixels": report.clamped_pixels,
            "apply_ms": lut_ms,
        });
        sidecar_lut_stage = Some(StageEntry::from(&stage));
        out_img
    } else {
        ap1.clone()
    };

    // Identity film chain (M1; real stages at M2).
    let film_out = FilmChain::new().apply(&working).expect("film chain");

    // S5 + encode. Per-format factual capabilities (CP §6): JPEG is 8-bit
    // and gets an ICC APP2; PNG 8/16 + ICC; TIFF 16 (ICC tag write is a
    // documented gap — CODEC_MATRIX.md).
    let t = Instant::now();
    let encode_result: Result<(), String> = match (opts.format, opts.depth) {
        (ApplyFormat::Png, 8) => {
            let u8px = luma_to_u8(&film_out);
            mio::encode::encode_png8(
                &opts.output,
                film_out.width(),
                film_out.height(),
                &u8px,
                Some(&srgb_icc_bytes()),
            )
            .map_err(|e| format!("png8: {e}"))
        }
        (ApplyFormat::Png, 16) => {
            let u16px = if use_lcms {
                match mcolor::ap1_to_srgb_lcms(&film_out) {
                    Ok(back) => back
                        .data()
                        .iter()
                        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
                        .collect(),
                    Err(e) => {
                        return serde_json::json!({"command": "apply", "error": format!("lcms S5: {e}")});
                    }
                }
            } else {
                mcolor::ap1_to_srgb_u16_fast(&film_out)
            };
            mio::encode::encode_png16(
                &opts.output,
                film_out.width(),
                film_out.height(),
                &u16px,
                Some(&srgb_icc_bytes()),
            )
            .map_err(|e| format!("png16: {e}"))
        }
        (ApplyFormat::Jpeg, depth) => {
            if depth != 8 {
                Err(
                    "JPEG export is 8-bit only (per-format factual; use PNG/TIFF for 16-bit)"
                        .to_string(),
                )
            } else {
                let u8px = luma_to_u8(&film_out);
                mio::encode::encode_jpeg_u8_with_icc(
                    &opts.output,
                    film_out.width(),
                    film_out.height(),
                    &u8px,
                    opts.quality,
                    &srgb_icc_bytes(),
                )
                .map_err(|e| format!("jpeg: {e}"))
            }
        }
        (ApplyFormat::Tiff, depth) => {
            if depth != 16 {
                Err("TIFF export is 16-bit only in v1 (per-format factual)".to_string())
            } else {
                let u16px: Vec<u16> = if use_lcms {
                    match mcolor::ap1_to_srgb_lcms(&film_out) {
                        Ok(back) => back
                            .data()
                            .iter()
                            .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
                            .collect(),
                        Err(e) => {
                            return serde_json::json!({"command": "apply", "error": format!("lcms S5: {e}")});
                        }
                    }
                } else {
                    mcolor::ap1_to_srgb_u16_fast(&film_out)
                };
                mio::encode::encode_tiff_u16(
                    &opts.output,
                    film_out.width(),
                    film_out.height(),
                    &u16px,
                )
                .map_err(|e| format!("tiff16: {e}"))
            }
        }
        (_, depth) => Err(format!(
            "unsupported bit depth {depth} for this format (png: 8|16, jpeg: 8, tiff: 16)"
        )),
    };
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;
    if let Err(e) = encode_result {
        return serde_json::json!({"command": "apply", "error": e});
    }

    // Sidecar v1 with fingerprint + LUT stage (by content hash).
    let mut sidecar_sha = serde_json::json!(null);
    let mut sidecar_path = serde_json::json!(null);
    if opts.write_sidecar {
        let file_bytes = std::fs::read(&opts.input).unwrap_or_default();
        let fingerprint = Fingerprint::compute(&file_bytes, &decoded.pixels);
        let mut sc = SidecarV1::new(
            opts.input
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Some(fingerprint),
        );
        sc.image.input_encoding = match &decoded.encoding {
            melography_core::ColorEncoding::Icc { profile } => mpresets::InputEncodingRecord::Icc {
                sha256: sha256_hex(profile),
            },
            _ => mpresets::InputEncodingRecord::UntaggedSrgb,
        };
        sc.pipeline.stages.push(StageEntry::FilmIdentity {
            enabled: true,
            extra: Default::default(),
        });
        if let Some(stage) = sidecar_lut_stage {
            sc.pipeline.stages.push(stage);
        }
        if let Some(stage) = tone_stage {
            sc.pipeline.stages.push(stage);
        }
        let spath = opts.output.with_extension("melography.json");
        sc.save(&spath).expect("sidecar write");
        sidecar_sha = serde_json::json!(sha256_hex(sc.to_json().expect("sidecar json").as_bytes()));
        sidecar_path = serde_json::json!(spath.display().to_string());
    }

    serde_json::json!({
        "command": "apply",
        "input": opts.input.display().to_string(),
        "output": opts.output.display().to_string(),
        "format": format!("{:?}", opts.format),
        "depth": opts.depth,
        "via_lcms": use_lcms,
        "dims": [decoded.pixels.width(), decoded.pixels.height()],
        "lut": lut_report,
        "tone": tone_report,
        "timings_ms": {
            "decode": decode_ms,
            "s1_to_ap1": s1_ms,
            "total": t_total.elapsed().as_secs_f64() * 1000.0,
            "encode": encode_ms,
        },
        "hashes": {
            "ap1_f32": mcolor::hash_image(&ap1),
            "working_f32": mcolor::hash_image(&working),
            "output_file": sha256_hex(&std::fs::read(&opts.output).expect("read back")),
            "sidecar_v1": sidecar_sha,
        },
        "sidecar": sidecar_path,
    })
}

/// `melography sidecar <file>` — load (with migration), report structure.
pub fn cmd_sidecar(path: &Path) -> serde_json::Value {
    match SidecarV1::load(path) {
        Ok(sc) => serde_json::json!({
            "command": "sidecar",
            "file": path.display().to_string(),
            "version": sc.version,
            "schema": sc.schema,
            "image": {
                "original_name": sc.image.original_name,
                "fingerprint_present": sc.image.fingerprint.is_some(),
            },
            "stages": sc.pipeline.stages.iter().map(|s| match s {
                StageEntry::FilmIdentity { enabled, .. } =>
                    serde_json::json!({"stage": "film_identity", "enabled": enabled}),
                StageEntry::FilmStock { stock, stock_version, enabled, .. } =>
                    serde_json::json!({"stage": "film_stock", "stock": stock, "stock_version": stock_version, "enabled": enabled}),
                StageEntry::Tone { params, source, enabled, .. } =>
                    serde_json::json!({"stage": "tone", "params": params, "source": source, "enabled": enabled}),
                StageEntry::Lut { instance, insertion, intensity, declared_input, declared_output, enabled, .. } =>
                    serde_json::json!({
                        "stage": "lut", "instance": instance,
                        "insertion": format!("{:?}", insertion),
                        "intensity": intensity,
                        "declared_input": declared_input.as_str(),
                        "declared_output": declared_output.as_str(),
                        "enabled": enabled,
                    }),
            }).collect::<Vec<_>>(),
            "unknown_fields": sc.extra.len(),
        }),
        Err(e) => serde_json::json!({
            "command": "sidecar",
            "file": path.display().to_string(),
            "error": format!("{e}"),
        }),
    }
}

// keep Channels referenced for the luma helper's type domain
#[allow(dead_code)]
fn _ch(c: Channels) -> usize {
    c.count()
}

// ─── film (M2, issues #10/#11) ─────────────────────────────────────────────

/// Options for `film apply` (M2): decode → AP1 → full film chain → S5
/// export, with the recipe recorded in a sidecar v1 `film_stock` stage.
pub struct FilmApplyOptions {
    pub input: PathBuf,
    pub output: PathBuf,
    /// Stock profile id (registry lookup) or path to a profile JSON.
    pub stock: String,
    /// Registry directory; required for id lookups.
    pub profiles_dir: Option<PathBuf>,
    pub adjustments: melography_film::Adjustments,
    pub lens: melography_film::LensParams,
    pub format: ApplyFormat,
    pub depth: u32,
    pub quality: u8,
    /// Strip height for the tiled executor (0 = monolithic).
    pub strip_h: u32,
    pub write_sidecar: bool,
    /// M3 tone stage, applied before the film chain.
    pub tone: Option<ToneParams>,
}

/// Resolve a stock reference (id or file path) to a validated profile.
fn resolve_stock(
    stock: &str,
    profiles_dir: Option<&Path>,
) -> Result<(melography_film::StockProfile, String), String> {
    let as_path = Path::new(stock);
    if as_path.extension().map(|e| e == "json").unwrap_or(false) && as_path.is_file() {
        let p = mpresets::load_stock_profile(as_path).map_err(|e| e.to_string())?;
        let origin = as_path.display().to_string();
        return Ok((p, origin));
    }
    let dir = profiles_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("profiles"));
    let reg =
        mpresets::StockRegistry::open(&dir).map_err(|e| format!("{e} (dir: {})", dir.display()))?;
    let p = reg
        .by_id(stock)
        .ok_or_else(|| {
            let known: Vec<String> = reg.profiles.iter().map(|p| p.id.clone()).collect();
            format!("unknown stock id {stock:?} (known: {})", known.join(", "))
        })?
        .clone();
    Ok((p, format!("{}/{}", dir.display(), stock)))
}

/// `film list` — registry summaries (stock browser data source).
pub fn cmd_film_list(profiles_dir: Option<&Path>) -> serde_json::Value {
    let dir = profiles_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("profiles"));
    match mpresets::StockRegistry::open(&dir) {
        Ok(reg) => serde_json::json!({
            "command": "film list",
            "profiles_dir": dir.display().to_string(),
            "stocks": reg.summaries(),
        }),
        Err(e) => serde_json::json!({
            "command": "film list",
            "error": format!("{e} (dir: {})", dir.display()),
        }),
    }
}

/// `film apply` — the full M2 slice.
pub fn cmd_film_apply(opts: &FilmApplyOptions) -> serde_json::Value {
    let t_total = Instant::now();
    let (profile, origin) = match resolve_stock(&opts.stock, opts.profiles_dir.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"command": "film apply", "error": e});
        }
    };

    let (decoded, ap1, use_lcms, decode_ms, s1_ms) = match decode_to_ap1(&opts.input, false) {
        Ok(v) => v,
        Err(e) => {
            return serde_json::json!({"command": "film apply", "error": e});
        }
    };

    // M3 tone stage — scene-referred corrections precede the film chain.
    let (ap1, tone_report, tone_stage) = match &opts.tone {
        Some(t) => {
            let t0 = Instant::now();
            let toned = mcolor::tone::apply(&ap1, t);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let report = serde_json::json!({"params": t, "apply_ms": ms});
            (
                toned,
                report,
                Some(StageEntry::Tone {
                    params: *t,
                    source: None,
                    enabled: true,
                    extra: Default::default(),
                }),
            )
        }
        None => (ap1, serde_json::json!(null), None),
    };

    let params = match melography_film::chain::FilmChainParams::new(profile.clone())
        .and_then(|p| p.with_adjustments(opts.adjustments.clone(), opts.lens))
    {
        Ok(p) => p,
        Err(e) => {
            return serde_json::json!({"command": "film apply", "error": e.to_string()});
        }
    };
    let t = Instant::now();
    let film_out = if opts.strip_h > 0 {
        melography_film::chain::render_tiled(&ap1, &params, opts.strip_h)
    } else {
        melography_film::chain::render(&ap1, &params)
    };
    let film_out = match film_out {
        Ok(o) => o,
        Err(e) => {
            return serde_json::json!({"command": "film apply", "error": e.to_string()});
        }
    };
    let film_ms = t.elapsed().as_secs_f64() * 1000.0;

    // S5 + encode (same per-format factual capabilities as `apply`).
    let t = Instant::now();
    let encode_result: Result<(), String> = match (opts.format, opts.depth) {
        (ApplyFormat::Png, 8) => {
            let u8px = luma_to_u8(&film_out);
            mio::encode::encode_png8(
                &opts.output,
                film_out.width(),
                film_out.height(),
                &u8px,
                Some(&srgb_icc_bytes()),
            )
            .map_err(|e| format!("png8: {e}"))
        }
        (ApplyFormat::Png, 16) => {
            let u16px = if use_lcms {
                match mcolor::ap1_to_srgb_lcms(&film_out) {
                    Ok(back) => back
                        .data()
                        .iter()
                        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
                        .collect(),
                    Err(e) => {
                        return serde_json::json!({"command": "film apply", "error": format!("lcms S5: {e}")})
                    }
                }
            } else {
                mcolor::ap1_to_srgb_u16_fast(&film_out)
            };
            mio::encode::encode_png16(
                &opts.output,
                film_out.width(),
                film_out.height(),
                &u16px,
                Some(&srgb_icc_bytes()),
            )
            .map_err(|e| format!("png16: {e}"))
        }
        (ApplyFormat::Jpeg, depth) => {
            if depth != 8 {
                Err("JPEG export is 8-bit only (use PNG/TIFF for 16-bit)".to_string())
            } else {
                let u8px = luma_to_u8(&film_out);
                mio::encode::encode_jpeg_u8_with_icc(
                    &opts.output,
                    film_out.width(),
                    film_out.height(),
                    &u8px,
                    opts.quality,
                    &srgb_icc_bytes(),
                )
                .map_err(|e| format!("jpeg: {e}"))
            }
        }
        (ApplyFormat::Tiff, depth) => {
            if depth != 16 {
                Err("TIFF export is 16-bit only in v1".to_string())
            } else {
                let u16px = mcolor::ap1_to_srgb_u16_fast(&film_out);
                mio::encode::encode_tiff_u16(
                    &opts.output,
                    film_out.width(),
                    film_out.height(),
                    &u16px,
                )
                .map_err(|e| format!("tiff: {e}"))
            }
        }
        (ApplyFormat::Png, _) => Err("PNG depth must be 8 or 16".to_string()),
    };
    if let Err(e) = encode_result {
        return serde_json::json!({"command": "film apply", "error": e});
    }
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Sidecar v1 with the film_stock stage (data reference, not a copy).
    let mut sidecar_sha = serde_json::json!(null);
    if opts.write_sidecar {
        let file_bytes = std::fs::read(&opts.input).unwrap_or_default();
        let fingerprint = Fingerprint::compute(&file_bytes, &decoded.pixels);
        let mut sc = SidecarV1::new(
            opts.input
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Some(fingerprint),
        );
        sc.image.input_encoding = match &decoded.encoding {
            melography_core::ColorEncoding::Icc { profile } => mpresets::InputEncodingRecord::Icc {
                sha256: sha256_hex(profile),
            },
            _ => mpresets::InputEncodingRecord::UntaggedSrgb,
        };
        sc.pipeline.stages.push(StageEntry::FilmStock {
            stock: profile.id.clone(),
            stock_version: profile.profile_version,
            adjustments: opts.adjustments.clone(),
            lens: opts.lens,
            enabled: opts.adjustments.enabled,
            extra: Default::default(),
        });
        if let Some(stage) = tone_stage {
            sc.pipeline.stages.push(stage);
        }
        let spath = opts.output.with_extension("melography.json");
        sc.save(&spath).expect("sidecar write");
        sidecar_sha = serde_json::json!(sha256_hex(sc.to_json().expect("sidecar json").as_bytes()));
    }

    serde_json::json!({
        "command": "film apply",
        "input": opts.input.display().to_string(),
        "output": opts.output.display().to_string(),
        "stock": {
            "id": profile.id,
            "display_name": profile.display_name,
            "provenance": format!("{:?}", profile.provenance),
            "confidence": format!("{:?}", profile.confidence),
            "origin": origin,
        },
        "timings_ms": {
            "decode": decode_ms,
            "s1_ap1": s1_ms,
            "film_chain": film_ms,
            "encode": encode_ms,
            "total": t_total.elapsed().as_secs_f64() * 1000.0,
        },
        "strip_h": opts.strip_h,
        "format": format!("{:?}", opts.format),
        "tone": tone_report,
        "sidecar_sha256": sidecar_sha,
    })
}

// ── AI commands (M3, issues #13/#14) ────────────────────────────────────────

/// `ai suggest` — the non-ML auto-lighting baseline on a real image
/// (decode → AP1 → ≤512 px proxy → deterministic suggestion → JSON).
pub fn cmd_ai_suggest(input: &Path, proxy: u32) -> serde_json::Value {
    use melography_ai as mai;
    let t0 = Instant::now();
    let (_decoded, ap1, _use_lcms, _decode_ms, _s1_ms) = match decode_to_ap1(input, false) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({"command": "ai suggest", "error": e}),
    };
    let suggestion = mai::auto_light::suggest_proxy(&ap1, proxy);
    let analysis_ms = t0.elapsed().as_secs_f64() * 1000.0;
    serde_json::json!({
        "command": "ai suggest",
        "engine": {
            "id": mai::auto_light::BASELINE_ID,
            "version": mai::auto_light::BASELINE_VERSION,
            "kind": "baseline",
            "model_free": true,
        },
        "input": input.display().to_string(),
        "proxy_long_edge": proxy,
        "params": suggestion.params,
        "notes": suggestion.notes.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>(),
        "wb_cct_estimate_k": suggestion.wb_cct_estimate_k,
        "stats": {
            "p001": suggestion.stats.p001,
            "p005": suggestion.stats.p005,
            "p50": suggestion.stats.p50,
            "p99": suggestion.stats.p99,
            "p995": suggestion.stats.p995,
            "gray_rgb": suggestion.stats.gray_rgb,
            "skin_fraction": suggestion.stats.skin_fraction,
        },
        "analysis_ms": analysis_ms,
    })
}

/// `ai vet` — run the manifest vetting gate on a manifest JSON (issue #14:
/// the gate is a CLI-verifiable workflow, not a hidden constant).
pub fn cmd_ai_vet(manifest_path: &Path) -> serde_json::Value {
    use melography_ai as mai;
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(e) => {
            return serde_json::json!({
                "command": "ai vet",
                "error": format!("read {}: {e}", manifest_path.display())
            })
        }
    };
    let manifest: mai::ModelManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            return serde_json::json!({
                "command": "ai vet",
                "error": format!("manifest parse: {e}"),
                "manifest": manifest_path.display().to_string(),
            })
        }
    };
    match mai::vet(&manifest) {
        Ok(()) => serde_json::json!({
            "command": "ai vet",
            "manifest": manifest_path.display().to_string(),
            "id": manifest.id,
            "version": manifest.version.to_string(),
            "verdict": "pass",
        }),
        Err(e) => serde_json::json!({
            "command": "ai vet",
            "manifest": manifest_path.display().to_string(),
            "id": manifest.id,
            "version": manifest.version.to_string(),
            "verdict": "reject",
            "reasons": e.to_string(),
        }),
    }
}

/// `ai install` — offline install from a local weights file through the
/// gate + SHA-256 + atomic pipeline (FileFetch; AI_MODELS §6).
pub fn cmd_ai_install(manifest_path: &Path, weights: &Path, root: &Path) -> serde_json::Value {
    use melography_ai as mai;
    let text = match std::fs::read_to_string(manifest_path) {
        Ok(t) => t,
        Err(e) => {
            return serde_json::json!({"command": "ai install", "error": format!("read {}: {e}", manifest_path.display())})
        }
    };
    let manifest: mai::ModelManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            return serde_json::json!({"command": "ai install", "error": format!("manifest parse: {e}")})
        }
    };
    let mut mgr = mai::manager::ModelManager::open(root);
    let cancel = melography_core::CancelToken::default();
    let mut fetch = mai::manager::FileFetch {
        path: weights.to_path_buf(),
    };
    match mgr.install(manifest, &mut fetch, &cancel) {
        Ok(()) => serde_json::json!({
            "command": "ai install",
            "root": root.display().to_string(),
            "state": "ready",
        }),
        Err(e) => serde_json::json!({
            "command": "ai install",
            "root": root.display().to_string(),
            "error": e.to_string(),
        }),
    }
}

/// `ai models` — list the registry (states per AI_MODELS §6).
pub fn cmd_ai_models(root: &Path) -> serde_json::Value {
    use melography_ai as mai;
    let mgr = mai::manager::ModelManager::open(root);
    serde_json::json!({
        "command": "ai models",
        "root": root.display().to_string(),
        "models": mgr.list(),
    })
}

/// `ai delete` — remove a model and all its files immediately.
pub fn cmd_ai_delete(root: &Path, id: &str) -> serde_json::Value {
    use melography_ai as mai;
    let mut mgr = mai::manager::ModelManager::open(root);
    match mgr.delete(id) {
        Ok(()) => serde_json::json!({"command": "ai delete", "id": id, "deleted": true}),
        Err(e) => serde_json::json!({"command": "ai delete", "id": id, "error": e.to_string()}),
    }
}
