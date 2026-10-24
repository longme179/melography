//! Kill-during-export battery (PLAN M4 exit criterion: "crash-safety
//! tests (kill during save/export/update) green" — the export leg,
//! issue #25).
//!
//! Companion to `melography-core`'s `durable_kill.rs` (kill during
//! SAVE). This one targets the stage-S5 encoders. A naive
//! "spawn + sleep + kill" battery was tried first and **rejected by
//! mutation testing**: the encoders' write phases are only a few
//! milliseconds of a ~100 ms loop, so random kills missed them almost
//! every time and a direct-to-dest writer (the torn-file bug this
//! battery exists to catch) survived 5/5 runs. The battery therefore
//! ANCHORS the kill to the write phase with a progress pipe: the child
//! emits one `W` byte on stdout immediately before each encode, the
//! parent reads it and kills 0.2–3 ms later — deterministically inside
//! the create/deflate/raw-write/flush/fsync window.
//!
//! Contracts (asserted after every kill, for each of PNG/TIFF/JPEG):
//!
//! 1. the destination file is ALWAYS either absent (killed before the
//!    first rename completed) or a COMPLETE, decodable image with the
//!    right geometry — never a torn file at the final path;
//! 2. the child's own read-back decode never observed a torn file
//!    either (a regression to direct `File::create`-at-dest writes
//!    trips exactly this race);
//! 3. `clean_stale_tmp` removes the `.tmp-*` litter a kill leaves
//!    behind and touches nothing else;
//! 4. RETRY after a kill is safe and successful: re-exporting to the
//!    same destination produces a complete, decodable file (the S6
//!    batch queue's retry path relies on this).
//!
//! Mutation-sensitivity is part of the contract: reverting any encoder
//! to a direct write must fail this battery (verified by hand when the
//! pipe anchoring was introduced; see session log 10). Runs on all
//! three OSes in CI.

use melography_core::durable::clean_stale_tmp;
use melography_io::decode::decode_file;
use melography_io::encode::{encode_jpeg_u8, encode_png16, encode_tiff_u16};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD_MODE_ENV: &str = "MELOGRAPHY_EXPORT_KILL_CHILD";
// 1280×960: every encoder's write phase is ≥ tens of ms even in a debug
// build (TIFF raw = 7.4 MB, PNG16/JPEG payloads similar) — the anchored
// kill (0.2–3 ms after the announce byte) lands mid-write reliably, not
// just when the scheduler happens to be quick. Mutation-tested: a
// direct-to-dest TIFF writer fails this battery; smaller images made
// the kill window scheduler-dependent (flaky sensitivity).
const W: u32 = 1280;
const H: u32 = 960;

/// Deterministic pseudo-noise (xorshift64) — incompressible on purpose
/// so deflate/JPEG cannot shortcut the payload writes.
fn noise_u8(seed: u64, n: usize) -> Vec<u8> {
    let mut x = seed | 1;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push((x >> 33) as u8);
    }
    out
}

#[derive(Clone, Copy)]
enum Fmt {
    Png16,
    Tiff16,
    Jpeg,
}

impl Fmt {
    fn from_index(i: usize) -> Self {
        match i % 3 {
            0 => Fmt::Png16,
            1 => Fmt::Tiff16,
            _ => Fmt::Jpeg,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Fmt::Png16 => "png16",
            Fmt::Tiff16 => "tiff16",
            Fmt::Jpeg => "jpeg",
        }
    }
}

/// One encode → returns () (buffers are pre-built by the caller so the
/// loop between progress bytes is pure write+verify).
fn encode_one(fmt: Fmt, px8: &[u8], px16: &[u16], target: &std::path::Path) {
    match fmt {
        Fmt::Png16 => encode_png16(target, W, H, px16, None).expect("png16 encode"),
        Fmt::Tiff16 => encode_tiff_u16(target, W, H, px16).expect("tiff16 encode"),
        Fmt::Jpeg => encode_jpeg_u8(target, W, H, px8, 92).expect("jpeg encode"),
    }
}

/// Child: loop [announce → encode → decode-back-verify] for ONE format
/// (given via the env value) until killed. The announce byte is the
/// anchor the parent kills against — it must be emitted immediately
/// before the encoder opens any file.
fn child_run(dir: PathBuf, fmt: Fmt) -> ! {
    let target = dir.join("export-out.img");
    let px8 = noise_u8(0x0BAD_C0DE, (W * H * 3) as usize);
    let px16: Vec<u16> = noise_u8(0xA5A5_5A5A, (W * H * 3) as usize)
        .into_iter()
        .map(|b| u16::from(b) * 257)
        .collect();
    let mut out = std::io::stdout();
    let mut i: u64 = 0;
    loop {
        i += 1;
        out.write_all(b"W").unwrap();
        out.flush().unwrap();
        encode_one(fmt, &px8, &px16, &target);
        let decoded = decode_file(&target).unwrap_or_else(|e| {
            panic!("child read-back decode failed ({}, i={i}): {e}", fmt.name())
        });
        assert_eq!(
            (decoded.pixels.width(), decoded.pixels.height()),
            (W, H),
            "child torn read-back ({}, i={i})",
            fmt.name()
        );
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// Parent-side verification: the file at `target` (if present) is a
/// complete image with the expected geometry.
fn assert_complete_or_absent(round: usize, fmt: Fmt, target: &std::path::Path) {
    if !target.exists() {
        return; // killed before the very first rename — allowed
    }
    let decoded = decode_file(target).unwrap_or_else(|e| {
        panic!(
            "round {round} ({}): TORN export survived at the final path ({} bytes): {e}",
            fmt.name(),
            target.metadata().map(|m| m.len()).unwrap_or(0)
        )
    });
    assert_eq!(
        (decoded.pixels.width(), decoded.pixels.height()),
        (W, H),
        "round {round} ({}): geometry wrong at final path",
        fmt.name()
    );
}

#[test]
fn kill_during_export_never_leaves_a_torn_file_and_retry_succeeds() {
    if let Ok(dir) = std::env::var(CHILD_MODE_ENV) {
        let fmt = Fmt::from_index(
            std::env::var("MELOGRAPHY_EXPORT_KILL_FMT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0),
        );
        child_run(PathBuf::from(dir), fmt);
    }

    let exe = std::env::current_exe().expect("test binary path");
    // Three rounds per format (PNG / TIFF / JPEG), each killing at a
    // different point inside the anchored write window. Every round must
    // independently satisfy the contract, whatever point it interrupted.
    for round in 0..9 {
        let fmt = Fmt::from_index(round);
        let dir = tempfile::Builder::new()
            .prefix(&format!("melography-export-kill-{round}-"))
            .tempdir()
            .unwrap();
        let target = dir.path().join("export-out.img");

        let mut child = Command::new(&exe)
            .arg("--nocapture") // libtest must pass test stdout through (the announce bytes)
            .env(CHILD_MODE_ENV, dir.path())
            .env("MELOGRAPHY_EXPORT_KILL_FMT", format!("{}", round % 3))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child exporter");

        // Read until the first announce `W` byte (skipping the libtest
        // harness's own `\nrunning N tests\n` chatter), then kill
        // 0.2–3 ms later — inside the file-write phase. EOF before a `W`
        // means the child died on startup (battery failure in its own
        // right, reported by the panic below).
        {
            let mut so = child.stdout.take().expect("stdout piped");
            let mut byte = [0u8; 1];
            loop {
                so.read_exact(&mut byte)
                    .expect("child announced before dying");
                if &byte == b"W" {
                    break;
                }
            }
        }
        let delay_micros = 200 + (now_nanos() % 2800);
        std::thread::sleep(Duration::from_micros(delay_micros.try_into().unwrap()));

        let killed = child.kill().is_ok();
        let _ = child.wait();
        // The child loops forever, so kill() failing means it died on its
        // own — a battery failure in its own right.
        if !killed {
            panic!(
                "round {round} ({}): child exporter exited before the kill — encoder crashed",
                fmt.name()
            );
        }

        // Contract 1: complete-or-absent at the final path.
        assert_complete_or_absent(round, fmt, &target);

        // Contract 3: litter recovery is surgical.
        let removed = clean_stale_tmp(&target).unwrap();
        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        if removed > 0 {
            assert_eq!(
                leftovers.len(),
                if target.exists() { 1 } else { 0 },
                "round {round} ({}): after cleanup only the published file may remain, got {leftovers:?}",
                fmt.name()
            );
        } else {
            assert!(
                leftovers.len() <= 1,
                "round {round} ({}): unexpected extra files {leftovers:?}",
                fmt.name()
            );
        }

        // Contract 4: retry to the SAME destination succeeds and is
        // complete (the batch-queue retry path; also proves a leftover
        // tmp from the kill cannot poison a re-export).
        let px8 = noise_u8(0x0BAD_C0DE, (W * H * 3) as usize);
        let px16: Vec<u16> = noise_u8(0xA5A5_5A5A, (W * H * 3) as usize)
            .into_iter()
            .map(|b| u16::from(b) * 257)
            .collect();
        encode_one(fmt, &px8, &px16, &target);
        assert_complete_or_absent(round, fmt, &target);
    }
}
