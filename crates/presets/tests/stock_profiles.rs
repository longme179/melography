//! Shipped stock-profile battery (issue #11, FILM_MODEL §J).
//!
//! The five original artist-authored profiles must: load strictly,
//! validate (physics guards), validate against the JSON Schema, carry
//! honest provenance labels, be genuinely distinct, and render the full
//! chain on a synthetic image (negative *and* reversal paths, color *and*
//! monochrome layouts).

use melography_core::{Channels, ImageF32};
use melography_film::chain::{render, FilmChainParams};
use melography_film::profile::{InversionPath, LayerLayout, Polarity, Provenance, StockProfile};

fn repo_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = crates/presets → repo root is two levels up.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn load_all() -> Vec<StockProfile> {
    let root = repo_root().join("profiles");
    let reg = melography_presets::StockRegistry::open(&root)
        .unwrap_or_else(|e| panic!("registry open: {e}"));
    reg.profiles
}

fn synthetic(w: u32, h: u32) -> ImageF32 {
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    for y in 0..h {
        for x in 0..w {
            let fx = f64::from(x) / f64::from(w - 1);
            let fy = f64::from(y) / f64::from(h - 1);
            let dx = fx - 0.6;
            let dy = fy - 0.4;
            let disc = (-((dx * dx + dy * dy) / 0.01).exp() * 3.0).exp();
            let v = (0.03 + 0.5 * fx + 0.3 * (1.0 - fy) + 1.5 * disc).min(3.0);
            let p = img.pixel_mut(x, y);
            p[0] = (v * 1.04) as f32;
            p[1] = (v * 1.0) as f32;
            p[2] = (v * 0.92) as f32;
        }
    }
    img
}

#[test]
fn exactly_five_shipped_profiles() {
    let all = load_all();
    assert_eq!(all.len(), 5, "issue #11: five shipped profiles");
    let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    for expected in [
        "warm-400-negative",
        "soft-pastel-160",
        "cool-800-night",
        "vivid-reversal-100",
        "classic-monochrome-400",
    ] {
        assert!(ids.contains(&expected), "missing profile {expected}");
    }
}

#[test]
fn all_profiles_carry_honest_provenance() {
    for p in load_all() {
        assert_eq!(
            p.provenance,
            Provenance::ArtistAuthored,
            "{}: shipped profiles are artist-authored (no measured calibration claimed)",
            p.id
        );
        assert!(p.calibration.is_none(), "{}: no calibration claim", p.id);
        assert!(
            p.provenance_text.contains("No measured calibration"),
            "{}: provenance text must state the honesty line",
            p.id
        );
        assert!(!p.license.is_empty());
        assert!(!p.physical_references.is_empty());
    }
}

#[test]
fn all_profiles_render_the_full_chain() {
    let img = synthetic(96, 64);
    for p in load_all() {
        let params =
            FilmChainParams::new(p.clone()).unwrap_or_else(|e| panic!("{}: params: {e}", p.id));
        let out = render(&img, &params).unwrap_or_else(|e| panic!("{}: render: {e}", p.id));
        let mut non_black = 0usize;
        for v in out.data() {
            assert!(v.is_finite(), "{}: non-finite output", p.id);
            if *v > 0.002 {
                non_black += 1;
            }
        }
        assert!(
            non_black > out.data().len() / 20,
            "{}: render must have tonal content",
            p.id
        );
    }
}

#[test]
fn polarity_and_layout_coverage() {
    let all = load_all();
    assert!(
        all.iter().any(|p| p.polarity == Polarity::Reversal),
        "reversal path must be shipped"
    );
    assert!(
        all.iter().any(|p| p.layers == LayerLayout::Monochrome),
        "monochrome path must be shipped"
    );
    assert!(
        all.iter()
            .any(|p| p.print.default_path == InversionPath::Scanner),
        "scanner inversion path must be shipped"
    );
}

#[test]
fn profiles_are_genuinely_distinct() {
    // Render the same synthetic scene through every profile and require
    // pairwise-different outputs (an "artist-authored" set that renders
    // identically would be five copies of nothing).
    let img = synthetic(48, 32);
    let mut hashes: Vec<(String, [u8; 32])> = Vec::new();
    for p in load_all() {
        let params = FilmChainParams::new(p.clone()).unwrap();
        let out = render(&img, &params).unwrap();
        use sha2::{Digest, Sha256};
        hashes.push((p.id.clone(), Sha256::digest(out.as_bytes()).into()));
    }
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i].1, hashes[j].1,
                "{} and {} render identically — profiles are not distinct",
                hashes[i].0, hashes[j].0
            );
        }
    }
}

#[test]
fn profiles_match_the_json_schema() {
    // Structure gate: every shipped profile validates against
    // schemas/stock-profile.schema.json (draft 2020-12). The Rust-side
    // `validate()` covers physics; this covers the documented contract.
    let schema_text =
        std::fs::read_to_string(repo_root().join("schemas/stock-profile.schema.json")).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema_text).unwrap();
    for entry in std::fs::read_dir(repo_root().join("profiles")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        let profile: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if let Err(e) = jsonschema::validate(&schema, &profile) {
            panic!(
                "{}: schema violation: {} (instance path {})",
                path.display(),
                e,
                e.instance_path
            );
        }
    }
}
