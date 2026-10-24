//! Print the go/no-go report JSON (evidence lives in tests + HEIF_PROBE.md).

fn main() {
    let heic = std::fs::read(heif_probe_spike::fixture_path("gradient.heic"))
        .expect("fixture gradient.heic");
    let avif = std::fs::read(heif_probe_spike::fixture_path("gradient.avif"))
        .expect("fixture gradient.avif");
    let report = heif_probe_spike::run_report(&heic, &avif);
    println!("{report}");
}
