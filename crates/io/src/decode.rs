//! Stage S0: decode + sanitize (COLOR_PIPELINE §2).
//!
//! * EXIF orientation applied at decode, never re-applied later (guard flag).
//! * Embedded ICC bytes extracted and passed through opaquely.
//! * Alpha normalization (straight alpha) happens in `to_rgb_f32` — M0
//!   targets opaque RGB; the premultiplied↔straight roundtrip gate (CP §8
//!   row 6) lands with the alpha-carrying M1 pipeline.
//! * Decompression-bomb guard: dimensions rejected above [`MAX_PIXELS`].

use crate::IoError;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use melography_core::{Channels, ColorEncoding, ImageF32};
use std::io::Cursor;
use std::path::Path;

/// Sanitized decode result.
#[derive(Debug)]
pub struct DecodedImage {
    /// Pixels, sRGB-convention *encoded* f32 (or the file's own encoding —
    /// see `encoding`); linearization is stage S1's job, not S0's.
    pub pixels: ImageF32,
    /// Colorimetric meaning of `pixels` (embedded ICC if present, else the
    /// format default with `Untagged` semantics surfaced to the UI).
    pub encoding: ColorEncoding,
    /// Source container format.
    pub format: ImageFormat,
    /// Orientation already baked in — later stages must NOT re-apply.
    pub orientation_applied: bool,
}

/// Decode a file from disk, applying sanitization (S0).
pub fn decode_file(path: &Path) -> Result<DecodedImage, IoError> {
    let reader = ImageReader::open(path)
        .map_err(|e| IoError::Unsupported(format!("cannot open {}: {e}", path.display())))?
        .with_guessed_format()
        .map_err(|e| IoError::Unsupported(format!("format detection failed: {e}")))?;
    let format = reader.format();
    sanitize_decode(format, reader.into_decoder().map_err(IoError::Decode)?)
}

/// Decode from an in-memory buffer, applying the SAME sanitization (S0)
/// as [`decode_file`] — bomb guard, ICC extraction, orientation bake-in,
/// RGB-f32 normalization. This is the fuzz-target entry (ARCHITECTURE §9
/// "decoders"; issue #25): arbitrary bytes in, no panics out — every
/// rejection is a typed `IoError`.
pub fn decode_bytes(bytes: &[u8]) -> Result<DecodedImage, IoError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| IoError::Unsupported(format!("format detection failed: {e}")))?;
    let format = reader.format();
    sanitize_decode(format, reader.into_decoder().map_err(IoError::Decode)?)
}

/// Shared S0 pipeline over any decoder: dimension bomb guard → ICC →
/// orientation → RGB f32 (straight alpha; see module docs for the M0
/// alpha note) → encoding classification.
fn sanitize_decode(
    format: Option<ImageFormat>,
    mut decoder: impl ImageDecoder,
) -> Result<DecodedImage, IoError> {
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > crate::MAX_PIXELS {
        return Err(IoError::Bomb {
            width: width.into(),
            height: height.into(),
        });
    }
    let icc = decoder.icc_profile().map_err(IoError::Decode)?;
    let orientation = decoder.orientation().map_err(IoError::Decode)?;
    let mut dynamic = DynamicImage::from_decoder(decoder).map_err(IoError::Decode)?;
    dynamic.apply_orientation(orientation);

    // Normalize to RGB f32 (encoded convention; straight alpha). `to_rgb32f`
    // drops alpha — acceptable at M0 (alpha-carrying pipeline lands at M1;
    // documented in CP §8 row 6 scheduling).
    let rgb32f = dynamic.to_rgb32f();
    let (w, h) = (rgb32f.width(), rgb32f.height());
    let pixels = ImageF32::from_interleaved(w, h, Channels::Rgb, rgb32f.into_raw());

    let format = format.ok_or_else(|| IoError::Unsupported("unknown format".into()))?;
    let encoding = match icc {
        Some(bytes) if !bytes.is_empty() => ColorEncoding::Icc { profile: bytes },
        // JPEG/PNG default assumption is sRGB per CP §2; mark Untagged so the
        // UI can warn when the display is wide-gamut.
        _ => ColorEncoding::Untagged,
    };
    Ok(DecodedImage {
        pixels,
        encoding,
        format,
        orientation_applied: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> std::io::Result<Vec<u8>> {
        // 4×2 RGB8 PNG via our own encoder (no raw png-crate usage in tests).
        // Unique panic-safe temp dir (drop = cleanup).
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("tiny_src.png");
        super::super::encode::encode_png8(
            &p,
            4,
            2,
            &[
                255, 0, 0, 0, 255, 0, 0, 0, 255, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            ],
            None,
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::read(&p)
    }

    #[test]
    fn decodes_png_with_orientation_flag() {
        let buf = tiny_png().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tiny.png");
        std::fs::write(&p, buf.clone()).unwrap();
        let img = decode_file(&p).unwrap();
        assert_eq!((img.pixels.width(), img.pixels.height()), (4, 2));
        assert!(img.orientation_applied);
        assert_eq!(img.format, ImageFormat::Png);
        // first pixel is red
        assert!((img.pixels.pixel(0, 0)[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_bytes_agrees_with_decode_file() {
        // The fuzz entry must be the SAME pipeline as the file entry
        // (ARCHITECTURE §9: fuzz the real path, not a parallel one).
        let buf = tiny_png().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tiny.png");
        std::fs::write(&p, buf).unwrap();
        let from_file = decode_file(&p).unwrap();
        let from_bytes = decode_bytes(&tiny_png().unwrap()).unwrap();
        assert_eq!(from_file.format, from_bytes.format);
        assert_eq!(
            (
                from_file.pixels.width(),
                from_file.pixels.height(),
                from_file.pixels.data(),
            ),
            (
                from_bytes.pixels.width(),
                from_bytes.pixels.height(),
                from_bytes.pixels.data(),
            )
        );
    }

    #[test]
    fn decode_bytes_rejects_garbage_typed() {
        // No panic, no hang — a typed error for every garbage class.
        for garbage in [
            &b""[..],
            b"not an image at all",
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A], // truncated PNG magic
            &[0xFF, 0xD8, 0xFF, 0xE0],             // truncated JPEG SOI
        ] {
            let r = decode_bytes(garbage);
            assert!(
                r.is_err(),
                "garbage must be rejected, got Ok for {garbage:?}"
            );
        }
    }
}
