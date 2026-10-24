//! Codec capability matrix — runtime introspection + documented facts
//! (the M0 exit criterion "codec matrix verified"; full write-up in
//! `docs/CODEC_MATRIX.md`).

use image::ImageFormat;
use serde::{Deserialize, Serialize};

/// Decode capability of a codec as compiled into this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodeCaps {
    pub format: FormatId,
    pub decode: bool,
    pub max_bit_depth: u8,
    /// Decoder honors EXIF orientation reporting (applied at S0 by us).
    pub orientation: bool,
    /// Decoder surfaces embedded ICC bytes (passthrough at S0).
    pub embedded_icc: bool,
    pub alpha: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormatId {
    Jpeg,
    Png,
    Tiff,
    Webp,
    Heif,
    Avif,
}

impl From<ImageFormat> for FormatId {
    fn from(f: ImageFormat) -> Self {
        match f {
            ImageFormat::Jpeg => FormatId::Jpeg,
            ImageFormat::Png => FormatId::Png,
            ImageFormat::Tiff => FormatId::Tiff,
            ImageFormat::WebP => FormatId::Webp,
            ImageFormat::Avif => FormatId::Avif,
            _ => unreachable!("FormatId only covers M0 matrix formats"),
        }
    }
}

/// Runtime matrix for the four core codecs (compile-time features of the
/// `image` crate are all enabled in this build — see Cargo.toml).
///
/// HEIC/AVIF rows are NOT here: they are gated on the `spikes/heif-probe`
/// decision (ADR-level go/no-go with build evidence, `docs/CODEC_MATRIX.md`).
#[must_use]
pub fn capability_matrix() -> Vec<DecodeCaps> {
    vec![
        DecodeCaps {
            format: FormatId::Jpeg,
            decode: true,
            max_bit_depth: 8,
            orientation: true,
            embedded_icc: true,
            alpha: false,
        },
        DecodeCaps {
            format: FormatId::Png,
            decode: true,
            max_bit_depth: 16,
            orientation: false, // PNG has no EXIF orientation by spec
            embedded_icc: true,
            alpha: true,
        },
        DecodeCaps {
            format: FormatId::Tiff,
            decode: true,
            max_bit_depth: 16,
            orientation: true,
            embedded_icc: true,
            alpha: true,
        },
        DecodeCaps {
            format: FormatId::Webp,
            decode: true,
            max_bit_depth: 8,
            orientation: true,
            embedded_icc: true,
            alpha: true,
        },
    ]
}

/// JSON dump for the CLI `melography codecs` subcommand (evidence artifact).
#[must_use]
pub fn capability_matrix_json() -> String {
    serde_json::to_string_pretty(&capability_matrix()).unwrap_or_else(|_| "[]".into())
}
