//! # Spike S1 — preview transport budget (issue #3, ADR-0001)
//!
//! The webview preview path is: **Rust render → (GPU staging readback) →
//! frame encode (or raw) → typed binary channel → canvas blit**. This spike
//! measures every leg that is measurable headlessly:
//!
//! 1. **adapter report** — what GPU (if any) this machine offers;
//! 2. **readback throughput** — compute-pass → map, GB/s, at preview sizes
//!    (wgpu backend; skipped honestly when no adapter exists);
//! 3. **frame encode cost** — raw RGBA copy vs PNG vs WebP-lossless at
//!    1080p/1440p (the compressed-transport candidates);
//! 4. **payload sizes** — bytes per frame per transport option.
//!
//! What this spike deliberately does NOT measure (honest scope): the
//! webview-side IPC hop and canvas blit cost need a live webview — measured
//! by the S4 app harness + owner runbook (`docs/research/spikes/S1_REPORT.md`
//! records the composite budget math and the decision).
//!
//! Run: `cargo run -p s1-transport-spike --release`

use std::time::Instant;

const SIZES: &[(u32, u32, &str)] = &[(1920, 1080, "1080p"), (2560, 1440, "1440p")];

fn main() {
    let mut report = serde_json::Map::new();
    report.insert("os".into(), std::env::consts::OS.into());
    report.insert("arch".into(), std::env::consts::ARCH.into());

    // ── 1. adapters ────────────────────────────────────────────────────────
    let adapters = melography_gpu::enumerate_adapters();
    let adapter_count = adapters.len();
    report.insert("adapters".into(), serde_json::json!(adapters));
    report.insert("adapter_count".into(), adapter_count.into());

    // ── 2. GPU readback throughput ────────────────────────────────────────
    if adapter_count == 0 {
        report.insert(
            "readback".into(),
            serde_json::json!({
                "skipped": true,
                "reason": "no graphics adapter on this machine (honest skip — see lib.rs docs)"
            }),
        );
    } else {
        let mut readback = serde_json::Map::new();
        for (w, h, label) in SIZES {
            let results = melography_gpu::readback::readback_bench(*w, *h, 10);
            match results {
                Ok(runs) => {
                    let mut ms: Vec<f64> = runs.iter().map(|r| r.ms).collect();
                    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let median = ms[ms.len() / 2];
                    let best_gbs = runs.iter().map(|r| r.gb_per_s).fold(0.0, f64::max);
                    readback.insert(
                        (*label).into(),
                        serde_json::json!({
                            "width": w, "height": h,
                            "median_ms": median,
                            "best_gb_per_s": best_gbs,
                            "runs": ms,
                        }),
                    );
                }
                Err(e) => {
                    readback.insert((*label).into(), serde_json::json!({"error": e.to_string()}));
                }
            }
        }
        report.insert("readback".into(), serde_json::Value::Object(readback));
    }

    // ── 3+4. CPU-side frame encode cost + payload size ────────────────────
    // Synthetic preview frame (deterministic pattern), RGBA8 — the wire
    // format of the transport candidates.
    let mut encode = serde_json::Map::new();
    for (w, h, label) in SIZES {
        let frame = synthetic_frame(*w, *h);

        // raw copy (the floor: any transport must at least memcpy)
        let t = Instant::now();
        let raw = frame.clone();
        let raw_ms = t.elapsed().as_secs_f64() * 1000.0;

        // PNG encode (spatial compression, higher CPU)
        let t = Instant::now();
        let png_size = {
            use image::{ExtendedColorType, ImageEncoder};
            let mut buf = Vec::new();
            let enc = image::codecs::png::PngEncoder::new(&mut buf);
            enc.write_image(&raw, *w, *h, ExtendedColorType::Rgba8)
                .unwrap();
            buf.len()
        };
        let png_ms = t.elapsed().as_secs_f64() * 1000.0;

        // WebP lossless encode (the compressed-transport candidate)
        let (webp_ms, webp_size) = {
            let t = Instant::now();
            use image::{ExtendedColorType, ImageEncoder};
            let mut buf = Vec::new();
            let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            let res = enc.write_image(&raw, *w, *h, ExtendedColorType::Rgba8);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            res.unwrap();
            (ms, buf.len())
        };

        encode.insert(
            (*label).into(),
            serde_json::json!({
                "width": w, "height": h,
                "raw": { "ms": raw_ms, "bytes": raw.len() },
                "png":  { "ms": png_ms, "bytes": png_size },
                "webp_lossless": { "ms": webp_ms, "bytes": webp_size },
            }),
        );
    }
    report.insert("frame_encode".into(), serde_json::Value::Object(encode));

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

/// Deterministic RGBA8 preview-like frame (color gradient + structure so
/// compression has realistic content).
fn synthetic_frame(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&[(x % 251) as u8, (y % 241) as u8, ((x + y) % 253) as u8, 255]);
        }
    }
    out
}
