//! `melography` — headless CLI (ARCHITECTURE §1: "the CLI is a first-class
//! citizen — benchmark, golden tests, CI").
//!
//! All command implementations live in the library crate (`src/lib.rs`) so
//! integration tests exercise the exact user-facing paths; this file is
//! only argument parsing + dispatch + JSON printing.
//!
//! Subcommands:
//! * `chart` — write the license-clean synthetic chart as 16-bit PNG
//! * `roundtrip` — decode, S1→AP1, identity chain, S5 export + sidecar v1
//! * `golden` — golden-transform hash battery (cross-OS S2 evidence)
//! * `bench` — decode/render/encode timings (budget re-pinning evidence)
//! * `codecs` — runtime codec capability matrix
//! * `lut parse <file>` — strict `.cube` validation report (M1)
//! * `apply` — full M1 slice: decode→AP1→LUT→export JPEG/PNG/TIFF (M1)
//! * `sidecar <file>` — inspect/migrate a sidecar (M1)

use clap::{Parser, Subcommand};
use melography_cli::{
    cmd_apply, cmd_bench, cmd_chart, cmd_film_apply, cmd_film_list, cmd_golden, cmd_lut_parse,
    cmd_roundtrip, cmd_sidecar, ApplyFormat, ApplyOptions, FilmApplyOptions,
};
use melography_lut::stage::InsertionPoint;
use melography_lut::LutEncoding;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "melography",
    version,
    about = "Headless MELOGRAPHY pipeline (M0 baseline + M1 LUT workflow)"
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
    /// Decode → AP1 → identity chain → sRGB 16-bit PNG + sidecar v1.
    Roundtrip {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "roundtrip_out.png")]
        out: PathBuf,
        /// Use lcms2 for S1/S5 instead of the fast-path matrices.
        #[arg(long, default_value_t = false)]
        via_lcms: bool,
    },
    /// Golden-transform hash battery (S2 cross-OS determinism evidence).
    Golden,
    /// Timings for decode/transform/encode on this machine.
    Bench {
        #[arg(long, default_value_t = 12)]
        mp: u32,
    },
    /// Runtime codec capability matrix (JSON).
    Codecs,
    /// Strict `.cube` parse + validation report (no library I/O).
    Lut {
        #[command(subcommand)]
        cmd: LutCmd,
    },
    /// Full M1 slice: decode → AP1 → LUT (library-verified) → export.
    Apply {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Path to a `.cube` file (omit for identity export).
        #[arg(long)]
        lut: Option<PathBuf>,
        /// Declared LUT input encoding (fixed vocabulary, luts.md §3).
        #[arg(long, default_value = "srgb-display")]
        lut_in: String,
        /// Declared LUT output encoding.
        #[arg(long, default_value = "srgb-display")]
        lut_out: String,
        /// LUT intensity mix, 0.0–1.0.
        #[arg(long, default_value_t = 1.0)]
        intensity: f32,
        /// Insertion point: pre-film | film-input | print-output | finishing.
        #[arg(long, default_value = "finishing")]
        insertion: String,
        /// Output format: png | jpeg | tiff.
        #[arg(long, default_value = "png")]
        format: String,
        /// Bit depth: 8 or 16 (per-format factual; JPEG=8, TIFF=16 in v1).
        #[arg(long, default_value_t = 16)]
        depth: u32,
        /// JPEG quality 1–100.
        #[arg(long, default_value_t = 92)]
        quality: u8,
        /// LUT library root (default $MELOGRAPHY_DATA or ~/.melography).
        #[arg(long)]
        library: Option<PathBuf>,
        /// Use lcms2 for S1/S5 (cross-check mode).
        #[arg(long, default_value_t = false)]
        via_lcms: bool,
        /// Skip sidecar writing.
        #[arg(long, default_value_t = false)]
        no_sidecar: bool,
    },
    /// Load/migrate/inspect a `.melography.json` sidecar.
    Sidecar {
        #[arg(long)]
        file: PathBuf,
    },
    /// Film chain (M2): list stocks / apply a stock profile end-to-end.
    Film {
        #[command(subcommand)]
        cmd: FilmCmd,
    },
}

// The `Apply` variant is field-heavy by design (every user knob is
// explicit — mirrors ApplyOptions); boxing would obscure the CLI schema.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum FilmCmd {
    /// List the stock registry (summaries for the browser UI).
    List {
        /// Registry directory (default: ./profiles).
        #[arg(long)]
        profiles_dir: Option<PathBuf>,
    },
    /// decode → AP1 → full film chain (stock + adjustments) → export.
    Apply {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        /// Stock profile id or path to a profile JSON.
        #[arg(long)]
        stock: String,
        /// Registry directory for id lookups (default: ./profiles).
        #[arg(long)]
        profiles_dir: Option<PathBuf>,
        /// Exposure compensation in stops (§A).
        #[arg(long, default_value_t = 0.0)]
        ev: f64,
        /// Push/pull processing in stops, −3..3 (§E).
        #[arg(long, default_value_t = 0.0)]
        push_pull: f64,
        /// Development contrast, 0.6..1.6 (§E).
        #[arg(long, default_value_t = 1.0)]
        dev_contrast: f64,
        /// Print/scan exposure in stops (§F).
        #[arg(long, default_value_t = 0.0)]
        print_exposure: f64,
        /// Grain intensity, 0..2 (§G).
        #[arg(long, default_value_t = 1.0)]
        grain_intensity: f64,
        /// Grain seed override (determinism knob).
        #[arg(long)]
        grain_seed: Option<u32>,
        /// Halation intensity, 0..1 (§C; energy guard still enforced).
        #[arg(long, default_value_t = 1.0)]
        halation_intensity: f64,
        /// Inversion path override: paper | scanner | direct-scan.
        #[arg(long)]
        inversion: Option<String>,
        /// Cross-process [ART]: none | e6-in-c41 | c41-in-e6.
        #[arg(long, default_value = "none")]
        cross_process: String,
        /// Bleach bypass weight, 0..1 [ART].
        #[arg(long, default_value_t = 0.0)]
        bleach_bypass: f64,
        /// Vignette amount, 0..1 (§B).
        #[arg(long, default_value_t = 0.0)]
        vignette: f64,
        /// Veiling glare amount, 0..1 (§B).
        #[arg(long, default_value_t = 0.0)]
        veiling: f64,
        /// Output format: png | jpeg | tiff.
        #[arg(long, default_value = "png")]
        format: String,
        /// Bit depth: 8 or 16.
        #[arg(long, default_value_t = 16)]
        depth: u32,
        /// JPEG quality 1–100.
        #[arg(long, default_value_t = 92)]
        quality: u8,
        /// Strip height for the tiled executor (0 = monolithic).
        #[arg(long, default_value_t = 0)]
        strip_h: u32,
        /// Skip sidecar writing.
        #[arg(long, default_value_t = false)]
        no_sidecar: bool,
    },
}

#[derive(Subcommand)]
enum LutCmd {
    /// Validate one `.cube` file.
    Parse {
        #[arg(long)]
        file: PathBuf,
    },
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
            println!("{}", melography_io::capabilities::capability_matrix_json());
            return;
        }
        Cmd::Lut { cmd } => match cmd {
            LutCmd::Parse { file } => cmd_lut_parse(&file),
        },
        Cmd::Apply {
            input,
            output,
            lut,
            lut_in,
            lut_out,
            intensity,
            insertion,
            format,
            depth,
            quality,
            library,
            via_lcms,
            no_sidecar,
        } => {
            // Resolve vocabulary strings BEFORE touching pixels — unknown
            // identifiers must fail fast with a precise message.
            let lut_in = match LutEncoding::from_str_fixed(&lut_in) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            };
            let lut_out = match LutEncoding::from_str_fixed(&lut_out) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            };
            let insertion = match parse_insertion_cli(&insertion) {
                Ok(i) => i,
                Err(msg) => {
                    eprintln!("error: {msg}");
                    std::process::exit(2);
                }
            };
            let format = match ApplyFormat::parse(&format) {
                Ok(f) => f,
                Err(msg) => {
                    eprintln!("error: {msg}");
                    std::process::exit(2);
                }
            };
            cmd_apply(&ApplyOptions {
                input,
                output,
                lut,
                lut_in,
                lut_out,
                intensity,
                insertion,
                format,
                depth,
                quality,
                library_root: library.unwrap_or_else(melography_cli::default_library_root),
                via_lcms,
                write_sidecar: !no_sidecar,
            })
        }
        Cmd::Sidecar { file } => cmd_sidecar(&file),
        Cmd::Film { cmd } => match cmd {
            FilmCmd::List { profiles_dir } => cmd_film_list(profiles_dir.as_deref()),
            FilmCmd::Apply {
                input,
                output,
                stock,
                profiles_dir,
                ev,
                push_pull,
                dev_contrast,
                print_exposure,
                grain_intensity,
                grain_seed,
                halation_intensity,
                inversion,
                cross_process,
                bleach_bypass,
                vignette,
                veiling,
                format,
                depth,
                quality,
                strip_h,
                no_sidecar,
            } => {
                use melography_film::profile::InversionPath;
                let inversion = match inversion.as_deref() {
                    None => None,
                    Some("paper") => Some(InversionPath::Paper),
                    Some("scanner") => Some(InversionPath::Scanner),
                    Some("direct-scan") => Some(InversionPath::DirectScan),
                    Some(other) => {
                        eprintln!(
                            "error: unknown inversion path {other:?} (fixed list: paper, scanner, direct-scan)"
                        );
                        std::process::exit(2);
                    }
                };
                use melography_film::emulsion::CrossProcessMode;
                let cross_process = match cross_process.as_str() {
                    "none" => CrossProcessMode::None,
                    "e6-in-c41" => CrossProcessMode::E6InC41,
                    "c41-in-e6" => CrossProcessMode::C41InE6,
                    other => {
                        eprintln!(
                            "error: unknown cross-process mode {other:?} (fixed list: none, e6-in-c41, c41-in-e6)"
                        );
                        std::process::exit(2);
                    }
                };
                let format = match ApplyFormat::parse(&format) {
                    Ok(f) => f,
                    Err(msg) => {
                        eprintln!("error: {msg}");
                        std::process::exit(2);
                    }
                };
                let adjustments = melography_film::Adjustments {
                    enabled: true,
                    ev,
                    push_pull_stops: push_pull,
                    dev_contrast,
                    print_exposure_stops: print_exposure,
                    inversion_path: inversion,
                    grain_intensity,
                    halation_intensity,
                    grain_seed,
                    cross_process,
                    bleach_bypass,
                    pre_desaturate: 0.0,
                };
                let lens = melography_film::LensParams {
                    vignette_amount: vignette,
                    veiling_amount: veiling,
                    ..Default::default()
                };
                if let Err(msg) = adjustments.validate() {
                    eprintln!("error: {msg}");
                    std::process::exit(2);
                }
                cmd_film_apply(&FilmApplyOptions {
                    input,
                    output,
                    stock,
                    profiles_dir,
                    adjustments,
                    lens,
                    format,
                    depth,
                    quality,
                    strip_h,
                    write_sidecar: !no_sidecar,
                })
            }
        },
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("report serializable")
    );
}

fn parse_insertion_cli(s: &str) -> Result<InsertionPoint, String> {
    match s {
        "pre-film" => Ok(InsertionPoint::PreFilm),
        "film-input" => Ok(InsertionPoint::FilmInput),
        "print-output" => Ok(InsertionPoint::PrintOutput),
        "finishing" => Ok(InsertionPoint::Finishing),
        other => Err(format!(
            "unknown insertion point {other:?} (fixed list: pre-film, film-input, print-output, finishing)"
        )),
    }
}
