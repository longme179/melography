//! Memory budget gate (#25, PLAN §4): **≤ 2 GiB peak RSS for a 50 MP
//! decode → render (full film chain + tone) → export end-to-end.**
//!
//! `#[ignore]`d like the film statistical gates — CI runs it explicitly
//! in release mode (`cargo test --release -p melography-cli --test
//! memory_budget -- --ignored --nocapture`); the run prints the hardware
//! used (PLAN §4: "every CI perf test prints the hardware used").
//!
//! What it exercises — the exact `cmd_film_apply` path the app's export
//! dialog and S6 batch queue drive (ARCHITECTURE §2: one pipeline):
//!
//! 1. a synthetic 8640×5760 (49.8 MP) smooth-gradient PNG is generated
//!    through the real `melography-io` encoder (in-process — the
//!    generation cost counts against the budget, honestly);
//! 2. `cmd_film_apply` with a real registry stock, tone stage ON, tiled
//!    strips 512 (the app's export options) → 16-bit PNG export + sidecar;
//! 3. peak RSS is read from the OS:
//!    * Linux: `VmHWM` (exact high-water mark),
//!    * Windows: `PeakWorkingSetSize` (exact, via psapi FFI — no crate),
//!    * macOS: sampled `ps -o rss=` (a **lower bound** — no portable
//!      VmHWM exists; printed as such, never presented as a peak);
//! 4. assertion: peak ≤ 2 GiB, with the measured number + headroom
//!    printed for the docs (M4_HARDENING_REPORT.md re-quotes them);
//! 5. the export is then decoded back (dims must match) — after the
//!    measurement, so the verification cannot inflate the number.
//!
//! The drops that keep this under budget (`decoded`/`ap1`/`working`
//! freed as soon as their facts are extracted) live in
//! `crates/cli/src/lib.rs` (#25 comments); a regression to the old
//! hold-everything-alive flow measures ≈ 2.1 GiB here and fails.

use melography_cli::{ApplyFormat, FilmApplyOptions};
use melography_color::tone::ToneParams;
use melography_io as mio;
use melography_presets as mpresets;
use std::path::PathBuf;

/// 2 GiB in bytes (PLAN §4: "Memory ≤ 2 GiB @ 50MP incl. caches").
const BUDGET_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Peak RSS of THIS process, per-OS (see module docs for the honesty
/// notes — macOS is a sampled lower bound and says so).
fn peak_rss_bytes() -> Option<(u64, &'static str)> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                return Some((kb * 1024, "VmHWM (exact high-water mark)"));
            }
        }
        None
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        #[allow(non_snake_case)]
        struct PROCESS_MEMORY_COUNTERS {
            cb: u32,
            PageFaultCount: u32,
            PeakWorkingSetSize: usize,
            WorkingSetSize: usize,
            QuotaPeakPagedPoolUsage: usize,
            QuotaPagedPoolUsage: usize,
            QuotaPeakNonPagedPoolUsage: usize,
            QuotaNonPagedPoolUsage: usize,
            PagefileUsage: usize,
            PeakPagefileUsage: usize,
        }
        extern "system" {
            fn GetCurrentProcess() -> isize;
            fn GetProcessMemoryInfo(
                process: isize,
                counters: *mut PROCESS_MEMORY_COUNTERS,
                cb: u32,
            ) -> i32;
        }
        let mut pmc = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            PageFaultCount: 0,
            PeakWorkingSetSize: 0,
            WorkingSetSize: 0,
            QuotaPeakPagedPoolUsage: 0,
            QuotaPagedPoolUsage: 0,
            QuotaPeakNonPagedPoolUsage: 0,
            QuotaNonPagedPoolUsage: 0,
            PagefileUsage: 0,
            PeakPagefileUsage: 0,
        };
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) };
        (ok != 0).then(|| (pmc.PeakWorkingSetSize as u64, "PeakWorkingSetSize (exact)"))
    }
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()?;
        let rss_kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
        Some((
            rss_kb * 1024,
            "ps rss sample (LOWER BOUND — macOS has no portable VmHWM)",
        ))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

fn cpu_description() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if let Some(model) = line.strip_prefix("model name") {
                    return model.trim_start_matches(':').trim().to_string();
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(out) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Processor).Name",
            ])
            .output()
        {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    "unknown".to_string()
}

#[test]
#[ignore = "release-only memory gate — CI: cargo test --release -p melography-cli --test memory_budget -- --ignored --nocapture"]
fn memory_budget_50mp_film_export_le_2gib() {
    // Repo-root profiles/ (the shipped registry — same resolution the
    // film perf tests use).
    let profiles_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../profiles");
    assert!(
        profiles_dir.is_dir(),
        "stock registry not found at {}",
        profiles_dir.display()
    );
    let registry = mpresets::StockRegistry::open(&profiles_dir).expect("open stock registry");
    // Deterministic pick: first id in sorted order (any stock drives the
    // same buffer plan — the budget is dominated by image buffers, not
    // stock constants; the choice is printed with the evidence).
    let mut ids: Vec<&str> = registry.profiles.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    let stock_id = ids
        .first()
        .copied()
        .expect("registry non-empty")
        .to_string();

    // 8640×5760 = 49.8 MP ("50 MP class", PLAN §4 budget line).
    let (w, h) = (8640u32, 5760u32);
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("budget-50mp.png");
    let out = dir.path().join("budget-50mp-out.png");

    // Smooth synthetic gradient through the real encoder (fast to
    // compress; decode allocations are content-independent).
    let t_gen = std::time::Instant::now();
    {
        let mut px = vec![0u8; (w as usize) * (h as usize) * 3];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 3;
                px[i] = (x * 255 / (w as usize - 1).max(1)) as u8;
                px[i + 1] = (y * 255 / (h as usize - 1).max(1)) as u8;
                px[i + 2] = ((x + y) % 256) as u8;
            }
        }
        mio::encode::encode_png8(&src, w, h, &px, None).expect("generate 50MP source");
    }
    let gen_ms = t_gen.elapsed().as_secs_f64() * 1000.0;

    // End-to-end: decode → AP1 → tone (worst-case extra full-res buffer)
    // → full film chain (tiled, strip 512 — the app's export shape) →
    // PNG16 encode → sidecar.
    let opts = FilmApplyOptions {
        input: src.clone(),
        output: out.clone(),
        stock: stock_id.clone(),
        profiles_dir: Some(profiles_dir.clone()),
        adjustments: Default::default(),
        lens: Default::default(),
        format: ApplyFormat::Png,
        depth: 16,
        quality: 92,
        strip_h: 512,
        write_sidecar: true,
        tone: Some(ToneParams {
            ev: 0.35,
            highlights: -0.2,
            shadows: 0.15,
            ..Default::default()
        }),
    };
    let t_run = std::time::Instant::now();
    let report = melography_cli::cmd_film_apply(&opts);
    let run_ms = t_run.elapsed().as_secs_f64() * 1000.0;

    assert!(
        report.get("error").is_none(),
        "export failed: {}",
        report["error"]
    );

    // Measure BEFORE the verification decode (macOS sampling would
    // otherwise include it).
    let (peak, method) = peak_rss_bytes().expect("peak RSS measurement available on this OS");
    let budget_mb = BUDGET_BYTES as f64 / (1024.0 * 1024.0);
    let peak_mb = peak as f64 / (1024.0 * 1024.0);

    // Evidence block — re-quoted verbatim in
    // docs/quality/M4_HARDENING_REPORT.md (PLAN §4: real measurements).
    println!("==== MELOGRAPHY memory budget gate (#25, PLAN §4) ====");
    println!(
        "image        : {w}x{h} ({:.1} MP), synthetic PNG8",
        w as f64 * h as f64 / 1e6
    );
    println!("stock        : {stock_id} (full film chain, strip 512) + tone stage");
    println!(
        "hardware     : {} / {} cores / {}",
        cpu_description(),
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        std::env::consts::OS
    );
    println!("measurement  : {method}");
    println!("generate_ms  : {gen_ms:.0}");
    println!(
        "e2e_ms       : {run_ms:.0} (decode+s1: {:.0}, film: {:.0}, encode: {:.0})",
        report["timings_ms"]["decode"].as_f64().unwrap_or(0.0)
            + report["timings_ms"]["s1_ap1"].as_f64().unwrap_or(0.0),
        report["timings_ms"]["film_chain"].as_f64().unwrap_or(0.0),
        report["timings_ms"]["encode"].as_f64().unwrap_or(0.0)
    );
    println!(
        "peak_rss     : {peak_mb:.0} MiB (budget {budget_mb:.0} MiB, headroom {:.0} MiB)",
        budget_mb - peak_mb
    );
    println!("======================================================");

    assert!(
        peak <= BUDGET_BYTES,
        "MEMORY BUDGET EXCEEDED: peak {peak_mb:.0} MiB > {budget_mb:.0} MiB (2 GiB @ 50 MP, PLAN §4). \
         Method: {method}. See docs/quality/M4_HARDENING_REPORT.md for the buffer-drop plan."
    );

    // The export is real: decode-back with matching geometry.
    let decoded = mio::decode::decode_file(&out).expect("decode exported file");
    assert_eq!(
        (decoded.pixels.width(), decoded.pixels.height()),
        (w, h),
        "export geometry"
    );
    // Sidecar rides along (provenance contract).
    assert!(out.with_extension("melography.json").exists());
}
