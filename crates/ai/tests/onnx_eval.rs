//! Issue #14 — ONNX parameter-predictor evaluation harness (feature `onnx`).
//!
//! Proves the full pipeline with the project-generated dummy predictor
//! (fixtures/dummy_param_predictor.onnx — a naive linear gray-world model,
//! CC0, see .provenance.json) and measures it against the non-ML baseline
//! on a synthetic corpus with known ground truth. **This is the benchmark
//! any real candidate must beat** (ADR-0004 §1b); the dummy is expected to
//! lose — that loss is the plumbing proof, not a failure.
//!
//! Metrics (per scene, after applying the suggestion as ToneParams):
//! * `ev_error`  — |log2( luma(neutral patch) / 0.18 )|  (mid-tone placement)
//! * `wb_error`  — mean channel spread of the neutral patch / 0.18
//! * `clip_rate` — fraction of pixels pushed above 1.0 (overshoot penalty)
//! * aggregate   — ev_error + 5·wb_error + clip_rate  (the benchmark score)
#![cfg(feature = "onnx")]

use melography_ai::auto_light::{self, Suggestion};
use melography_ai::manager::{FileFetch, ModelManager};
use melography_ai::onnx::{decode_params, OnnxPredictor};
use melography_ai::{vet, ModelManifest};
use melography_color::tone::{self, ToneParams};
use melography_core::{CancelToken, Channels, ImageF32};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const MANIFEST_JSON: &str = "dummy_param_predictor.manifest.json";
const WEIGHTS: &str = "dummy_param_predictor.onnx";

fn fixture_manifest() -> ModelManifest {
    let path = format!("{FIXTURE_DIR}/{MANIFEST_JSON}");
    let text = std::fs::read_to_string(&path).expect("fixture manifest present");
    serde_json::from_str(&text).expect("fixture manifest parses")
}

fn load_predictor() -> OnnxPredictor {
    let manifest = fixture_manifest();
    let weights = format!("{FIXTURE_DIR}/{WEIGHTS}");
    OnnxPredictor::from_manifest(std::path::Path::new(&weights), &manifest)
        .expect("fixture loads through the gate + sha check")
}

// ── synthetic corpus with ground truth ──────────────────────────────────────

/// A deterministic scene + the pixel indices of its neutral mid-gray patch
/// (the measurement target).
struct Scene {
    name: String,
    img: ImageF32,
    gray_idx: Vec<usize>,
}

/// Well-exposed base (79 % mid-gray patch, 1 % black, 20 % highlights to
/// 0.94), then perturbed: `mul_rgb` channel multipliers (EV shifts = uniform
/// multipliers, casts = per-channel).
fn scene(name: &str, mul_rgb: [f32; 3]) -> Scene {
    let (w, h) = (64u32, 64u32);
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    let mut gray_idx = Vec::new();
    for i in 0..(w * h) as usize {
        let base = if i % 100 < 79 {
            gray_idx.push(i);
            0.18
        } else if i % 100 < 80 {
            0.01
        } else {
            0.50 + ((i % 20) as f32) * 0.0231 // 0.50..=0.94
        };
        let px = &mut img.data_mut()[i * 3..i * 3 + 3];
        px[0] = base * mul_rgb[0];
        px[1] = base * mul_rgb[1];
        px[2] = base * mul_rgb[2];
    }
    Scene {
        name: name.to_string(),
        img,
        gray_idx,
    }
}

fn corpus() -> Vec<Scene> {
    let mut v = Vec::new();
    for (k, label) in [
        (-2.0f32, "ev-2"),
        (-1.0, "ev-1"),
        (-0.5, "ev-0.5"),
        (0.5, "ev+0.5"),
        (1.0, "ev+1"),
        (2.0, "ev+2"),
    ] {
        let s = 2f32.powf(k);
        v.push(scene(label, [s, s, s]));
    }
    // Green-family casts (kept away from the skin ellipse).
    for a in [0.15f32, 0.30, 0.50] {
        v.push(scene(
            &format!("cast{a}"),
            [1.0 - a, 1.0 + a, 1.0 - a * 0.8],
        ));
    }
    // Clipped (over-exposed into the sensor ceiling) + crushed.
    // NOTE: overwritten pixels are excluded from gray_idx (the patch is
    // the measurement target — perturbed pixels must not pollute it).
    let mut clipped = scene("clipped", [2.0f32.powf(1.5); 3]);
    for i in 0..(64 * 64) as usize {
        if i % 20 == 0 {
            let px = &mut clipped.img.data_mut()[i * 3..i * 3 + 3];
            px[0] = 1.0;
            px[1] = 1.0;
            px[2] = 1.0;
        }
    }
    clipped.gray_idx.retain(|&i| i % 20 != 0);
    v.push(clipped);
    let mut crushed = scene("crushed", [2.0f32.powf(-1.5); 3]);
    for i in 0..(64 * 64) as usize {
        if i % 25 == 0 {
            let px = &mut crushed.img.data_mut()[i * 3..i * 3 + 3];
            px[0] = 0.0;
            px[1] = 0.0;
            px[2] = 0.0;
        }
    }
    crushed.gray_idx.retain(|&i| i % 25 != 0);
    v.push(crushed);
    // Mixed: cast + shift.
    v.push(scene("mixed", [0.85 * 0.7, 1.15 * 0.7, 0.9 * 0.7]));
    v
}

// ── metrics ─────────────────────────────────────────────────────────────────

struct Metrics {
    ev_error: f32,
    wb_error: f32,
    clip_rate: f32,
}

impl Metrics {
    fn score(&self) -> f32 {
        self.ev_error + 5.0 * self.wb_error + self.clip_rate
    }
}

const LUMA: [f32; 3] = [0.272_228_7, 0.674_081_8, 0.053_689_5];

/// Apply the suggestion to the scene and measure against the neutral patch.
fn evaluate(scene: &Scene, params: &ToneParams) -> Metrics {
    let corrected = tone::apply(&scene.img, params);
    let data = corrected.data();
    let n = scene.gray_idx.len();
    let mut luma_sum = 0.0f32;
    let mut spread_sum = 0.0f32;
    for &i in &scene.gray_idx {
        let (r, g, b) = (data[i * 3], data[i * 3 + 1], data[i * 3 + 2]);
        luma_sum += LUMA[0] * r + LUMA[1] * g + LUMA[2] * b;
        spread_sum += (r - g).abs().max((b - g).abs());
    }
    let luma_mean = (luma_sum / n as f32).max(1e-6);
    let ev_error = (luma_mean / 0.18).log2().abs();
    let wb_error = spread_sum / n as f32 / 0.18;
    let clip = data.iter().filter(|&&v| v > 1.0).count() as f32 / data.len() as f32;
    Metrics {
        ev_error,
        wb_error,
        clip_rate: clip,
    }
}

fn baseline_params(scene: &Scene) -> ToneParams {
    auto_light::suggest(&scene.img).params
}

// ── tests ───────────────────────────────────────────────────────────────────

#[test]
fn fixture_manifest_passes_the_gate() {
    let m = fixture_manifest();
    vet(&m).expect("the committed fixture manifest must pass its own gate");
}

#[test]
fn fixture_checksum_mismatch_is_rejected() {
    let m = fixture_manifest();
    // Point at the provenance JSON (wrong bytes) → checksum must fail.
    let wrong = format!("{FIXTURE_DIR}/dummy_param_predictor.provenance.json");
    let err = match OnnxPredictor::from_manifest(std::path::Path::new(&wrong), &m) {
        Err(e) => e,
        Ok(_) => panic!("wrong file must fail the sha check"),
    };
    assert!(
        matches!(err, melography_ai::onnx::OnnxError::Checksum { .. }),
        "{err}"
    );
}

#[test]
fn manager_install_then_predict_roundtrip() {
    // The exact app flow: manager install (gate+sha+atomic) → weights_path
    // → OnnxPredictor → ToneParams.
    let root = tempfile::tempdir().unwrap();
    let manifest = fixture_manifest();
    let mut mgr = ModelManager::open(root.path());
    let weights = std::path::PathBuf::from(format!("{FIXTURE_DIR}/{WEIGHTS}"));
    mgr.install(
        manifest.clone(),
        &mut FileFetch {
            path: weights.clone(),
        },
        &CancelToken::default(),
    )
    .expect("fixture installs");
    assert!(mgr.is_ready(&manifest.id));
    let installed = mgr.weights_path(&manifest.id).unwrap();
    let mut predictor = OnnxPredictor::from_manifest(&installed, &manifest).unwrap();
    let s = scene("ev-1", [0.5; 3]);
    let params = predictor.predict(&s.img).expect("inference runs");
    // Dark scene → positive EV suggestion within the clamped range.
    assert!(params.ev > 0.2, "dark scene ev={}", params.ev);
    assert!(params.ev <= 3.0);
}

#[test]
fn predictor_is_deterministic_and_sane() {
    let mut predictor = load_predictor();
    let s = scene("cast0.3", [0.7, 1.3, 0.76]);
    let a = predictor.predict(&s.img).unwrap();
    let b = predictor.predict(&s.img).unwrap();
    assert_eq!(a, b, "same input → identical suggestion");
    // The naive gray-world predictor must at least suggest WB gains ≠ 1
    // for a strong green cast.
    let wb = a.wb.expect("dummy suggests WB");
    assert!(wb.r > 1.05 && wb.b > 1.05, "gains={wb:?}");
}

#[test]
fn baseline_beats_the_dummy_on_the_benchmark() {
    // THE bar (ADR-0004 §1b): a candidate ships only if it beats the
    // baseline here. The dummy is expected to lose — the loss margin is
    // the plumbing-proof evidence recorded in docs/quality/M3_AI_REPORT.md.
    let mut predictor = load_predictor();
    let corpus = corpus();
    let mut base_total = 0.0f32;
    let mut dummy_total = 0.0f32;
    println!("\nscene           baseline(ev/wb/clip)         dummy(ev/wb/clip)");
    for s in &corpus {
        let bp = baseline_params(s);
        let dp = predictor.predict(&s.img).unwrap();
        let bm = evaluate(s, &bp);
        let dm = evaluate(s, &dp);
        base_total += bm.score();
        dummy_total += dm.score();
        println!(
            "{:<15} {:.3} / {:.3} / {:.3}        {:.3} / {:.3} / {:.3}",
            s.name, bm.ev_error, bm.wb_error, bm.clip_rate, dm.ev_error, dm.wb_error, dm.clip_rate
        );
    }
    let n = corpus.len() as f32;
    println!(
        "aggregate (ev + 5·wb + clip): baseline {:.4} vs dummy {:.4}",
        base_total / n,
        dummy_total / n
    );
    assert!(
        base_total <= dummy_total,
        "the baseline must beat (or tie) the naive predictor: baseline={base_total:.4} dummy={dummy_total:.4}"
    );
}

/// Heavy budget measurement (ADR-0004 §5): analysis ≤ 3 s at ≤ 512 px on a
/// 12 MP image — release mode, `--ignored`.
#[test]
#[ignore]
fn analysis_latency_budget_12mp() {
    let (w, h) = (4000u32, 3000u32);
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    for i in 0..(w * h) as usize {
        let v = 0.05 + ((i % 997) as f32 / 997.0) * 0.8;
        let px = &mut img.data_mut()[i * 3..i * 3 + 3];
        px[0] = v * 1.05;
        px[1] = v;
        px[2] = v * 0.95;
    }
    let t = std::time::Instant::now();
    let _s: Suggestion = auto_light::suggest_proxy(&img, 512);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    println!("12 MP → 512 px proxy suggest: {ms:.1} ms (budget 3000 ms)");
    assert!(ms < 3000.0, "analysis budget exceeded: {ms} ms");
    // And the predictor path on the same proxy scale.
    let mut predictor = load_predictor();
    let t = std::time::Instant::now();
    let _ = predictor.predict(&img).unwrap();
    let pms = t.elapsed().as_secs_f64() * 1000.0;
    println!("12 MP → 64 px predictor infer: {pms:.1} ms");
    assert!(pms < 3000.0);
}

#[test]
fn decode_params_contract() {
    // The output decoder is the last line of defense — out-of-range model
    // output becomes clamped, editable parameters.
    let p = decode_params(&[f32::NAN, 0.5, 1.0, 2.0, 0.0, 0.0]).expect("len 6");
    assert_eq!(p.ev, 0.0, "NaN EV must sanitize to 0");
}
