//! S3 spike integration tests — the CI-enforced form of the measurement.
//! These run on every OS in the CI matrix (issue #5 exit criteria: evidence
//! from all three OSes). Thresholds are intentionally generous (CI machines
//! vary); the *measured numbers* are the spike's evidence, recorded in
//! `docs/research/spikes/S3_REPORT.md` per OS.

use std::time::Instant;

const DUMMY_MODEL: &[u8] = include_bytes!("../models/dummy_identity.onnx");

fn make_input() -> Vec<f32> {
    (0..3 * 64 * 64).map(|i| (i % 97) as f32 / 97.0).collect()
}

#[test]
fn s3_cold_init_under_1s_and_model_loads() {
    let t = Instant::now();
    let env = ort::init().with_telemetry(false).build().unwrap();
    let mut builder = ort::session::Session::builder(&env).unwrap();
    let session = builder.commit_from_memory(DUMMY_MODEL).unwrap();
    let elapsed_ms = t.elapsed().as_secs_f64() * 1000.0;
    // ADR-0001 S3 threshold: cold init ≤ 300 ms target on reference
    // hardware. CI runners + this dev box are slower; 1000 ms is the hard
    // sanity bound. The measured value is reported in the spike report.
    assert!(
        elapsed_ms < 1000.0,
        "cold init took {elapsed_ms} ms (hard sanity bound 1000 ms; ADR-0001 target 300 ms)"
    );
    let _ = session;
}

#[test]
fn s3_warm_latency_and_correctness() {
    let env = ort::init().with_telemetry(false).build().unwrap();
    let mut builder = ort::session::Session::builder(&env).unwrap();
    let mut session = builder.commit_from_memory(DUMMY_MODEL).unwrap();

    let input_data = make_input();
    let mut timings = Vec::new();
    let mut last_sum = 0.0_f32;
    for _ in 0..30 {
        let t = Instant::now();
        let tensor =
            ort::value::Tensor::from_array(([1_usize, 3, 64, 64], input_data.clone())).unwrap();
        let outputs = session.run(vec![("input", tensor)]).unwrap();
        let out = outputs["output"].try_extract_tensor::<f32>().unwrap();
        last_sum = out.1.iter().sum();
        drop(outputs);
        timings.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    timings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = timings[timings.len() / 2];
    // ADR-0004 §5 budget for parameter-predictor analysis is ~200 ms; a
    // trivial model must be far below. 500 ms is the generous CI bound.
    assert!(
        median < 500.0,
        "median warm invocation {median} ms exceeds the 500 ms sanity bound"
    );

    let expect_sum: f32 = input_data.iter().map(|v| v + 0.25).sum();
    assert!(
        (last_sum - expect_sum).abs() < 0.01,
        "Y=X+0.25 violated: {last_sum} vs {expect_sum}"
    );
    // print for CI log evidence
    println!(
        "s3 warm latency median {median:.3} ms (30 runs, {})",
        std::env::consts::OS
    );
}

#[test]
fn s3_model_integrity() {
    use sha2::{Digest, Sha256};
    let sha = format!("{:x}", Sha256::digest(DUMMY_MODEL));
    assert_eq!(
        sha, "b22a9c3d0c3c84bdfc549dd00a5a8006701d18e64d650a721d475f933d0b33db",
        "model bytes changed without updating provenance.json"
    );
}
