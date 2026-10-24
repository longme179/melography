//! Stage S5 file encoders (COLOR_PIPELINE §6).
//!
//! M0 reality, measured and documented in `docs/CODEC_MATRIX.md`:
//! * **PNG 8/16-bit + ICC** — via `image`'s `PngEncoder` (`set_icc_profile`
//!   trait method). This is the CLI roundtrip/export path.
//! * **TIFF 16-bit** — via `image` (no ICC tag write at M0; gap documented).
//! * **JPEG 8-bit + ICC** — pixels via `image` (quality control); the
//!   embedded profile is spliced in as `ICC_PROFILE` APP2 segments after
//!   encoding (M1, R-OUT-1) — chunked per the JPEG/ICC spec and re-read
//!   in tests (parse-back + sha256 compare).
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

/// Encode 8-bit JPEG with an embedded ICC profile (APP2 `ICC_PROFILE`
/// segments, R-OUT-1). The profile is chunked per the ICC-in-JPEG spec
/// (max 65519 payload bytes per chunk, 1-based sequence numbers).
pub fn encode_jpeg_u8_with_icc(
    path: &Path,
    width: u32,
    height: u32,
    rgb_u8: &[u8],
    quality: u8,
    icc: &[u8],
) -> Result<(), IoError> {
    // 1. Encode pixel data to an in-memory JPEG.
    let mut jpeg = Vec::new();
    {
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
        enc.encode(rgb_u8, width, height, image::ExtendedColorType::Rgb8)
            .map_err(IoError::Decode)?;
    }
    // 2. Splice APP2 ICC segments right after SOI.
    let spliced = splice_app2_icc(&jpeg, icc)?;
    // 3. Write atomically-ish (direct write; export targets are new files).
    std::fs::write(path, &spliced)
        .map_err(|e| IoError::Encode(format!("write {}: {e}", path.display())))?;
    Ok(())
}

/// Insert `ICC_PROFILE` APP2 segments after the SOI marker of a encoded
/// JPEG. Pure byte surgery — no re-encode, no quality loss.
fn splice_app2_icc(jpeg: &[u8], icc: &[u8]) -> Result<Vec<u8>, IoError> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(IoError::Encode(
            "not a JPEG (missing SOI) — cannot splice ICC".into(),
        ));
    }
    const CHUNK_DATA_MAX: usize = 65_533 - 14; // APP2 max payload minus header
    let chunks: Vec<&[u8]> = if icc.is_empty() {
        Vec::new()
    } else {
        icc.chunks(CHUNK_DATA_MAX).collect()
    };
    let total = chunks.len().max(1);
    if total > 255 {
        return Err(IoError::Encode(format!(
            "ICC profile needs {total} APP2 chunks (max 255)"
        )));
    }

    let mut out = Vec::with_capacity(jpeg.len() + icc.len() + chunks.len() * 20);
    out.extend_from_slice(&jpeg[..2]); // SOI
    for (i, chunk) in chunks.iter().enumerate() {
        let payload_len = 14 + chunk.len(); // "ICC_PROFILE\0" + seq(2) + data
        let segment_len = payload_len + 2; // + length field itself
        if segment_len > 65_535 {
            return Err(IoError::Encode("APP2 segment overflow".into()));
        }
        out.extend_from_slice(&[0xFF, 0xE2]); // APP2 marker
        out.extend_from_slice(&(segment_len as u16).to_be_bytes());
        out.extend_from_slice(b"ICC_PROFILE\0");
        out.push((i + 1) as u8); // sequence number, 1-based
        out.push(total as u8);
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&jpeg[2..]);
    Ok(out)
}

/// Parse `ICC_PROFILE` APP2 segments back out of a JPEG (test + roundtrip
/// evidence; also the basis for "profile honored on re-import" checks).
pub fn extract_app2_icc(jpeg: &[u8]) -> Result<Option<Vec<u8>>, IoError> {
    if jpeg.len() < 2 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return Err(IoError::Encode("not a JPEG (missing SOI)".into()));
    }
    let mut i = 2usize;
    let mut chunks: Vec<Option<Vec<u8>>> = Vec::new();
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            return Err(IoError::Encode(format!(
                "expected marker at byte {i}, found 0x{:02X}",
                jpeg[i]
            )));
        }
        let marker = jpeg[i + 1];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2; // standalone markers
            continue;
        }
        if marker == 0xDA {
            break; // SOS — entropy data follows; APP2s live before it
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            return Err(IoError::Encode(format!("bad segment length at {i}")));
        }
        let payload = &jpeg[i + 4..i + 2 + len];
        if marker == 0xE2 && payload.len() >= 14 && &payload[..12] == b"ICC_PROFILE\0" {
            let seq = payload[12] as usize;
            let total = payload[13] as usize;
            if seq == 0 || total == 0 {
                return Err(IoError::Encode("ICC APP2 with zero seq/count".into()));
            }
            while chunks.len() < total {
                chunks.push(None);
            }
            if seq > total {
                return Err(IoError::Encode("ICC APP2 seq > count".into()));
            }
            if chunks[seq - 1].is_some() {
                return Err(IoError::Encode("duplicate ICC APP2 seq".into()));
            }
            chunks[seq - 1] = Some(payload[14..].to_vec());
        }
        i += 2 + len;
    }
    if chunks.is_empty() {
        return Ok(None);
    }
    let mut icc = Vec::new();
    for (idx, c) in chunks.into_iter().enumerate() {
        match c {
            Some(bytes) => icc.extend_from_slice(&bytes),
            None => {
                return Err(IoError::Encode(format!(
                    "ICC APP2 chunk {} missing (incomplete profile)",
                    idx + 1
                )))
            }
        }
    }
    Ok(Some(icc))
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
    fn jpeg_app2_icc_roundtrip_single_and_multi_chunk() {
        let (w, h, px) = crate::chart::synthetic_chart_u8();
        let dir = std::env::temp_dir().join("melography_io_jpeg");
        std::fs::create_dir_all(&dir).unwrap();

        // Single-chunk: a small real profile.
        let icc = melography_color::icc_bytes_deterministic(&melography_color::srgb_profile());
        let p = dir.join("icc1.jpg");
        encode_jpeg_u8_with_icc(&p, w, h, &px, 90, &icc).unwrap();
        let jpeg = std::fs::read(&p).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        let extracted = extract_app2_icc(&jpeg).unwrap().expect("profile present");
        assert_eq!(extracted, icc, "single-chunk APP2 must round-trip");
        // And the image itself still decodes.
        let decoded = crate::decode::decode_file(&p).unwrap();
        assert_eq!(decoded.pixels.width(), w);

        // Multi-chunk: force chunking with a >65519-byte payload (padding
        // a synthetic profile-like blob).
        let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let p2 = dir.join("icc2.jpg");
        encode_jpeg_u8_with_icc(&p2, w, h, &px, 90, &big).unwrap();
        let jpeg2 = std::fs::read(&p2).unwrap();
        let extracted2 = extract_app2_icc(&jpeg2).unwrap().expect("profile present");
        assert_eq!(extracted2.len(), big.len());
        assert_eq!(extracted2, big, "multi-chunk APP2 must round-trip in order");
    }

    #[test]
    fn plain_jpeg_has_no_icc_segment() {
        let (w, h, px) = crate::chart::synthetic_chart_u8();
        let dir = std::env::temp_dir().join("melography_io_jpeg");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plain.jpg");
        encode_jpeg_u8(&p, w, h, &px, 90).unwrap();
        let jpeg = std::fs::read(&p).unwrap();
        assert!(extract_app2_icc(&jpeg).unwrap().is_none());
    }

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
