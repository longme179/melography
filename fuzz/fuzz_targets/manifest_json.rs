//! Fuzz target: AI model manifest JSON parse + the vetting gate (issue
//! #25; ADR-0004 — "ambiguous weights = rejected"). Arbitrary bytes →
//! `serde_json::from_str::<ModelManifest>`; a parsed manifest goes
//! through `vet()`. Neither may panic; every rejection is a typed
//! `ModelError` (license, checksum shape, runtime contract…).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(m) = serde_json::from_str::<melography_ai::ModelManifest>(&text) {
        let _ = melography_ai::vet(&m);
    }
});
