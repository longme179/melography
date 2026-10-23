//! Stage S5 file encoders (COLOR_PIPELINE §6).
//!
//! M0 reality, measured and documented in `docs/CODEC_MATRIX.md`:
//! * **PNG 8/16-bit + ICC** — via `image`'s `PngEncoder` (`set_icc_profile`
//!   trait method). This is the CLI roundtrip/export path.
//! * **TIFF 16-bit** — via `image` (no ICC tag write at M0; gap documented).
//! * **JPEG 8-bit** — via `image` (quality control; no APP2 ICC write at
//!   M0 — APP2 splicing is an M1 export task, 20 lines + tests).
//!
//! Bit depth/alpha support is *per-format factual*: impossible combinations
//! are absent from these APIs (they never silently convert).

use crate::IoError;
use image::ImageEncoder;
use std::io::Write;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
#[error("encode error: {0}")]
pub struct IoEncodeError(pub String);

/// Encode 16-bit RGB PNG with an embedded ICC profile.
pub fn encode_png16(
    path: &Path,
    width: u32,
    height: u32,
    rgb_u16: &[u16],
    icc: Option<&[u8]>,
) -> Result<(), IoError> {
    let file = std::fs::File::create(path)
        .map_err(|e| IoError::Encode(format!("create {}: {e}", path.display())))?;
    let mut enc = image::codecs::png::PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    );
    if let Some(icc) = icc {
        enc.set_icc_profile(icc.to_vec())
            .map_err(|e| IoError::Encode(format!("icc attach: {e}")))?;
    }
    let bytes: &[u8] = bytemuck::cast_slice(rgb_u16);
    enc.write_image(bytes, width, height, image::ExtendedColorType::Rgb16)
        .map_err(IoError::Decode)?;
    Ok(())
}

/// Encode 8-bit RGB PNG with an embedded ICC profile.
pub fn encode_png8(
    path: &Path,
    width: u32,
    height: u32,
    rgb_u8: &[u8],
    icc: Option<&[u8]>,
) -> Result<(), IoError> {
    let file = std::fs::File::create(path)
        .map_err(|e| IoError::Encode(format!("create {}: {e}", path.display())))?;
    let mut enc = image::codecs::png::PngEncoder::new_with_quality(
        std::io::BufWriter::new(file),
        image::codecs::png::CompressionType::Default,
        image::codecs::png::FilterType::Adaptive,
    );
    if let Some(icc) = icc {
        enc.set_icc_profile(icc.to_vec())
            .map_err(|e| IoError::Encode(format!("icc attach: {e}")))?;
    }
    enc.write_image(rgb_u8, width, height, image::ExtendedColorType::Rgb8)
        .map_err(IoError::Decode)?;
    Ok(())
}

/// Encode 8-bit JPEG at the given quality (1–100).
pub fn encode_jpeg_u8(
    path: &Path,
    width: u32,
    height: u32,
    rgb_u8: &[u8],
    quality: u8,
) -> Result<(), IoError> {
    let file = std::fs::File::create(path)
        .map_err(|e| IoError::Encode(format!("create {}: {e}", path.display())))?;
    let mut buf = std::io::BufWriter::new(file);
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    enc.encode(rgb_u8, width, height, image::ExtendedColorType::Rgb8)
        .map_err(IoError::Decode)?;
    buf.flush()
        .map_err(|e| IoError::Encode(format!("flush: {e}")))?;
    Ok(())
}

/// Encode 16-bit TIFF (uncompressed; deflate is an M1 option).
pub fn encode_tiff_u16(
    path: &Path,
    width: u32,
    height: u32,
    rgb_u16: &[u16],
) -> Result<(), IoError> {
    let file = std::fs::File::create(path)
        .map_err(|e| IoError::Encode(format!("create {}: {e}", path.display())))?;
    let mut buf = std::io::BufWriter::new(file);
    let enc = image::codecs::tiff::TiffEncoder::new(&mut buf);
    let bytes: &[u8] = bytemuck::cast_slice(rgb_u16);
    enc.write_image(bytes, width, height, image::ExtendedColorType::Rgb16)
        .map_err(IoError::Decode)?;
    buf.flush()
        .map_err(|e| IoError::Encode(format!("flush: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png16_roundtrip_via_decode() {
        let dir = std::env::temp_dir().join("melography_io_enc");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("rt16.png");
        let (w, h, px) = crate::chart::synthetic_chart_u16();
        encode_png16(&p, w, h, &px, None).unwrap();
        let decoded = crate::decode::decode_file(&p).unwrap();
        assert_eq!(decoded.pixels.width(), w);
        // code value reconstruction: u16 → f32 code (x/65535) → u16 again
        let got: Vec<u16> = decoded
            .pixels
            .data()
            .iter()
            .map(|&c| (c * 65_535.0).round() as u16)
            .collect();
        assert_eq!(got, px, "16-bit PNG roundtrip must be lossless");
    }
}
