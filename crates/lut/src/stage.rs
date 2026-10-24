//! LUT stage application: insertion points, encoding wrap, intensity mix
//! (luts.md §3, PLAN M1, issue #7).
//!
//! **Insertion points** position the LUT inside the render pipeline
//! (UX_SPEC §6 LUT panel; luts.md §3). With the M1 identity film chain the
//! four points are distinct *positions* around the (currently identity)
//! film stage — the serialization and pipeline placement are real from day
//! one so M2's real film stages drop in without a recipe-format change:
//!
//! ```text
//! decode → AP1 ─→ [pre-film LUTs] ─→ [film-input LUTs] ─→ FILM (identity
//! in M1) ─→ [print-output LUTs] ─→ [finishing LUTs] ─→ display/export
//! ```
//!
//! **Intensity/mix** lerps between the un-LUT'd working value and the
//! LUT'd value **in AP1 working space** (documented; matches darktable's
//! module opacity semantics in spirit):
//!
//! ```text
//! out = lerp(v_ap1, unwrap(lut(wrap(v_ap1))), intensity)
//! ```
//!
//! **Domain clipping** is counted per render and returned in
//! [`ApplyReport`] — surfaced as the amber "LUT domain clipping" UI badge
//! (luts.md §2: clamp logged once per render, never silent).

use crate::cube::CubeLut;
use crate::encoding::{unwrap_from, wrap_to, LutEncoding};
use crate::tetra;
use melography_core::ImageF32;
use serde::{Deserialize, Serialize};

/// Pipeline position of a LUT (luts.md §3 fixed list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InsertionPoint {
    PreFilm,
    FilmInput,
    PrintOutput,
    #[default]
    Finishing,
}

impl InsertionPoint {
    /// Stable order used to sort stages within a render pass (serialization
    /// keeps recipe order; the pipeline applies in this canonical order).
    #[must_use]
    pub fn order(self) -> u8 {
        match self {
            Self::PreFilm => 0,
            Self::FilmInput => 1,
            Self::PrintOutput => 2,
            Self::Finishing => 3,
        }
    }
}

/// A configured LUT application (one recipe stage entry).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LutApply {
    /// Content hash of the library LUT this stage refers to (library
    /// indirection — ARCHITECTURE §7; the bytes live in the library, the
    /// recipe only carries the reference).
    pub lut_hash: String,
    pub insertion: InsertionPoint,
    /// 0.0–1.0 mix of the LUT effect (UI shows 0–100 %).
    pub intensity: f32,
    pub declared_input: LutEncoding,
    pub declared_output: LutEncoding,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl LutApply {
    #[must_use]
    pub fn new(
        lut_hash: impl Into<String>,
        insertion: InsertionPoint,
        intensity: f32,
        declared_input: LutEncoding,
        declared_output: LutEncoding,
    ) -> Self {
        Self {
            lut_hash: lut_hash.into(),
            insertion,
            intensity: intensity.clamp(0.0, 1.0),
            declared_input,
            declared_output,
            enabled: true,
        }
    }

    /// Apply to an AP1-linear working image. `lut` must be the library LUT
    /// matching `self.lut_hash` (callers verify the hash — see
    /// `library.rs`); this function is the pure math core (CPU oracle).
    ///
    /// Determinism: pure function of (`input`, `lut`, `self`) — the
    /// tiled-vs-monolithic suite relies on that (point op, halo 0).
    pub fn apply(
        &self,
        lut: &CubeLut,
        input: &ImageF32,
    ) -> Result<(ImageF32, ApplyReport), LutError> {
        if !self.enabled {
            return Ok((input.clone(), ApplyReport::default()));
        }
        if !(0.0..=1.0).contains(&self.intensity) {
            return Err(LutError::Malformed {
                detail: format!("intensity {} outside 0..1", self.intensity),
            });
        }
        if !self.declared_input.implemented() {
            return Err(LutError::EncodingNotImplemented(
                self.declared_input.as_str(),
            ));
        }
        if !self.declared_output.implemented() {
            return Err(LutError::EncodingNotImplemented(
                self.declared_output.as_str(),
            ));
        }
        let mut out = ImageF32::alloc(input.width(), input.height(), input.channels(), 0.0);
        let mut report = ApplyReport {
            insertion: self.insertion,
            ..ApplyReport::default()
        };
        let px_ch = input.channels().count();
        for (src, dst) in input
            .data()
            .chunks_exact(px_ch)
            .zip(out.data_mut().chunks_exact_mut(px_ch))
        {
            let rgb = [src[0], src[1], src[2]];
            let v_enc = wrap_to(rgb, self.declared_input)?;
            let (lut_out, clamped) = tetra::sample(lut, v_enc);
            if clamped.any() {
                report.clamped_pixels += 1;
            }
            let w = unwrap_from(lut_out, self.declared_output)?;
            // Intensity mix in AP1 (module doc). Intensity 1.0 = pure LUT.
            let mixed = [
                rgb[0] + (w[0] - rgb[0]) * self.intensity,
                rgb[1] + (w[1] - rgb[1]) * self.intensity,
                rgb[2] + (w[2] - rgb[2]) * self.intensity,
            ];
            dst[0] = mixed[0];
            dst[1] = mixed[1];
            dst[2] = mixed[2];
            if px_ch == 4 {
                dst[3] = src[3];
            }
        }
        Ok((out, report))
    }
}

/// Per-render aggregate of a LUT stage run (drives UI badges + logs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ApplyReport {
    pub insertion: InsertionPoint,
    /// Pixels where any channel clamped to the LUT domain (amber badge).
    pub clamped_pixels: u64,
}

/// Apply a chain of LUT stages in canonical insertion order. The caller
/// resolves each `LutApply.lut_hash` to its parsed [`CubeLut`] (library
/// lookup + hash re-verification happens there).
pub fn apply_chain<'a, I>(
    stages: I,
    resolve: &dyn Fn(&LutApply) -> Result<CubeLut, LutError>,
    input: &ImageF32,
) -> Result<(ImageF32, Vec<ApplyReport>), LutError>
where
    I: IntoIterator<Item = &'a LutApply>,
{
    let mut stages: Vec<&LutApply> = stages.into_iter().collect();
    stages.sort_by_key(|s| s.insertion.order());
    let mut img = input.clone();
    let mut reports = Vec::new();
    for stage in stages {
        if !stage.enabled {
            continue;
        }
        let lut = resolve(stage)?;
        let (out, report) = stage.apply(&lut, &img)?;
        img = out;
        reports.push(report);
    }
    Ok((img, reports))
}

// LutError lives in lib.rs (M0 taxonomy, extended at M1); re-export the
// pieces the stage layer needs so external users have one import point.
pub use crate::LutError;
