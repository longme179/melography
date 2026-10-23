//! # melography-io
//!
//! Stage S0 (decode + sanitize) and S5 file encoding for MELOGRAPHY
//! (ARCHITECTURE §2, COLOR_PIPELINE §2/§6).
//!
//! **M0 scope:** the four core codecs (JPEG/PNG/TIFF/WebP) through the
//! `image` crate with EXIF orientation, embedded-ICC extraction and alpha
//! normalization; 16-bit PNG encoding with ICC via the `png` crate; the
//! codec capability matrix (`capabilities()` + `tests/codec_matrix.rs`);
//! and the license-clean synthetic chart generator used by every golden
//! test in the workspace.
//!
//! HEIC/AVIF are *not* wired into this crate at M0 — their go/no-go is
//! decided by `spikes/heif-probe` + `docs/CODEC_MATRIX.md` (M0 exit
//! criterion); a follow-up adds the chosen backend behind a feature flag.
//!
//! This crate never links a color-management library: ICC bytes are passed
//! through opaque, and encoding receives already-transformed pixels.

pub mod capabilities;
pub mod chart;
pub mod decode;
pub mod encode;

pub use chart::{synthetic_chart, ChartLayout};
pub use decode::{decode_file, DecodedImage};
pub use encode::{encode_jpeg_u8, encode_png16, encode_tiff_u16, IoEncodeError};

/// Errors from stage S0/S5 file operations.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("decode error: {0}")]
    Decode(#[source] image::ImageError),
    #[error("encode error: {0}")]
    Encode(String),
    #[error("decompression bomb or absurd dimensions rejected: {width}x{height}")]
    Bomb { width: u64, height: u64 },
    #[error("unsupported format or feature: {0}")]
    Unsupported(String),
}

/// Hard pixel cap (COLOR_PIPELINE §7): 300 MP default.
pub const MAX_PIXELS: u64 = 300_000_000;
