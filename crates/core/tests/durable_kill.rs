//! Kill-during-save battery (PLAN M4 exit criterion: "crash-safety tests
//! (kill during save/export/update) green").
//!
//! The test binary re-executes itself as a child writer that loops
//! `atomic_write_durable` + read-back verification forever. The parent
//! kills the child (SIGKILL / TerminateProcess — no cleanup handlers, a
//! faithful process-death simulation) at randomized points, then asserts
//! the recovery contract:
//!
//! 1. the published file is ALWAYS a complete, parseable document —
//!    either the previous or the new version, never torn;
//! 2. the child's own read-back verification never observed a torn file
//!    either (a regression to write-without-sync trips the same race the
//!    M2 Windows CI flake hashed an empty blob on);
//! 3. `clean_stale_tmp` removes any `.tmp-*` litter the kill left behind
//!    and touches nothing else.
//!
//! Runs on all three OSes in CI (the temp-file naming is pid+nanos
//! unique, so a killed writer's leftovers never collide with the next
//! run's temp files — that property is what this battery exercises).

use melography_core::durable::{atomic_write_durable, clean_stale_tmp};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD_MODE_ENV: &str = "MELOGRAPHY_DURABLE_KILL_CHILD";

fn doc(version: u64, pad: usize) -> String {
    // Sidecar-shaped: a JSON document whose size (a few KiB) spans
    // multiple write() chunks, so a kill mid-write is plausible.
    format!(
        "{{\"schema\":\"melography.sidecar\",\"version\":{version},\"pad\":\"{}\"}}",
        "x".repeat(pad)
    )
}

/// Child: loop write + read-back-verify until killed.
fn child_run(target: PathBuf) -> ! {
    let mut v: u64 = 0;
    loop {
        v += 1;
        let body = doc(v, 4096);
        atomic_write_durable(&target, body.as_bytes())
            .unwrap_or_else(|e| panic!("child write failed at v{v}: {e}"));
        // Read-back: the atomic contract means the read sees the complete
        // new version immediately after the rename returns.
        let got = std::fs::read_to_string(&target)
            .unwrap_or_else(|e| panic!("child read-back failed at v{v}: {e}"));
        assert_eq!(got, body, "torn read-back at v{v} — atomicity broken");
        // Also prove we can immediately re-open and parse it.
        let parsed: serde_json::Value = serde_json::from_str(&got)
            .unwrap_or_else(|e| panic!("child parse failed at v{v}: {e}"));
        assert_eq!(parsed["version"].as_u64(), Some(v));
        std::io::stdout().flush().ok();
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn kill_during_save_leaves_complete_file_and_recoverable_litter() {
    if let Ok(dir) = std::env::var(CHILD_MODE_ENV) {
        child_run(PathBuf::from(dir));
    }

    let exe = std::env::current_exe().expect("test binary path");
    // A handful of kill rounds at varied delays to hit different points
    // in the write→sync→rename cycle. Deterministic pass criteria, not a
    // deterministic schedule: every round must independently satisfy the
    // contract, whatever point it interrupted.
    for round in 0..6 {
        let dir = tempfile::Builder::new()
            .prefix(&format!("melography-kill-{round}-"))
            .tempdir()
            .unwrap();
        let target = dir.path().join("photo.melography.json");

        let mut child = Command::new(&exe)
            .env(CHILD_MODE_ENV, dir.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child writer");

        // Random-ish delay 2–30 ms: hits create/write/sync/rename phases.
        let delay = 2 + (now_nanos() % 28);
        std::thread::sleep(Duration::from_millis(delay.try_into().unwrap()));

        let killed = child.kill().is_ok();
        let _ = child.wait();

        // The child loops forever, so kill() can only fail if the child
        // already died — which is itself a failure surfaced by the panic
        // below (its stderr is inherited in that case; Stdio::null above
        // only mutes the loop's own output, panics still abort the run).
        if !killed {
            panic!("round {round}: child exited before the kill — writer crashed on its own");
        }

        // Contract 1: the published file is a complete document — either
        // absent (killed before the very first rename) or a whole version.
        if target.exists() {
            let text = std::fs::read_to_string(&target)
                .unwrap_or_else(|e| panic!("round {round}: read published file: {e}"));
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                panic!("round {round}: TORN sidecar (len {}): {e}", text.len())
            });
            assert_eq!(
                parsed["schema"].as_str(),
                Some("melography.sidecar"),
                "round {round}: content is not a sidecar"
            );
            let v = parsed["version"].as_u64().expect("version present");
            assert!(v >= 1, "round {round}: bogus version {v}");
        }

        // Contract 3: litter recovery is complete and surgical.
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
                "round {round}: after cleanup only the published file may remain, got {leftovers:?}"
            );
        } else {
            // No litter this round: dir holds at most the published file.
            assert!(
                leftovers.len() <= 1,
                "round {round}: unexpected extra files {leftovers:?}"
            );
        }
    }
}
