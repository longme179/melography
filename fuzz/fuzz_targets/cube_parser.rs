//! Fuzz target: the strict `.cube` LUT parser (issue #25, ARCHITECTURE §9
//! "LUT parser"). Arbitrary bytes → UTF-8 lossy → `parse_cube`. Any
//! outcome is fine (LutError for garbage); panics/absurd allocations are
//! crashes. The parser's own limits (DEFAULT_MAX_BYTES, size caps, finite
//! values only) are the contract under test. Seed corpus = the 30
//! hand-built adversarial cases from `crates/lut/tests/fuzz_corpus`
//! (invalid sizes, NaN/Inf entries, BOM/UTF-16, CRLF truncation, …).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = melography_lut::cube::parse_cube(&text);
});
