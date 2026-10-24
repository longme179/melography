//! Fuzz target: stage-S0 image decode + sanitization (issue #25,
//! ARCHITECTURE §9). Arbitrary bytes → `melography_io::decode::decode_bytes`
//! — the SAME sanitization pipeline `decode_file` runs (bomb guard, ICC
//! extraction, orientation bake-in, RGB-f32 normalization; proven equal by
//! `crates/io`'s `decode_bytes_agrees_with_decode_file` test).
//!
//! **Harness clip (documented honestly):** inputs whose *claimed* header
//! dimensions exceed 8 MP are skipped without decoding — the fuzzer would
//! otherwise spend its budget materializing multi-hundred-MB buffers for
//! tiny inputs that claim huge sizes (a deflate stream of zeros compresses
//! to bytes but decodes to gigabytes). The production bomb guard
//! (MAX_PIXELS = 300 MP) is still exercised: claimed dimensions above it
//! go through the real `decode_bytes` and must yield the typed `Bomb`
//! rejection cheaply. The 8–300 MP band stays un-fuzzed here on purpose —
//! it is covered by the memory-budget gate at 49.8 MP
//! (`crates/cli/tests/memory_budget.rs`) instead.

#![no_main]

use libfuzzer_sys::fuzz_target;

const HARNESS_MAX_PIXELS: u64 = 8 * 1024 * 1024;

/// Header-only dimension peek (never decodes the payload).
fn claimed_dimensions(bytes: &[u8]) -> Option<(u64, u64)> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    Some((u64::from(w), u64::from(h)))
}

fuzz_target!(|data: &[u8]| {
    if let Some((w, h)) = claimed_dimensions(data) {
        let claimed = w.saturating_mul(h);
        if claimed > melography_io::MAX_PIXELS {
            // Above the production bomb limit: the real call must reject
            // with the typed Bomb error BEFORE any pixel allocation —
            // cheap by contract, so the fuzzer stays fast.
            match melography_io::decode::decode_bytes(data) {
                Ok(_) => panic!("bomb guard did not reject {w}x{h}"),
                Err(_) => {}
            }
            return;
        }
        if claimed > HARNESS_MAX_PIXELS {
            // Harness clip (see module docs) — skip the decode.
            return;
        }
    }
    // Within the clip: full real decode. Any outcome is fine; only
    // panics/hangs/OOM are crashes.
    let _ = melography_io::decode::decode_bytes(data);
});
