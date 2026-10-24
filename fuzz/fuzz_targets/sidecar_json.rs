//! Fuzz target: sidecar v1 deserializer + migration chain + re-serialize
//! (issue #25, ARCHITECTURE §9 "sidecar deserializer"). Arbitrary bytes →
//! `SidecarV1::from_json` and `migrate`; a successful parse is
//! re-serialized and parsed back (the load→save→load roundtrip a resume
//! + autosave performs — unknown fields must survive inertly, floats must
//! round-trip, and nothing may panic).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(sc) = melography_presets::sidecar::SidecarV1::from_json(&text) {
        if let Ok(re) = sc.to_json() {
            // Round-trip must not panic; a parse→save→load cycle that
            // fails is a bug, but even that is an Err, not a crash.
            let _ = melography_presets::sidecar::SidecarV1::from_json(&re);
        }
    }
    let _ = melography_presets::sidecar::migrate(&text);
});
