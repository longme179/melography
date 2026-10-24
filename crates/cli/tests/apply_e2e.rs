//! M1 end-to-end slice (issues #7/#8/#9 exit criteria, scriptable form):
//! import a real `.cube` → library (content-addressed, hash-verified) →
//! apply with declared encodings + intensity/insertion → export
//! JPEG/PNG/TIFF → sidecar v1 with fingerprint + LUT-by-hash. The original
//! file is never modified.

use melography_cli::{cmd_apply, cmd_lut_parse, cmd_sidecar, ApplyFormat, ApplyOptions};
use melography_lut::stage::InsertionPoint;
use melography_lut::LutEncoding;
use melography_presets::sidecar::{SidecarV1, StageEntry};
use std::path::PathBuf;

/// Unique, panic-safe temp dir (guard + path). Replaces the hand-rolled
/// nanos-clock naming — same macOS µs-granularity collision class as the
/// lut crate flake (PR #22 run 40086829632); cleanup is automatic on drop.
fn workdir(name: &str) -> (tempfile::TempDir, PathBuf) {
    let guard = tempfile::Builder::new()
        .prefix(&format!("melography-m1-e2e-{name}-"))
        .tempdir()
        .unwrap();
    let path = guard.path().to_path_buf();
    (guard, path)
}

/// A real-world-shaped 17³ "teal & orange" look LUT (deterministic).
fn look_cube_17() -> String {
    let n = 17;
    let mut s =
        String::from("TITLE \"m1 e2e look\"\nLUT_3D_SIZE 17\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let (r, g, b) = (
                    i as f64 / (n - 1) as f64,
                    j as f64 / (n - 1) as f64,
                    k as f64 / (n - 1) as f64,
                );
                // warm lift in shadows, cool highlights, gentle saturation
                let rr = (1.08 * r + 0.04 * (1.0 - r)).min(1.0);
                let gg = 0.97 * g;
                let bb = (0.92 * b + 0.05 * (1.0 - b)).min(1.0);
                s.push_str(&format!("{rr:.7} {gg:.7} {bb:.7}\n"));
            }
        }
    }
    s
}

/// Identity 17³ LUT (verification anchor: apply(identity) ≡ plain export).
fn identity_cube_17() -> String {
    let n = 17;
    let mut s = String::from("TITLE \"identity\"\nLUT_3D_SIZE 17\n");
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let (r, g, b) = (
                    i as f64 / (n - 1) as f64,
                    j as f64 / (n - 1) as f64,
                    k as f64 / (n - 1) as f64,
                );
                s.push_str(&format!("{r:.7} {g:.7} {b:.7}\n"));
            }
        }
    }
    s
}

fn source_png(dir: &std::path::Path) -> PathBuf {
    let p = dir.join("source.png");
    let (w, h, px) = melography_io::chart::synthetic_chart_u16();
    let icc = melography_cli::srgb_icc_bytes();
    melography_io::encode::encode_png16(&p, w, h, &px, Some(&icc)).unwrap();
    p
}

fn opts_for(dir: &std::path::Path, name: &str) -> ApplyOptions {
    ApplyOptions {
        library_root: dir.join("library"),
        ..ApplyOptions::simple(dir.join("source.png"), dir.join(name))
    }
}

#[test]
fn lut_parse_reports_valid_and_invalid() {
    let (_td, dir) = workdir("parse");
    let good = dir.join("good.cube");
    std::fs::write(&good, look_cube_17()).unwrap();
    let report = cmd_lut_parse(&good);
    assert_eq!(report["valid"], serde_json::json!(true));
    assert_eq!(report["size"], serde_json::json!(17));
    assert_eq!(report["entries"], serde_json::json!(4913));

    let bad = dir.join("bad.cube");
    std::fs::write(&bad, "LUT_3D_SIZE 2\n0 0 0\n").unwrap();
    let report = cmd_lut_parse(&bad);
    assert_eq!(report["valid"], serde_json::json!(false));
    assert!(report["error"]["kind"]
        .as_str()
        .unwrap()
        .contains("entry count"));
}

#[test]
fn apply_identity_lut_matches_plain_export_within_tolerance() {
    let (_td, dir) = workdir("identity");
    let src = source_png(&dir);
    let lut_path = dir.join("identity.cube");
    std::fs::write(&lut_path, identity_cube_17()).unwrap();

    // Plain export (no LUT).
    let plain = opts_for(&dir, "plain.png");
    let plain_report = cmd_apply(&plain);
    assert!(plain_report.get("error").is_none(), "{plain_report}");

    // Identity LUT at full intensity, linear encodings (wrap = matrix only).
    let mut with_lut = opts_for(&dir, "withlut.png");
    with_lut.lut = Some(lut_path);
    with_lut.lut_in = LutEncoding::Rec709DisplayLinear;
    with_lut.lut_out = LutEncoding::Rec709DisplayLinear;
    let lut_report = cmd_apply(&with_lut);
    assert!(lut_report.get("error").is_none(), "{lut_report}");

    // Identity LUT + invertible linear encodings = pass-through *up to f32
    // rounding* (AP1→709→AP1 matrix round-trip + tetrahedral lattice math
    // are each ~1e-7). Compare the re-decoded exports numerically, not by
    // hash: max |Δ| must sit far below the 16-bit 0.5-LSB quantizer.
    let a = melography_io::decode::decode_file(&dir.join("plain.png")).unwrap();
    let b = melography_io::decode::decode_file(&dir.join("withlut.png")).unwrap();
    let mut worst = 0.0f32;
    for (pa, pb) in a.pixels.data().iter().zip(b.pixels.data().iter()) {
        worst = worst.max((pa - pb).abs());
    }
    assert!(
        worst <= 1e-5,
        "identity LUT changed pixels: max |Δ| = {worst:.3e} (code-value domain)"
    );
    let _ = src;
}

#[test]
fn apply_look_lut_end_to_end_all_three_formats() {
    let (_td, dir) = workdir("look");
    source_png(&dir);
    let lut_path = dir.join("look.cube");
    std::fs::write(&lut_path, look_cube_17()).unwrap();

    let mut opts = opts_for(&dir, "out.png");
    opts.lut = Some(lut_path.clone());
    opts.lut_in = LutEncoding::SrgbDisplay;
    opts.lut_out = LutEncoding::SrgbDisplay;
    opts.intensity = 0.8;
    opts.insertion = InsertionPoint::Finishing;

    // PNG 16 (default)
    let report = cmd_apply(&opts);
    assert!(report.get("error").is_none(), "{report}");
    assert!(report["lut"]["content_hash"].is_string());
    // The synthetic chart contains wide-gamut patches; the AP1→sRGB wrap
    // legitimately leaves out-of-domain code values, which the stage
    // CLAMPS and REPORTS (the amber-badge data path). It must be a number;
    // zero is NOT required (and would mean the report is fake).
    assert!(report["lut"]["clamped_pixels"].is_u64(), "{report}");
    let got_intensity = report["lut"]["intensity"].as_f64().unwrap();
    assert!((got_intensity - 0.8).abs() < 1e-6, "{got_intensity}");
    let png_hash = report["hashes"]["output_file"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(
        png_hash,
        report["hashes"]["ap1_f32"].as_str().unwrap(),
        "sanity: hashes differ across domains"
    );

    // JPEG 8-bit + ICC APP2
    let mut jopts = opts.clone();
    jopts.output = dir.join("out.jpg");
    jopts.format = ApplyFormat::Jpeg;
    jopts.depth = 8;
    let jreport = cmd_apply(&jopts);
    assert!(jreport.get("error").is_none(), "{jreport}");
    let jpeg_bytes = std::fs::read(dir.join("out.jpg")).unwrap();
    let extracted = melography_io::encode::extract_app2_icc(&jpeg_bytes)
        .unwrap()
        .expect("JPEG carries ICC");
    assert_eq!(extracted, melography_cli::srgb_icc_bytes());

    // TIFF 16
    let mut topts = opts.clone();
    topts.output = dir.join("out.tiff");
    topts.format = ApplyFormat::Tiff;
    topts.depth = 16;
    let treport = cmd_apply(&topts);
    assert!(treport.get("error").is_none(), "{treport}");

    // Impossible combos are refused, never silently converted.
    let mut bad = opts.clone();
    bad.format = ApplyFormat::Jpeg;
    bad.depth = 16;
    let err = cmd_apply(&bad);
    assert!(err["error"].as_str().unwrap().contains("8-bit only"));

    // Sidecar v1: fingerprint + LUT-by-hash, loadable, original untouched.
    let sidecar_path = dir.join("out.melography.json");
    let sc = SidecarV1::load(&sidecar_path).unwrap();
    assert!(sc.image.fingerprint.is_some());
    match &sc.pipeline.stages[1] {
        StageEntry::Lut {
            instance,
            intensity,
            ..
        } => {
            assert_eq!(
                instance,
                &report["lut"]["content_hash"].as_str().unwrap().to_string()
            );
            assert!((intensity - 0.8).abs() < 1e-6);
        }
        other => panic!("expected LUT stage: {other:?}"),
    }
    // The library stored the LUT content-addressed.
    let lib_index = dir.join("library").join("luts").join("index.json");
    assert!(lib_index.exists(), "library index written");
    let instance = match &sc.pipeline.stages[1] {
        StageEntry::Lut { instance, .. } => instance.clone(),
        _ => unreachable!(),
    };
    assert!(dir
        .join("library")
        .join("luts")
        .join(format!("{instance}.cube"))
        .exists());

    // cmd_sidecar reports the structure.
    let srep = cmd_sidecar(&sidecar_path);
    assert_eq!(srep["version"], serde_json::json!(1));
    assert_eq!(srep["stages"].as_array().unwrap().len(), 2);

    // Original source file untouched: re-export from it reproduces the
    // first export byte-for-byte (non-destructive contract).
    let again = cmd_apply(&opts);
    assert_eq!(
        again["hashes"]["output_file"].as_str().unwrap(),
        report["hashes"]["output_file"].as_str().unwrap(),
        "re-apply from the untouched original is deterministic"
    );
}

#[test]
fn apply_missing_or_corrupt_lut_errors_explicitly() {
    let (_td, dir) = workdir("errors");
    source_png(&dir);

    // Malformed LUT → structured error naming the file.
    let bad = dir.join("bad.cube");
    std::fs::write(&bad, "LUT_3D_SIZE 4\n1 2 3\n").unwrap();
    let mut opts = opts_for(&dir, "x.png");
    opts.lut = Some(bad);
    let report = cmd_apply(&opts);
    let err = report["error"].as_str().unwrap();
    assert!(err.contains("LUT import"), "{err}");
    assert!(err.contains("bad.cube"), "{err}");

    // Library tamper WITHOUT re-import → checksum mismatch on load (the
    // UI recipe-open path). NOTE: `apply` re-imports the source file first,
    // which legitimately self-heals the stored blob (idempotent import);
    // the integrity gate under test is `LutLibrary::load` itself.
    let good = dir.join("good.cube");
    std::fs::write(&good, identity_cube_17()).unwrap();
    let mut opts2 = opts_for(&dir, "y.png");
    opts2.lut = Some(good.clone());
    opts2.lut_in = LutEncoding::Rec709DisplayLinear;
    opts2.lut_out = LutEncoding::Rec709DisplayLinear;
    let ok = cmd_apply(&opts2);
    assert!(ok.get("error").is_none(), "{ok}");
    let hash = ok["lut"]["content_hash"].as_str().unwrap().to_string();
    let blob = dir
        .join("library")
        .join("luts")
        .join(format!("{hash}.cube"));
    std::fs::write(&blob, "tampered content").unwrap();
    let load_err = melography_lut::LutLibrary::open(dir.join("library"))
        .load(&hash)
        .unwrap_err();
    assert!(
        load_err.to_string().contains("integrity"),
        "expected ChecksumMismatch, got: {load_err}"
    );
}
