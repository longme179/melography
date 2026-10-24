//! # melography-core
//!
//! Shared vocabulary for the MELOGRAPHY processing pipeline: image buffer
//! types, region/margin geometry, color-encoding descriptors, the stage
//! contract skeleton, and the cross-crate error taxonomy.
//!
//! **M0 scope** (PLAN.md): this crate establishes the *types* the other
//! crates communicate through. The scheduling/cache machinery (tiling
//! scheduler, stage cache) arrives with M1/M2 rendering work; the trait
//! signatures below are the frozen contract surface (ARCHITECTURE §3).
//!
//! This crate is UI-agnostic by construction (no Tauri, no GPU types) —
//! enforced by dependency direction and cargo-deny bans.

use std::fmt;

// ─── Image buffers ──────────────────────────────────────────────────────────

/// Channel count of an image buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channels {
    Rgb,
    Rgba,
}

impl Channels {
    #[must_use]
    pub fn count(self) -> usize {
        match self {
            Channels::Rgb => 3,
            Channels::Rgba => 4,
        }
    }
}

/// Owned interleaved f32 image buffer — the pipeline's universal pixel type
/// after stage S1 (input color transform). Values are *unbounded* linear:
/// negatives and >1.0 survive until a documented clamp boundary
/// (COLOR_PIPELINE §1 "hard rules").
#[derive(Debug, Clone, PartialEq)]
pub struct ImageF32 {
    width: u32,
    height: u32,
    channels: Channels,
    data: Vec<f32>,
}

impl ImageF32 {
    /// Builds an image from an interleaved buffer. Panics if `data.len()`
    /// does not match `width * height * channels.count()` — callers transform
    /// external data at the boundary where sizes are already validated.
    #[must_use]
    pub fn from_interleaved(width: u32, height: u32, channels: Channels, data: Vec<f32>) -> Self {
        let expected = width as usize * height as usize * channels.count();
        assert_eq!(
            data.len(),
            expected,
            "ImageF32 buffer size mismatch: {expected} expected, got {}",
            data.len()
        );
        Self {
            width,
            height,
            channels,
            data,
        }
    }

    #[must_use]
    pub fn alloc(width: u32, height: u32, channels: Channels, fill: f32) -> Self {
        Self::from_interleaved(
            width,
            height,
            channels,
            vec![fill; width as usize * height as usize * channels.count()],
        )
    }

    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }
    #[must_use]
    pub fn channels(&self) -> Channels {
        self.channels
    }
    #[must_use]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    /// Mutable access is deliberately constrained: geometry is immutable,
    /// only pixel values may change in place (stage in/out contracts rely on
    /// stable geometry).
    #[must_use]
    pub fn data_mut(&mut self) -> &mut [f32] {
        &mut self.data
    }

    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> &[f32] {
        let c = self.channels.count();
        let i = (y as usize * self.width as usize + x as usize) * c;
        &self.data[i..i + c]
    }

    /// Mutable single-pixel accessor (geometry-safe counterpart of
    /// [`ImageF32::pixel`]; introduced at M1 for tile assembly and app
    /// preview composition).
    pub fn pixel_mut(&mut self, x: u32, y: u32) -> &mut [f32] {
        let c = self.channels.count();
        let i = (y as usize * self.width as usize + x as usize) * c;
        &mut self.data[i..i + c]
    }

    /// Byte length of the f32 payload (little-endian on all supported hosts).
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.data.len() * 4
    }

    /// Raw slice view for hashing/serialization (determinism gates).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        // f32 has no padding; this is the standard safe view (no alignment
        // hazard: we only *read*).
        unsafe { std::slice::from_raw_parts(self.data.as_ptr().cast::<u8>(), self.byte_len()) }
    }
}

// ─── Geometry ───────────────────────────────────────────────────────────────

/// Axis-aligned pixel region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Tile-overlap requirement a stage declares (ARCHITECTURE §3 `halo()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Margins {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Margins {
    #[must_use]
    pub fn uniform(v: u32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }
}

// ─── Color encoding descriptors ─────────────────────────────────────────────

/// Identifies the colorimetric meaning of pixel values crossing a stage
/// boundary. The full ICC path carries profile bytes; fast paths name the
/// space directly (COLOR_PIPELINE §1 stage contracts).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColorEncoding {
    /// sRGB IEC 61966-2-1 (D65, sRGB transfer), 8-bit-code or linear floats
    /// depending on `linear` flag.
    Srgb { linear: bool },
    /// Display P3 (D65, P3 transfer or linear).
    DisplayP3 { linear: bool },
    /// ACES AP1 (ACEScg primaries, ACES white ~D60), always linear.
    Ap1Linear,
    /// Arbitrary ICC profile (bytes as embedded in the source file).
    Icc { profile: Vec<u8> },
    /// Unknown/untagged — decode layer falls back to format default with a
    /// UI warning (COLOR_PIPELINE §2).
    Untagged,
}

// ─── Stage contract (frozen surface, M0 skeleton) ──────────────────────────

/// Stable stage identifier serialized into recipes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StageId(pub String);

impl StageId {
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Determinism class of a stage (ARCHITECTURE §3): pure stages reproduce
/// bit-exactly; seeded stages derive all randomness from parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeterminismClass {
    Deterministic,
    SeededDeterministic,
}

/// Input/output contract of a pipeline stage (COLOR_PIPELINE §1 table).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StageSpec {
    pub input: ColorEncoding,
    pub output: ColorEncoding,
}

/// M0-minimal stage contract. Typed per-stage parameters (`StageParams`) and
/// the `RenderContext` scheduler arrive with M1 rendering; the shape below is
/// the part later traits must keep (ARCHITECTURE §3 "boundaries fixed now").
pub trait Stage {
    fn id(&self) -> StageId;
    fn spec(&self) -> StageSpec;
    fn determinism(&self) -> DeterminismClass;
    /// Tile-overlap requirement in working-scale pixels.
    fn halo(&self) -> Margins {
        Margins::default()
    }
}

// ─── Error taxonomy ─────────────────────────────────────────────────────────

/// Error classes surfaced to the UI (ARCHITECTURE §8). Crates wrap their
/// concrete errors into this taxonomy at the boundary; messages are
/// user-presentable, causes keep the technical chain.
#[derive(Debug, thiserror::Error)]
pub enum MelographyError {
    #[error("input/output error: {0}")]
    Io(#[source] anyhow_like::BoxError),
    #[error("color management error: {0}")]
    Color(#[source] anyhow_like::BoxError),
    #[error("film chain error: {0}")]
    Film(#[source] anyhow_like::BoxError),
    #[error("LUT error: {0}")]
    Lut(#[source] anyhow_like::BoxError),
    #[error("GPU/render error: {0}")]
    Gpu(#[source] anyhow_like::BoxError),
    #[error("AI model error: {0}")]
    Ai(#[source] anyhow_like::BoxError),
    #[error("preset/recipe error: {0}")]
    Preset(#[source] anyhow_like::BoxError),
}

/// Minimal boxed error alias (avoids adding `anyhow` to the core vocabulary;
/// each crate supplies its own concrete `std::error::Error` impl).
pub mod anyhow_like {
    pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
}

// ─── Progress & cancellation (M0 minimal, extended at M1) ──────────────────

/// Cooperative cancellation token shared between UI and render threads.
#[derive(Debug, Default)]
pub struct CancelToken {
    cancelled: std::sync::atomic::AtomicBool,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_f32_roundtrip_layout() {
        let mut img = ImageF32::alloc(3, 2, Channels::Rgb, 0.5);
        assert_eq!(img.data().len(), 18);
        assert_eq!(img.byte_len(), 72);
        assert_eq!(img.as_bytes().len(), 72);
        img.data_mut().fill(1.0);
        assert_eq!(img.pixel(2, 1), &[1.0, 1.0, 1.0]);
    }

    #[test]
    #[should_panic(expected = "size mismatch")]
    fn image_f32_rejects_bad_buffer() {
        let _ = ImageF32::from_interleaved(2, 2, Channels::Rgb, vec![0.0; 11]);
    }

    #[test]
    fn channels_counts() {
        assert_eq!(Channels::Rgb.count(), 3);
        assert_eq!(Channels::Rgba.count(), 4);
    }

    #[test]
    fn color_encoding_serde_roundtrip() {
        let enc = ColorEncoding::Ap1Linear;
        let s = serde_json::to_string(&enc).unwrap();
        assert!(s.contains("ap1_linear"));
        let back: ColorEncoding = serde_json::from_str(&s).unwrap();
        assert_eq!(enc, back);
    }
}
