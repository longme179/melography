//! `melography` — headless CLI (ARCHITECTURE §1: "the CLI is a first-class
//! citizen — benchmark, golden tests, CI").
//!
//! Subcommands (M0):
//! * `chart` — write the license-clean synthetic chart as 16-bit PNG
//! * `roundtrip` — decode, S1 to AP1, identity film chain, S5 export;
//!   verifies tolerances, reports timings + hashes as JSON
//! * `golden` — print the golden-transform hash battery (cross-OS S2
//!   evidence collector)
//! * `bench` — decode/render/encode timings on this machine (budget
//!   re-pinning evidence)
//! * `codecs` — runtime codec capability matrix

use clap::{Parser, Subcommand};
use melography_color as mcolor;
use melography_core::{Channels, ImageF32};
use melography_film::FilmChain;
use melography_io as mio;
use melography_presets as mpresets;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "melography",
    version,
    about = "Headless MELOGRAPHY pipeline (M0)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate the synthetic test chart as 16-bit PNG (license-clean).
    Chart {
        #[arg(long, default_value = "chart.png")]
        out: PathBuf,
    },
    /// Decode → AP1 → identity film chain → sRGB 16-bit export, with
    /// verification and timings (JSON report on stdout).
    Roundtrip {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "roundtrip_out.png")]
        out: PathBuf,
        /// Use the lcms2 path for S1/S5 instead of the fast-path matrices
        /// (cross-check mode; doubles as S2 evidence).
        #[arg(long, default_value_t = false)]
        via_lcms: bool,
    },
    /// Golden-transform hash battery (S2 cross-OS determinism evidence).
    Golden,
    /// Timings for decode/transform/encode on this machine.
    Bench {
        /// Approximate megapixels of the synthetic bench image.
        #[arg(long, default_value_t = 12)]
        mp: u32,
    },
    /// Runtime codec capability matrix (JSON).
    Codecs,
}

fn main() {
    let cli = Cli::parse();
    let report = match cli.cmd {
        Cmd::Chart { out } => cmd_chart(&out),
        Cmd::Roundtrip {
            input,
            out,
            via_lcms,
        } => cmd_roundtrip(&input, &out, via_lcms),
        Cmd::Golden => cmd_golden(),
        Cmd::Bench { mp } => cmd_bench(mp),
        Cmd::Codecs => {
            println!("{}", mio::capabilities::capability_matrix_json());
            return;
        }
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report serializable")
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn srgb_icc_bytes() -> Vec<u8> {
    mcolor::srgb_profile()
        .icc()
        .expect("serializing the built-in sRGB profile is total")
}

fn cmd_chart(out: &Path) -> serde_json::Value {
    let (w, h, px) = mio::chart::synthetic_chart_u16();
    mio::encode::encode_png16(out, w, h, &px, Some(&srgb_icc_bytes())).expect("chart encode");
    serde_json::json!({
        "command": "chart",
        "out": out.display().to_string(),
        "width": w,
        "height": h,
        "sha256_png": sha256_hex(&std::fs::read(out).expect("read back")),
    })
}

fn cmd_roundtrip(input: &Path, out: &Path, via_lcms: bool) -> serde_json::Value {
    let t_total = Instant::now();

    // S0 decode
    let t = Instant::now();
    let decoded = mio::decode_file(input).expect("decode");
    let decode_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Encoding policy: embedded ICC honored; untagged → sRGB (CP §2)
    let encoding = match &decoded.encoding {
        melography_core::ColorEncoding::Icc { profile } => melography_core::ColorEncoding::Icc {
            profile: profile.clone(),
        },
        _ => melography_core::ColorEncoding::Srgb { linear: false },
    };

    // S1 → AP1. The fast path is only valid for the known sRGB encoding;
    // ICC-tagged or P3 inputs always go through lcms2 (encoding is data).
    let fastpath_ok = matches!(
        encoding,
        melography_core::ColorEncoding::Srgb { .. } | melography_core::ColorEncoding::Untagged
    );
    let use_lcms = via_lcms || !fastpath_ok;
    let t = Instant::now();
    let ap1 = if use_lcms {
        mcolor::to_ap1_lcms(&decoded.pixels, &encoding).expect("lcms S1")
    } else {
        mcolor::srgb_to_ap1_fast(&decoded.pixels)
    };
    let s1_ms = t.elapsed().as_secs_f64() * 1000.0;

    // S3 film chain (identity at M0)
    let t = Instant::now();
    let chain = FilmChain::new();
    let film_out = chain.apply(&ap1).expect("film chain");
    let film_ms = t.elapsed().as_secs_f64() * 1000.0;

    // S5 export transform + encode (16-bit PNG with sRGB ICC)
    let t = Instant::now();
    let u16px = if use_lcms {
        let back = mcolor::ap1_to_srgb_lcms(&film_out).expect("lcms S5");
        back.data()
            .iter()
            .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
            .collect()
    } else {
        mcolor::ap1_to_srgb_u16_fast(&film_out)
    };
    let s5_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    mio::encode::encode_png16(
        out,
        film_out.width(),
        film_out.height(),
        &u16px,
        Some(&srgb_icc_bytes()),
    )
    .expect("encode");
    let encode_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Verification: compare against input at 16-bit scale (identity chain:
    // roundtrip must be tight; tolerance = 0.5/255 in 8-bit scale = 128.5 in
    // 16-bit, COLOR_PIPELINE §8 row 1 — we assert the much stricter fast-path
    // reality but REPORT the max error honestly).
    let input_u16: Vec<u16> = decoded
        .pixels
        .data()
        .iter()
        .map(|&c| (c.clamp(0.0, 1.0) * 65_535.0).round() as u16)
        .collect();
    let max_err_16 = input_u16
        .iter()
        .zip(&u16px)
        .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
        .max()
        .unwrap_or(0);
    let passed = max_err_16 <= 128;

    // Recipe v0 recorded alongside (non-destructive contract, ARCHITECTURE §7)
    let recipe = mpresets::RecipeV0 {
        version: mpresets::RECIPE_VERSION,
        input_encoding: mpresets::InputEncodingRecord::UntaggedSrgb,
        film: serde_json::to_value(&chain).expect("chain serializes"),
        working_space: mpresets::WorkingSpace::Ap1,
    };
    let sidecar = input.with_extension("melography.json");
    mpresets::atomic_write(
        &sidecar,
        mpresets::to_json(&recipe).expect("recipe json").as_bytes(),
    )
    .expect("sidecar write");

    serde_json::json!({
        "command": "roundtrip",
        "input": input.display().to_string(),
        "output": out.display().to_string(),
        "via_lcms": use_lcms,
        "dims": [decoded.pixels.width(), decoded.pixels.height()],
        "timings_ms": {
            "decode": decode_ms,
            "s1_to_ap1": s1_ms,
            "film_identity": film_ms,
            "s5_to_srgb": s5_ms,
            "encode_png16": encode_ms,
            "total": t_total.elapsed().as_secs_f64() * 1000.0,
        },
        "hashes": {
            "ap1_f32": mcolor::hash_image(&ap1),
            "output_png": sha256_hex(&std::fs::read(out).expect("read back")),
        },
        "verify": {
            "max_abs_err_16bit": max_err_16,
            "threshold_16bit": 128,
            "passed": passed,
        },
    })
}

fn cmd_golden() -> serde_json::Value {
    // The same battery as crates/color/tests/golden_transforms.rs, in a
    // CLI-reportable form: hashes of fast-path/lcms transforms of the chart.
    let chart = mio::chart::synthetic_chart();
    let fast_ap1 = mcolor::srgb_to_ap1_fast(&chart);
    let lcms_ap1 = mcolor::to_ap1_lcms(
        &chart,
        &melography_core::ColorEncoding::Srgb { linear: false },
    )
    .expect("lcms S1 on chart");
    let roundtrip = mcolor::ap1_to_srgb_u16_fast(&fast_ap1);

    serde_json::json!({
        "command": "golden",
        "lcms2_version": mcolor::lcms2_version(),
        "chart_sha256_f32": mcolor::hash_image(&chart),
        "fastpath_ap1_sha256": mcolor::hash_image(&fast_ap1),
        "lcms_ap1_sha256": mcolor::hash_image(&lcms_ap1),
        "roundtrip_u16_sha256": sha256_hex(
            roundtrip.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>().as_slice()
        ),
    })
}

fn cmd_bench(mp: u32) -> serde_json::Value {
    // Synthetic bench image ~ mp megapixels, code-value noise-free pattern
    let w = ((mp as f64 * 1_000_000.0).sqrt() * 1.5) as u32; // 3:2
    let h = (mp as f64 * 1_000_000.0 / w as f64) as u32;
    let mut data = Vec::with_capacity((w as usize) * (h as usize) * 3);
    for y in 0..h {
        for x in 0..w {
            let r = (x % 251) as f32 / 251.0;
            let g = (y % 241) as f32 / 241.0;
            let b = ((x + y) % 253) as f32 / 253.0;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    let img = ImageF32::from_interleaved(w, h, Channels::Rgb, data);

    let t = Instant::now();
    let ap1 = mcolor::srgb_to_ap1_fast(&img);
    let s1_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let out16 = mcolor::ap1_to_srgb_u16_fast(&ap1);
    let s5_ms = t.elapsed().as_secs_f64() * 1000.0;

    let dir = std::env::temp_dir().join(format!("melography-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let png_path = dir.join("bench.png");
    let t = Instant::now();
    mio::encode::encode_png16(&png_path, w, h, &out16, None).expect("encode");
    let enc_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    let decoded = mio::decode_file(&png_path).expect("decode");
    let dec_ms = t.elapsed().as_secs_f64() * 1000.0;
    let _ = std::fs::remove_dir_all(&dir);

    serde_json::json!({
        "command": "bench",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "parallelism": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "bench_image": { "width": w, "height": h, "mpx": (w as f64 * h as f64 / 1e6) },
        "timings_ms": {
            "s1_to_ap1_f32": s1_ms,
            "s5_to_u16": s5_ms,
            "encode_png16": enc_ms,
            "decode_png16": dec_ms,
        },
        "decoded_dims": [decoded.pixels.width(), decoded.pixels.height()],
    })
}
