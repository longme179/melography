//! S1 spike CI tests: adapter presence is machine-dependent — the test
//! reports (and asserts only what is structural): where an adapter exists,
//! the readback benchmark must actually run and produce sane throughput;
//! where none exists, the report must say so honestly (never fabricate).

#[test]
fn s1_readback_runs_or_skips_honestly() {
    let adapters = melography_gpu::enumerate_adapters();
    println!(
        "s1: {} adapter(s) on {}: {:?}",
        adapters.len(),
        std::env::consts::OS,
        adapters.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
    );
    if adapters.is_empty() {
        // Honest skip: CI Linux/Windows runners may expose no usable
        // adapter; macOS runners expose Metal and run the real benchmark.
        return;
    }
    let results = melography_gpu::readback::readback_bench(64, 64, 3)
        .expect("adapter exists → benchmark must run");
    assert_eq!(results.len(), 3);
    for r in &results {
        assert!(r.ms > 0.0, "wall time must be positive");
        assert!(r.bytes == 64 * 64 * 4);
        assert!(r.gb_per_s.is_finite());
    }
    println!("s1 readback 64x64: {:?}", results);
}

#[test]
fn s1_frame_payload_math() {
    // The transport-budget arithmetic the report relies on (catches unit
    // mistakes): a 1080p RGBA8 frame is exactly 8.29 MB.
    let bytes = 1920_usize * 1080 * 4;
    assert_eq!(bytes, 8_294_400);
    // At the p50 preview budget of 50 ms, even a 0.5 GiB/s path moves
    // ~26.8 MB — more than 3 frames of raw RGBA (24.9 MB) — the transport is
    // bandwidth-feasible; the open question S1 answers is end-to-end latency,
    // not bandwidth.
    let feasible = 0.5 * 1024.0 * 1024.0 * 1024.0 * 0.050; // GiB/s × s → bytes
    assert!(feasible as usize > bytes * 3);
}
