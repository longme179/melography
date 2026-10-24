//! S4 spike report runner: prints the positive-roundtrip JSON (the full
//! evidence lives in `cargo test` output + docs/research/spikes/S4_REPORT.md).

fn main() {
    let report = s4_updater_spike::run_report();
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
