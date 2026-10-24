//! # melography-ai
//!
//! Local-only AI assistance (ADR-0004): models output *editable parameters*
//! (never baked pixels), run on CPU, are offline, and ship only through the
//! manifest vetting gate.
//!
//! * M0: manifest schema + gate skeleton (issue #5).
//! * M3 (issue #13): the **non-ML auto-lighting baseline** ([`auto_light`])
//!   — deterministic percentile/gray-world statistics, zero models, zero
//!   downloads, offline by definition — plus the model manager
//!   ([`manager`], AI_MODELS §6 states) and the ONNX parameter-predictor
//!   evaluation harness (issue #14, feature `onnx`).

pub mod auto_light;
pub mod manager;
#[cfg(feature = "onnx")]
pub mod onnx;

use serde::{Deserialize, Serialize};

// ── budgets (ADR-0004 §3 / AI_MODELS §5) ────────────────────────────────────

/// Compressed on-disk ceiling per model.
pub const MAX_MODEL_BYTES: u64 = 50 * 1024 * 1024;
/// Highest ONNX opset the pinned runtime is verified for (S3 evidence).
pub const MAX_OPSET: u16 = 17;

#[derive(Debug, thiserror::Error)]
#[error("AI model error: {0}")]
pub enum ModelError {
    #[error("manifest gate rejected model: {0}")]
    Gate(String),
}

/// What a model is allowed to output (ADR-0004 §2 hard rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    /// A vector of named, typed recipe parameters (suggestions).
    Parameters,
    /// A mask (segmentation/depth) as an image-like attachment.
    Mask,
}

/// Canonical upstream provenance (ADR-0004 §4 "canonical upstream + weights
/// URI + revision").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Upstream {
    pub repo: String,
    /// Canonical weights URI — must be HTTPS on the project allowlist.
    pub uri: String,
    /// Pinned revision (commit SHA or release id).
    pub revision: String,
}

/// Runtime I/O contract (ADR-0004 §4: opset, tensor shapes, preprocessing,
/// output semantics — a model whose outputs are not parameterizable is
/// architecturally rejected).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeContract {
    pub opset: u16,
    /// e.g. `"1x3x256x256"`.
    pub input_shape: String,
    /// e.g. `"srgb-display"` (AI_MODELS §4 encoding vocabulary).
    pub input_encoding: String,
    /// Preprocessing note (resize/bilinear, /255, …).
    pub preprocess: String,
    /// Output semantics: which parameter each output element means.
    pub output_semantics: String,
    /// Value ranges per output element.
    pub output_ranges: String,
    /// `"none"` or `"q8 (artifacts note)"`.
    pub quantization: String,
    /// Minimum runtime the measurements were taken with.
    pub min_runtime: String,
}

/// Execution providers (CPU mandatory everywhere).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Eps {
    pub tested: Vec<String>,
    pub optional: Vec<String>,
}

/// Per-OS measured evidence (budgets re-measured per release, §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub latency_ms: f64,
    pub rss_mb: f64,
    /// Quality note / benchmark score.
    pub quality: String,
}

/// Per-model manifest (ADR-0004 §4). Every shipped model carries one; the
/// manager refuses to install any model whose manifest fails [`vet`].
/// M3 extends the M0 skeleton to the full AI_MODELS §4 shape — every new
/// field carries a serde default so older manifests still parse (the gate
/// then rejects them for missing evidence, which is the honest outcome).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub id: String,
    pub version: semver::Version,
    pub task: String,
    pub output_kind: OutputKind,
    /// sha256 of the weights file — the installer verifies before use.
    pub sha256: String,
    pub size_bytes: u64,
    /// License of the *weights themselves* (independent of any repo license
    /// — ADR-0003 asset rule).
    pub weights_license: String,
    pub source_url: String,
    /// Owner sign-off that the weights license was verified for
    /// redistribution (AGENTS.md §2.4).
    pub license_cleared_by_owner: bool,
    /// Weights may be redistributed by this project (required when the
    /// model is offered as a download).
    #[serde(default)]
    pub redistributable: bool,
    /// Canonical upstream (repo / weights URI / pinned revision).
    #[serde(default)]
    pub upstream: Option<Upstream>,
    /// Runtime I/O contract.
    #[serde(default)]
    pub runtime: Option<RuntimeContract>,
    /// Execution providers (CPU must be in `tested`).
    #[serde(default)]
    pub eps: Option<Eps>,
    /// Measured evidence per OS (budgets met, §5).
    #[serde(default)]
    pub measurements: std::collections::BTreeMap<String, Measurement>,
    /// Known biases / failure modes, disclosed honestly.
    #[serde(default)]
    pub known_issues: Vec<String>,
}

/// Download-source allowlist (AI_MODELS §8: downloads only from the pinned
/// manifest URI over HTTPS; RELEASING §7: this repository's Releases only).
pub const DOWNLOAD_HOST_ALLOWLIST: &[&str] = &["github.com", "objects.githubusercontent.com"];

fn sha256_is_hex_64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Vetting gate: reject models that cannot legally or architecturally
/// ship. Rejection reasons accumulate — the caller sees every problem at
/// once (ADR-0004 §4: "Ambiguous weights = rejected").
pub fn vet(manifest: &ModelManifest) -> Result<(), ModelError> {
    let mut rejections = Vec::new();

    // Identity + integrity.
    if !sha256_is_hex_64(&manifest.sha256) {
        rejections.push("sha256 must be a 64-hex digest".to_string());
    }
    if manifest.size_bytes == 0 {
        rejections.push("size_bytes must be positive".to_string());
    }
    if manifest.size_bytes > MAX_MODEL_BYTES {
        rejections.push(format!(
            "size_bytes {} exceeds the {} MiB budget (ADR-0004 §3)",
            manifest.size_bytes,
            MAX_MODEL_BYTES / (1024 * 1024)
        ));
    }

    // Licensing (the hard gate).
    if !manifest.license_cleared_by_owner {
        rejections.push("weights license not cleared by owner (ADR-0003 asset rule)".to_string());
    }
    if manifest.weights_license.is_empty() {
        rejections.push("weights_license must be named".to_string());
    }
    let wl = manifest.weights_license.to_ascii_lowercase();
    if [
        "unknown",
        "unverified",
        "unlicensed",
        "research-only",
        "no license",
        "none",
    ]
    .iter()
    .any(|bad| wl.contains(bad))
    {
        rejections.push(format!(
            "weights_license {:?} is ambiguous/rejected (ADR-0004 §4)",
            manifest.weights_license
        ));
    }
    if !manifest.redistributable {
        rejections.push("weights not redistributable — cannot offer as download".to_string());
    }

    // Source discipline (AI_MODELS §8).
    if !manifest.source_url.starts_with("https://") {
        rejections.push(format!(
            "source_url must be HTTPS (got {:?})",
            manifest.source_url
        ));
    } else if let Ok(url) = url_host(&manifest.source_url) {
        if !DOWNLOAD_HOST_ALLOWLIST.contains(&url.as_str()) {
            rejections.push(format!(
                "source host {url:?} not in the download allowlist {DOWNLOAD_HOST_ALLOWLIST:?}"
            ));
        }
    } else {
        rejections.push("source_url is not a parseable URL".to_string());
    }
    if let Some(up) = &manifest.upstream {
        if !up.uri.starts_with("https://") {
            rejections.push("upstream weights URI must be HTTPS".to_string());
        }
    }

    // Runtime contract (architectural gate).
    match &manifest.runtime {
        None => rejections.push("runtime contract missing (opset/shapes/preprocess/output)".into()),
        Some(rt) => {
            if rt.opset == 0 || rt.opset > MAX_OPSET {
                rejections.push(format!(
                    "opset {} outside verified range 1..={MAX_OPSET} (S3 evidence)",
                    rt.opset
                ));
            }
            if rt.output_semantics.trim().is_empty() {
                rejections.push("output semantics must name the predicted parameters".to_string());
            }
        }
    }
    match &manifest.eps {
        None => rejections.push("execution-provider evidence missing (CPU must be tested)".into()),
        Some(eps) => {
            if !eps.tested.iter().any(|e| e.eq_ignore_ascii_case("cpu")) {
                rejections.push("CPU execution provider must be tested (ADR-0004 §2)".to_string());
            }
        }
    }
    if manifest.measurements.is_empty() {
        rejections.push("no per-OS measurements (budgets must be met, §5)".to_string());
    }

    // v1 ships parameter predictors only (mask pipeline is a separate,
    // later gate — ADR-0004 §1c).
    if manifest.output_kind != OutputKind::Parameters {
        rejections.push("v1 ships parameter predictors only (masks are a later gate)".to_string());
    }

    if !rejections.is_empty() {
        return Err(ModelError::Gate(rejections.join("; ")));
    }
    Ok(())
}

/// Tiny URL host extractor (no url crate — one more dep for one line of
/// policy is not worth it; https-only is checked before this runs).
fn url_host(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "not https".to_string())?;
    let host = rest.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        Err("empty host".into())
    } else {
        Ok(host.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn good() -> ModelManifest {
        ModelManifest {
            id: "melography-autolight-dummy".into(),
            version: semver::Version::new(0, 1, 0),
            task: "exposure/white-balance parameter prediction (evaluation plumbing only)".into(),
            output_kind: OutputKind::Parameters,
            sha256: "a".repeat(64),
            size_bytes: 1024,
            weights_license: "CC0-1.0 (generated by this project)".into(),
            source_url: "https://github.com/longme179/melography".into(),
            license_cleared_by_owner: true,
            redistributable: true,
            upstream: Some(Upstream {
                repo: "longme179/melography".into(),
                uri: "https://github.com/longme179/melography/releases/download/ai/melography-autolight-dummy.onnx".into(),
                revision: "generated-by-tools-gen_dummy_onnx".into(),
            }),
            runtime: Some(RuntimeContract {
                opset: 17,
                input_shape: "1x3x64x64".into(),
                input_encoding: "srgb-display".into(),
                preprocess: "area-downscale to 64px, sRGB decode, /255".into(),
                output_semantics: "[ev, wb_r, wb_g, wb_b, highlights, shadows]".into(),
                output_ranges: "ev [-3,3]; gains [0.5,2]; hl/sh [-100,100]".into(),
                quantization: "none".into(),
                min_runtime: "ort-2.0.0-rc.15".into(),
            }),
            eps: Some(Eps {
                tested: vec!["cpu".into()],
                optional: vec![],
            }),
            measurements: std::collections::BTreeMap::from([(
                "linux-x86_64-cpu".into(),
                Measurement {
                    latency_ms: 1.0,
                    rss_mb: 12.0,
                    quality: "plumbing proof, not a shipping candidate".into(),
                },
            )]),
            known_issues: vec!["outputs are constants — evaluation harness proof only".into()],
        }
    }

    #[test]
    fn gate_passes_complete_manifest() {
        assert!(vet(&good()).is_ok());
    }

    #[test]
    fn gate_rejects_uncleared_license() {
        let mut m = good();
        m.license_cleared_by_owner = false;
        assert!(matches!(vet(&m), Err(ModelError::Gate(_))));
    }

    #[test]
    fn gate_rejects_bad_hash() {
        let mut m = good();
        m.sha256 = "short".into();
        assert!(matches!(vet(&m), Err(ModelError::Gate(_))));
    }

    #[test]
    fn gate_rejects_research_only_weights() {
        let mut m = good();
        m.weights_license = "research-only, no commercial use".into();
        let err = vet(&m).unwrap_err().to_string();
        assert!(err.contains("ambiguous/rejected"), "{err}");
    }

    #[test]
    fn gate_rejects_oversized_model() {
        let mut m = good();
        m.size_bytes = MAX_MODEL_BYTES + 1;
        let err = vet(&m).unwrap_err().to_string();
        assert!(err.contains("budget"), "{err}");
    }

    #[test]
    fn gate_rejects_non_https_and_off_repo_sources() {
        let mut m = good();
        m.source_url = "http://example.com/model.onnx".into();
        assert!(vet(&m).unwrap_err().to_string().contains("HTTPS"));
        m.source_url = "https://evil.example.com/model.onnx".into();
        assert!(vet(&m).unwrap_err().to_string().contains("allowlist"));
    }

    #[test]
    fn gate_rejects_unverified_opset() {
        let mut m = good();
        m.runtime.as_mut().unwrap().opset = 21;
        assert!(vet(&m).unwrap_err().to_string().contains("opset"));
    }

    #[test]
    fn gate_rejects_missing_runtime_evidence() {
        let mut m = good();
        m.runtime = None;
        assert!(vet(&m).unwrap_err().to_string().contains("runtime"));
        let mut m2 = good();
        m2.eps = None;
        assert!(vet(&m2)
            .unwrap_err()
            .to_string()
            .contains("execution-provider"));
        let mut m3 = good();
        m3.measurements.clear();
        assert!(vet(&m3).unwrap_err().to_string().contains("measurements"));
    }

    #[test]
    fn gate_rejects_mask_output_in_v1() {
        let mut m = good();
        m.output_kind = OutputKind::Mask;
        assert!(vet(&m)
            .unwrap_err()
            .to_string()
            .contains("parameter predictors"));
    }

    #[test]
    fn gate_accumulates_all_rejections() {
        let mut m = good();
        m.sha256 = "x".into();
        m.license_cleared_by_owner = false;
        m.redistributable = false;
        let err = vet(&m).unwrap_err().to_string();
        assert!(err.contains("sha256"));
        assert!(err.contains("cleared"));
        assert!(err.contains("redistributable"));
    }

    #[test]
    fn manifest_serde_roundtrip() {
        let m = good();
        let s = serde_json::to_string(&m).unwrap();
        let back: ModelManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn manifest_m0_shape_still_parses_but_fails_gate_honestly() {
        // An M0-era minimal manifest must parse (additive fields default)
        // and then be REJECTED for missing evidence — never silently pass.
        let m0 = serde_json::json!({
            "id": "old",
            "version": "0.1.0",
            "task": "spike",
            "output_kind": "parameters",
            "sha256": "a".repeat(64),
            "size_bytes": 1024,
            "weights_license": "MIT",
            "source_url": "https://github.com/longme179/melography",
            "license_cleared_by_owner": true
        });
        let m: ModelManifest = serde_json::from_value(m0).expect("M0 shape parses");
        let err = vet(&m).unwrap_err().to_string();
        assert!(err.contains("redistributable"));
        assert!(err.contains("runtime"));
        assert!(err.contains("measurements"));
    }
}
