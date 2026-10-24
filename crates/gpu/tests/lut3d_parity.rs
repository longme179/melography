//! CPU↔GPU parity gate for the LUT kernel (luts.md §5, COLOR_PIPELINE §8,
//! issue #7). Runs **only** where a real adapter exists (macOS Metal /
//! Windows WARP CI runners); adapter-less environments (sandbox, Linux
//! runners) print an honest skip and pass structurally — the measured
//! verdict is whatever the logs show, never fabricated.
//!
//! Inputs: the *oracle fixture* samples (`../lut/tests/oracle_fixtures.json`)
//! — the same boundary-critical + seeded-random set that already validates
//! the CPU engine against the independent Python oracle. Three-way
//! agreement (CPU ↔ GPU ↔ oracle) on one sample set.

#![cfg(feature = "wgpu")]

use melography_gpu::lut3d::run_lut3d;

#[derive(serde::Deserialize)]
struct Fixture {
    luts: Vec<FixtureLut>,
}
#[derive(serde::Deserialize)]
struct FixtureLut {
    name: String,
    kind: String,
    size: u32,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    cube_text: String,
    samples: Vec<FixtureSample>,
}
#[derive(serde::Deserialize)]
struct FixtureSample {
    #[serde(rename = "in")]
    input: [f32; 3],
}

#[test]
fn lut3d_gpu_matches_cpu_within_1e_6() {
    let adapters = melography_gpu::enumerate_adapters();
    println!(
        "lut3d parity: {} adapter(s) on {}",
        adapters.len(),
        std::env::consts::OS
    );
    if adapters.is_empty() {
        println!(
            "lut3d parity: HONEST SKIP — no adapter on this machine; \
             CI runners with adapters (macos/windows) run the real gate."
        );
        return;
    }

    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../lut/tests/oracle_fixtures.json"
    );
    let bytes = std::fs::read(path).expect("oracle fixture readable");
    let fixture: Fixture = serde_json::from_slice(&bytes).expect("fixture schema");

    let mut worst_overall: f32 = 0.0;
    let mut total = 0usize;
    for lut_fix in &fixture.luts {
        if lut_fix.kind != "3d" {
            continue; // GPU kernel is the 3D mirror; 1D stays CPU-side at M1
        }
        let lut =
            melography_lut::cube::parse_cube(&lut_fix.cube_text).expect("oracle cube_text parses");
        let samples: Vec<[f32; 3]> = lut_fix.samples.iter().map(|s| s.input).collect();
        let (gpu_out, report) = run_lut3d(
            lut_fix.size,
            lut_fix.domain_min,
            lut_fix.domain_max,
            &lut.table,
            &samples,
        )
        .expect("adapter exists → run must succeed");
        let mut worst: f32 = 0.0;
        for (input, gpu) in samples.iter().zip(&gpu_out) {
            let (cpu, _) = melography_lut::tetra::sample3d(&lut, *input);
            for c in 0..3 {
                let d = (cpu[c] - gpu[c]).abs();
                worst = worst.max(d);
                assert!(
                    d <= 1e-6,
                    "{}: in={input:?} ch={c} cpu={:.9} gpu={:.9} |Δ|={d:e}",
                    lut_fix.name,
                    cpu[c],
                    gpu[c]
                );
            }
        }
        worst_overall = worst_overall.max(worst);
        total += samples.len();
        println!(
            "lut3d parity [{}] on {}: {} samples, worst |Δ| = {worst:.3e} ({:.3} ms)",
            lut_fix.name, report.adapter, report.samples, report.ms
        );
    }
    println!(
        "lut3d parity TOTAL: {total} samples, worst |Δ| = {worst_overall:.3e} (tolerance 1e-6)"
    );
    assert!(total >= 400, "sample set shrank: {total}");
}
