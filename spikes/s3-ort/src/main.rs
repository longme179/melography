//! # Spike S3 — `ort` / ONNX Runtime CPU packaging (issue #5, ADR-0001)
//!
//! Measures the packaging/runtime costs that decide whether the local-AI
//! plan (ADR-0004) is viable on all three desktop OSes:
//!
//! 1. **cold init** — `ort::init()` + first `Session` creation from bytes
//!    (includes ORT library init, no model execution); measured exactly
//!    once — the rc.15 wrapper forbids re-initialization per process
//!    ("only one environment is allowed"), which mirrors real usage;
//! 2. **warm invocation latency** — tensor in → run → read out, median of N;
//! 3. **binary/artifact footprint** — ONNX Runtime library size on disk;
//! 4. **correctness** — the dummy model's `Y = X + 0.25` verified, and the
//!    model's sha256 asserted (weights are data — never executed
//!    implicitly; AGENTS.md §2.6).
//!
//! The dummy model is project-authored (`scripts/gen_dummy_onnx.py`,
//! provenance in `models/provenance.json`), 171 bytes, no third-party
//! weights. It measures *overhead*, not compute — M3's real models are
//! small parameter predictors and get their own budgets then.
//!
//! Run the report: `cargo run -p s3-ort-spike --release`

use std::time::Instant;

/// Embedded dummy model (see module docs). Embedding (vs. reading at
/// runtime) makes the measurement independent of filesystem state.
const DUMMY_MODEL: &[u8] = include_bytes!("../models/dummy_identity.onnx");

fn main() -> ort::Result<()> {
    let mut report = serde_json::Map::new();
    report.insert("os".into(), std::env::consts::OS.into());
    report.insert("arch".into(), std::env::consts::ARCH.into());
    report.insert(
        "parallelism".into(),
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .into(),
    );

    // sha256 of the model — provenance gate (the spike refuses to run an
    // unexpected model; weights integrity is checked, not trusted).
    use sha2::{Digest, Sha256};
    let model_sha = format!("{:x}", Sha256::digest(DUMMY_MODEL));
    assert_eq!(
        model_sha, "b22a9c3d0c3c84bdfc549dd00a5a8006701d18e64d650a721d475f933d0b33db",
        "dummy model hash changed — regenerate provenance.json (scripts/gen_dummy_onnx.py)"
    );
    report.insert("model_sha256".into(), model_sha.into());
    report.insert("model_bytes".into(), DUMMY_MODEL.len().into());

    // 1. cold init: ONE measurement — `ort::init()` + first Session
    //    creation. The rc.15 wrapper forbids re-initializing after teardown
    //    ("only one environment is allowed per process"), which also matches
    //    the real app: one environment per app process, ever. Measuring the
    //    cold path exactly once is the honest form.
    let t = Instant::now();
    let env = ort::init().with_telemetry(false).build()?;
    let mut session = {
        let mut b = ort::session::Session::builder(&env)?;
        b.commit_from_memory(DUMMY_MODEL)?
    };
    let cold_init_ms = t.elapsed().as_secs_f64() * 1000.0;
    report.insert("cold_init_ms".into(), cold_init_ms.into());

    // 1b. warm session re-creation on the same env, 5 attempts — this is
    //     the model-manager path (loading/switching models later);
    //     re-initializing the environment is neither allowed nor needed.
    let mut session_recreate_ms = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let s = {
            let mut b = ort::session::Session::builder(&env)?;
            b.commit_from_memory(DUMMY_MODEL)?
        };
        drop(s);
        session_recreate_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    report.insert(
        "session_recreate_ms_all".into(),
        serde_json::json!(session_recreate_ms),
    );
    let input_data: Vec<f32> = (0..3 * 64 * 64).map(|i| (i % 97) as f32 / 97.0).collect();
    let mut warm_ms = Vec::new();
    let mut last_sum = 0.0_f32;
    for _ in 0..50 {
        let t = Instant::now();
        let tensor = ort::value::Tensor::from_array(([1_usize, 3, 64, 64], input_data.clone()))?;
        let outputs = session.run(vec![("input", tensor)])?;
        let out = outputs["output"].try_extract_tensor::<f32>()?;
        last_sum = out.1.iter().sum();
        drop(outputs);
        warm_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    warm_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = warm_ms[warm_ms.len() / 2];
    report.insert("warm_invocation_ms_median".into(), median.into());
    report.insert("warm_invocation_ms_min".into(), warm_ms[0].into());
    report.insert(
        "warm_invocation_ms_max".into(),
        (*warm_ms.last().unwrap()).into(),
    );

    // 3. correctness: Y = X + 0.25
    let expect_sum: f32 = input_data.iter().map(|v| v + 0.25).sum();
    assert!(
        (last_sum - expect_sum).abs() < 0.01,
        "dummy model output mismatch: {last_sum} vs {expect_sum}"
    );
    report.insert("correctness_check".into(), "Y=X+0.25 sum verified".into());

    // 4. artifact footprint: locate the onnxruntime artifact via ort-sys
    // linkage notes (static by default in rc.15 → report the executable
    // size delta instead, honest and directly measurable).
    let exe = std::env::current_exe().expect("current_exe works in a CLI context");
    let exe_size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    report.insert("binary_bytes_this_exe".into(), exe_size.into());
    report.insert("binary_note".into(),
        "static-linked ORT by default (rc.15, download-binaries): onnxruntime is inside this executable; a dynamic-build comparison runs in CI (docs/research/spikes/S3_REPORT.md)".into());

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(())
}
