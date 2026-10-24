//! ONNX parameter-predictor runner + vetting integration (feature `onnx`,
//! issue #14 / ADR-0004).
//!
//! Weights are **untrusted file inputs**: loaded only after the manifest
//! gate passes and the on-disk SHA-256 matches the manifest; the session
//! is built from bytes; nothing from the model content is ever executed
//! beyond the ONNX graph itself (AI_MODELS §8).
//!
//! The output contract (manifest `runtime.output_semantics`) is exactly
//! the tone-stage knob vector: `[ev, wb_r, wb_g, wb_b, highlights,
//! shadows]` — model output becomes editable [`ToneParams`], never baked
//! pixels (ADR-0004 §2 hard rule).

use crate::{vet, ModelManifest};
use melography_color as mcolor;
use melography_color::tone::{ToneParams, WbGains};
use melography_core::ImageF32;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, thiserror::Error)]
pub enum OnnxError {
    #[error("manifest gate rejected model: {0}")]
    Gate(#[from] crate::ModelError),
    #[error("weights file {0}: {1}")]
    WeightsIo(String, std::io::Error),
    #[error("checksum mismatch: manifest {expected}, file {actual}")]
    Checksum { expected: String, actual: String },
    #[error("ort: {0}")]
    Ort(String),
    #[error("output contract violation: {0}")]
    Contract(String),
}

fn ort_err(e: impl std::fmt::Display) -> OnnxError {
    OnnxError::Ort(e.to_string())
}

/// One ort environment per process (rc.15 requirement, matches the app).
fn env() -> &'static ort::environment::Environment {
    static ENV: OnceLock<ort::environment::Environment> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_telemetry(false).build().expect("ort env"))
}

/// A loaded, gate-passing parameter predictor.
pub struct OnnxPredictor {
    session: ort::session::Session,
    /// Declared input spatial size (square, from the manifest contract).
    input_size: usize,
}

impl OnnxPredictor {
    /// Gate → SHA-256 → session. The manifest is the *only* source of the
    /// I/O contract (shapes are validated against the loaded graph).
    pub fn from_manifest(weights_path: &Path, manifest: &ModelManifest) -> Result<Self, OnnxError> {
        vet(manifest)?; // blocking gate — before any parse of weights
        let bytes = std::fs::read(weights_path)
            .map_err(|e| OnnxError::WeightsIo(weights_path.display().to_string(), e))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != manifest.sha256 {
            return Err(OnnxError::Checksum {
                expected: manifest.sha256.clone(),
                actual,
            });
        }
        let rt = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| OnnxError::Contract("manifest has no runtime contract".into()))?;
        let dims: Vec<usize> = rt
            .input_shape
            .split('x')
            .map(|s| s.trim().parse::<usize>())
            .collect::<Result<_, _>>()
            .map_err(|_| OnnxError::Contract(format!("bad input_shape {:?}", rt.input_shape)))?;
        if dims.len() != 4 || dims[0] != 1 || dims[1] != 3 {
            return Err(OnnxError::Contract(format!(
                "expected NCHW 1x3xHxW, got {:?}",
                rt.input_shape
            )));
        }
        let session = {
            let mut b = ort::session::Session::builder(env()).map_err(ort_err)?;
            b.commit_from_memory(&bytes).map_err(ort_err)?
        };
        if session.inputs().len() != 1 || session.outputs().len() != 1 {
            return Err(OnnxError::Contract(
                "predictor must have exactly one input and one output".into(),
            ));
        }
        Ok(Self {
            session,
            input_size: dims[2],
        })
    }

    /// AP1-linear image → display-encoded NCHW proxy → inference →
    /// [`ToneParams`]. Deterministic for a fixed image + weights.
    pub fn predict(&mut self, img: &ImageF32) -> Result<ToneParams, OnnxError> {
        // Preprocess per the manifest: area-stretch to the declared input
        // size, AP1→sRGB OETF, NCHW f32.
        let side = self.input_size as u32;
        let proxy = crate::auto_light::resize_exact(img, side, side);
        let n = (side * side) as usize;
        let ch = proxy.channels().count();
        let mut nchw = vec![0f32; 3 * n];
        let data = proxy.data();
        for c in 0..3 {
            for i in 0..n {
                let v = data[i * ch + c].clamp(0.0, 1.0);
                nchw[c * n + i] = mcolor::srgb_tf::linear_to_srgb(v);
            }
        }
        let tensor =
            ort::value::Tensor::from_array(([1_usize, 3, side as usize, side as usize], nchw))
                .map_err(ort_err)?;
        let outputs = self.session.run(vec![("input", tensor)]).map_err(ort_err)?;
        let extracted = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(ort_err)?;
        let flat = extracted.1;
        decode_params(flat).ok_or_else(|| {
            OnnxError::Contract(format!(
                "expected 6 output values [ev, wb_r, wb_g, wb_b, highlights, shadows], got {}",
                flat.len()
            ))
        })
    }
}

/// Decode raw model output into sanitized [`ToneParams`] — every value is
/// clamped into the tone-stage ranges (a misbehaving model can at worst
/// produce a clamped, editable suggestion, never garbage pixels).
pub fn decode_params(raw: &[f32]) -> Option<ToneParams> {
    if raw.len() != 6 {
        return None;
    }
    let wb = WbGains {
        r: raw[1].clamp(0.125, 8.0),
        g: raw[2].clamp(0.125, 8.0),
        b: raw[3].clamp(0.125, 8.0),
    };
    let p = ToneParams {
        ev: raw[0].clamp(-3.0, 3.0),
        highlights: raw[4].clamp(-100.0, 100.0),
        shadows: raw[5].clamp(-100.0, 100.0),
        wb: Some(wb),
        ..ToneParams::default()
    };
    Some(p.sanitized())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_clamps_and_rejects() {
        let p = decode_params(&[9.0, 100.0, 1.0, 0.001, 500.0, -500.0]).expect("6 values");
        assert_eq!(p.ev, 3.0);
        assert_eq!(p.wb.unwrap().r, 8.0);
        assert_eq!(p.wb.unwrap().b, 0.125);
        assert_eq!(p.highlights, 100.0);
        assert_eq!(p.shadows, -100.0);
        assert!(decode_params(&[0.0; 5]).is_none());
        assert!(decode_params(&[0.0; 7]).is_none());
    }
}
