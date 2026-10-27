//! Film chain composition (stages A–I) and the strip executor.
//!
//! Stage order (FILM_MODEL §A–§I):
//!
//! ```text
//! input (AP1-linear RGB f32)
//!   → A/B   vignette + veiling bloom        (per-pixel / gaussian margin)
//!   → C1,C3 excitation M_exc + EV + anchor  (per-pixel)
//!   → C2    halation                         (gaussian margin)
//!   → D/E   H&D response + chemistry         (per-pixel)
//!   → G     grain                            (lattice margin)
//!   → F     print/scan inversion             (per-pixel)
//!   → AP1-linear output (I: export stage owns the final transform)
//! ```
//!
//! **Tiling model.** For an output strip `[y, y1)` the row regions each
//! stage must cover are computed top-down from declared margins:
//!
//! ```text
//! r1 = expand(strip, m_grain)        // density rows (response output)
//! r2 = expand(r1,    m_halation)     // excitation rows
//! r3 = expand(r2,    m_veiling)      // vignetted rows
//! ```
//!
//! Every stage then runs over exactly its region with the same fixed tap
//! windows and summation order as any other tiling — byte-identical to
//! the monolithic render by construction (verified by the §L gate).
//!
//! **Memory.** Three strip-local planes (A/B/T) sized
//! `(strip_h + 2·m_total)` rows — ~44 MB each at 6000 px width — plus the
//! input/output images. Peak ≈ input + output + 3 planes, comfortably
//! inside the ≤ 2 GiB @ 50 MP budget (R-REN-2).
//! **Threading (A2.1).** `render_threaded` splits the image into
//! contiguous chunks, one worker thread per chunk. Strips are
//! independent by the tiling model above (shared read-only inputs,
//! disjoint output rows), so chunking is just another strip partition
//! and the output stays byte-identical — the §L gate pins that for
//! every worker count. Each chunk carries its own margin set, so N
//! workers need `h + N·2·m_total` plane rows; the worker cap keeps that
//! within 2× the monolithic footprint.

use crate::emulsion::{self, Adjustments, Emulsion};
use crate::frame::FilmFrame;
use crate::grain::GrainStage;
use crate::optics::{self, GaussianKernel, LensParams, Plane};
use crate::print::Print;
use crate::profile::StockProfile;
use melography_core::{Channels, ImageF32};

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("film chain: {0}")]
    BadParams(String),
    #[error("film chain: input must be RGB, got {0:?}")]
    NotRgb(Channels),
}

/// Everything a render needs: validated profile + adjustments + lens.
#[derive(Clone)]
pub struct FilmChainParams {
    pub profile: StockProfile,
    pub adjustments: Adjustments,
    pub lens: LensParams,
    /// A2.2 — render the grain stage through the fast approximation
    /// (FILM_MODEL §G "interactive fast path"). Preview-only: export and
    /// the CLI never set it, so the §L reference path is the full model.
    pub grain_fast: bool,
}

impl FilmChainParams {
    pub fn new(profile: StockProfile) -> Result<Self, ChainError> {
        profile
            .validate()
            .map_err(|e| ChainError::BadParams(e.to_string()))?;
        let p = Self {
            profile,
            adjustments: Adjustments::default(),
            lens: LensParams::default(),
            grain_fast: false,
        };
        p.validate()?;
        Ok(p)
    }

    pub fn with(mut self, adjustments: Adjustments) -> Result<Self, ChainError> {
        self.adjustments = adjustments;
        self.validate()?;
        Ok(self)
    }

    /// Full recipe-level parameterization (adjustments + lens).
    pub fn with_adjustments(
        mut self,
        adjustments: Adjustments,
        lens: LensParams,
    ) -> Result<Self, ChainError> {
        self.adjustments = adjustments;
        self.lens = lens;
        self.validate()?;
        Ok(self)
    }

    /// A2.2 — select the grain fast path (preview leg only).
    pub fn with_grain_fast(mut self, fast: bool) -> Self {
        self.grain_fast = fast;
        self
    }

    pub fn validate(&self) -> Result<(), ChainError> {
        self.profile
            .validate()
            .map_err(|e| ChainError::BadParams(e.to_string()))?;
        self.adjustments.validate().map_err(ChainError::BadParams)?;
        self.lens.validate().map_err(ChainError::BadParams)?;
        Ok(())
    }
}

/// Render the film chain monolithically (single strip).
pub fn render(img: &ImageF32, params: &FilmChainParams) -> Result<ImageF32, ChainError> {
    render_tiled(img, params, img.height())
}

/// Render in horizontal strips of `strip_h` output rows (byte-identical to
/// `render` for any `strip_h ≥ 1` — the tiled-vs-monolithic gate verifies).
pub fn render_tiled(
    img: &ImageF32,
    params: &FilmChainParams,
    strip_h: u32,
) -> Result<ImageF32, ChainError> {
    params.validate()?;
    if img.channels() != Channels::Rgb {
        return Err(ChainError::NotRgb(img.channels()));
    }
    if !params.adjustments.enabled {
        return Ok(img.clone());
    }
    let (w, h) = (img.width(), img.height());
    let st = ChainStages::resolve(params, w, h);

    let strip_h = strip_h.clamp(1, h);
    let buf_rows = (strip_h + 2 * st.m_total()).clamp(1, h);
    let mut planes = StripPlanes::new(w, buf_rows);

    let mut out_img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    let out = out_img.data_mut();
    let mut strip_y = 0u32;
    while strip_y < h {
        let strip_y1 = (strip_y + strip_h).min(h);
        render_strip(img, &st, &mut planes, strip_y, strip_y1, out, 0);
        strip_y = strip_y1;
    }
    Ok(out_img)
}

/// A2.1 — render with the image split into `workers` contiguous chunks,
/// one thread per chunk. **Byte-identical to [`render`] for any worker
/// count**: a chunk is just a strip of its own height, and the §L gate
/// (any strip partition reproduces the monolithic bytes) extends to
/// chunk partitions. Chunks read only shared read-only state (`img`, the
/// once-resolved stages) and write only their own contiguous output
/// rows — strips are independent by the tiling model, so there is no
/// synchronization and no order dependence to reason about.
///
/// `workers: 0` = one per logical core. The count is clamped so every
/// worker gets at least one output row and the total scratch stays
/// bounded (see [`worker_cap`]): N chunks need `h + N·2·m_total` plane
/// rows because each carries its own margin set.
///
/// The sequential executors (`render`, `render_tiled`) remain the
/// reference path; the preview leg uses this one, export stays
/// sequential (memory-first, R-REN-2).
pub fn render_threaded(
    img: &ImageF32,
    params: &FilmChainParams,
    workers: u32,
) -> Result<ImageF32, ChainError> {
    params.validate()?;
    if img.channels() != Channels::Rgb {
        return Err(ChainError::NotRgb(img.channels()));
    }
    if !params.adjustments.enabled {
        return Ok(img.clone());
    }
    let (w, h) = (img.width(), img.height());
    let st = ChainStages::resolve(params, w, h);
    let m_total = st.m_total();
    // Shared read-only across workers: bind the reference so the move
    // closures below capture `&ChainStages` (Copy), not the struct by
    // value (E0382 otherwise — a `move ||` that writes `&st` captures
    // `st` itself).
    let st = &st;

    let requested = if workers == 0 {
        std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
    } else {
        workers
    };
    let n = requested.clamp(1, h.max(1)).min(worker_cap(h, m_total));

    let mut out_img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    if n <= 1 {
        // One chunk = the whole image: the exact monolithic render.
        let buf_rows = (h + 2 * m_total).clamp(1, h);
        let mut planes = StripPlanes::new(w, buf_rows);
        render_strip(img, st, &mut planes, 0, h, out_img.data_mut(), 0);
        return Ok(out_img);
    }

    // Contiguous row chunks; the remainder spreads over the first chunks.
    let base = h / n;
    let rem = h % n;
    let mut chunks: Vec<(u32, u32)> = Vec::with_capacity(n as usize);
    let mut y = 0u32;
    for k in 0..n {
        let rows = base + u32::from(k < rem);
        chunks.push((y, y + rows));
        y += rows;
    }

    // Disjoint interleaved-RGB output slices, one per chunk.
    let mut rest = out_img.data_mut();
    let mut outs: Vec<&mut [f32]> = Vec::with_capacity(chunks.len());
    for &(y0, y1) in &chunks {
        let len = (y1 - y0) as usize * w as usize * 3;
        let (head, tail) = rest.split_at_mut(len);
        outs.push(head);
        rest = tail;
    }

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());
        for (&(y0, y1), out) in chunks.iter().zip(outs) {
            handles.push(scope.spawn(move || {
                let buf_rows = (y1 - y0 + 2 * m_total).clamp(1, h);
                let mut planes = StripPlanes::new(w, buf_rows);
                render_strip(img, st, &mut planes, y0, y1, out, y0);
            }));
        }
        // A panicking worker resumes its panic in the caller — the same
        // failure mode as a sequential render (no swallowing, no wrap).
        for handle in handles {
            if let Err(payload) = handle.join() {
                std::panic::resume_unwind(payload);
            }
        }
    });
    Ok(out_img)
}

/// Upper bound on concurrent chunk workers (A2.1): total plane rows are
/// `h + n·2·m_total`, and the cap keeps them ≤ 2× the monolithic
/// footprint `h + 2·m_total` — i.e. `n ≤ (h + 4·m_total) / (2·m_total)`.
/// With no neighbor stages (`m_total == 0`) memory does not constrain
/// the split. Saturating arithmetic: `radius_px` is only u32-bounded for
/// pathological σ, and the cap must never itself overflow.
fn worker_cap(h: u32, m_total: u32) -> u32 {
    if m_total == 0 {
        u32::MAX
    } else {
        (h.saturating_add(m_total.saturating_mul(4)) / (2 * m_total)).max(1)
    }
}

/// Stages resolved ONCE per render and shared read-only by every strip
/// (and, in [`render_threaded`], by every worker thread — every field is
/// plain data, so the struct is `Sync` by construction).
/// `pub(crate)` (A2.4): the GPU chain (`gpu_chain`) resolves the same
/// stages and packs them for the WGSL kernels.
pub(crate) struct ChainStages {
    pub(crate) frame: FilmFrame,
    pub(crate) lens: LensParams,
    pub(crate) em: Emulsion,
    pub(crate) grain: GrainStage,
    pub(crate) print: Print,
    pub(crate) veiling: GaussianKernel,
    pub(crate) veiling_on: bool,
    /// Recipe knobs the strip body needs per pixel (the rest are folded
    /// into the resolved stages above).
    pub(crate) ev: f64,
    pub(crate) pre_desaturate: f64,
    pub(crate) m_veiling: u32,
    pub(crate) m_halation: u32,
    pub(crate) m_grain: u32,
}

impl ChainStages {
    pub(crate) fn resolve(params: &FilmChainParams, w: u32, h: u32) -> Self {
        let frame = FilmFrame::from_dims(w, h, params.profile.crop_rule);
        let mmpp = frame.mm_per_pixel;

        let em: Emulsion = emulsion::resolve_emulsion(&params.profile, &params.adjustments, mmpp);
        let grain = GrainStage::resolve(
            &params.profile,
            params.adjustments.grain_intensity,
            params.adjustments.grain_seed,
            params.grain_fast,
        );
        let print = Print::resolve(
            &params.profile,
            params.adjustments.inversion_path,
            params.adjustments.print_exposure_stops,
        );
        let veiling_on = params.lens.veiling_amount > 0.0;
        let veiling = GaussianKernel::new(params.lens.veiling_sigma_mm / mmpp);
        let veiling_on = veiling_on && !veiling.is_identity();

        let m_veiling = if veiling_on { veiling.radius_px } else { 0 };
        let m_halation = em.input_margin();
        let m_grain = grain.margin_px(mmpp);

        Self {
            frame,
            lens: params.lens,
            em,
            grain,
            print,
            veiling,
            veiling_on,
            ev: params.adjustments.ev,
            pre_desaturate: params.adjustments.pre_desaturate,
            m_veiling,
            m_halation,
            m_grain,
        }
    }

    fn m_total(&self) -> u32 {
        self.m_veiling + self.m_halation + self.m_grain
    }
}

/// Strip-local scratch planes (A/B/T): reused across strips by the
/// sequential executors, owned per worker by the threaded one.
struct StripPlanes {
    a: Plane,
    b: Plane,
    t: Plane,
}

impl StripPlanes {
    fn new(w: u32, rows: u32) -> Self {
        Self {
            a: Plane::new(w, rows),
            b: Plane::new(w, rows),
            t: Plane::new(w, rows),
        }
    }
}

/// Render output rows `[strip_y, strip_y1)` into `out` — an interleaved
/// RGB f32 slice whose row 0 is image row `out_origin` (0 for the
/// sequential executors, the chunk origin for the threaded one).
///
/// THE single strip body shared by every executor (monolithic, tiled,
/// threaded) — one code path is what makes them byte-identical, and the
/// §L gate pins that contract from the outside.
#[allow(clippy::too_many_arguments)]
fn render_strip(
    img: &ImageF32,
    st: &ChainStages,
    planes: &mut StripPlanes,
    strip_y: u32,
    strip_y1: u32,
    out: &mut [f32],
    out_origin: u32,
) {
    let (w, h) = (img.width(), img.height());
    // Field borrows (disjoint by construction, same as the old locals):
    // the big stage structs by reference, the Copy knobs via `st.…`.
    let ChainStages {
        frame,
        lens,
        em,
        grain,
        print,
        veiling,
        ..
    } = st;
    let StripPlanes {
        a: plane_a,
        b: plane_b,
        t: plane_t,
    } = planes;
    // Row regions (absolute, clamped): each stage's coverage derives
    // top-down from the margins of the stages above it.
    let r1 = Region::expand(strip_y, strip_y1, st.m_grain, h); // density
    let r2 = Region::expand(r1.y0, r1.y1, st.m_halation, h); // excitation
    let r3 = Region::expand(r2.y0, r2.y1, st.m_veiling, h); // vignetted
    let y3 = r3.y0; // local row 0 of every strip plane

    // -- Vignette (B) over r3 into plane_a (in place) ---------------------
    for (ly, ay) in (r3.y0..r3.y1).enumerate() {
        for x in 0..w {
            let p = img.pixel(x, ay);
            plane_a.set3(
                x,
                ly as u32,
                [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])],
            );
        }
    }
    optics::apply_vignette_inplace(plane_a, 0, r3.len(), r3.y0, lens, frame);

    // -- Veiling (B) over r2 ------------------------------------------------
    if st.veiling_on {
        // Horizontal pass over all r3 rows into plane_t, vertical pass
        // over the r2 window into plane_b, then mix into plane_t and
        // move the result back to plane_a.
        veiling.conv_h(plane_a, plane_t, 0, r3.len());
        veiling.conv_v(plane_t, plane_b, r2.lo(y3), r2.hi(y3));
        optics::apply_veiling_mix(
            plane_a,
            plane_b,
            plane_t,
            r2.lo(y3),
            r2.hi(y3),
            lens.veiling_amount,
        );
        copy_rows(plane_t, plane_a, r2.lo(y3), r2.hi(y3));
    }

    // -- Excitation C1/C3 + A (EV, pre-desat) over r2 (in-place) -----------
    emulsion::apply_excitation_inplace(plane_a, r2.lo(y3), r2.hi(y3), em, st.ev, st.pre_desaturate);

    // -- Halation C2 over r1 (reads the r2 window) → plane_b ---------------
    emulsion::apply_halation(
        plane_a,
        plane_b,
        r1.lo(y3),
        r1.hi(y3),
        em,
        (r2.lo(y3), r2.hi(y3)),
    );

    // -- Response D + chemistry E over r1 of plane_b — the HALATED ----------
    // excitation, per FILM_MODEL §C2→§D (X_halo = X + H(X) feeds the H&D
    // curves). Until A2.4 this stage ran on plane_a (the un-halated X)
    // and the halation output below was computed and then discarded —
    // the full-chain GPU parity gate caught it on its first real run
    // (worst |Δ| 3.9e-2 on classic-monochrome-400): halation was
    // visually dead in every CPU render since M2.
    emulsion::apply_response_inplace(plane_b, r1.lo(y3), r1.hi(y3), em);

    // -- Grain G over the strip (reads plane_b's r1 window) → plane_a ------
    grain.apply(plane_b, plane_a, strip_y - y3, strip_y1 - y3, frame, y3, h);

    // -- Print/scan F over the strip → plane_b, then into the output ------
    print.apply(plane_a, plane_b, strip_y - y3, strip_y1 - y3);
    for ay in strip_y..strip_y1 {
        let plane_row = ay - y3; // strip-local plane row of this output row
        let out_row = (ay - out_origin) as usize;
        let out_base = out_row * w as usize * 3;
        for x in 0..w {
            let v = plane_b.px64(x, plane_row);
            let i = out_base + x as usize * 3;
            out[i] = v[0] as f32;
            out[i + 1] = v[1] as f32;
            out[i + 2] = v[2] as f32;
        }
    }
}

/// Clamped absolute row region.
struct Region {
    y0: u32,
    y1: u32,
}

impl Region {
    fn expand(y0: u32, y1: u32, margin: u32, h: u32) -> Self {
        Self {
            y0: y0.saturating_sub(margin),
            y1: (y1 + margin).min(h),
        }
    }
    fn len(&self) -> u32 {
        self.y1 - self.y0
    }
    /// Local (plane-relative) start row, given the plane's absolute origin.
    fn lo(&self, origin: u32) -> u32 {
        self.y0 - origin
    }
    fn hi(&self, origin: u32) -> u32 {
        self.y1 - origin
    }
}

fn copy_rows(src: &Plane, dst: &mut Plane, y0: u32, y1: u32) {
    for y in y0..y1 {
        for x in 0..src.w {
            dst.set3(x, y, src.px64(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::testing::test_profile;
    use melography_core::ImageF32;

    /// A deterministic synthetic photo: gradients + a bright disc (gives
    /// every stage something to do — halation blooms around the disc,
    /// grain sees a density range, curves get a exposure sweep).
    fn synthetic_photo(w: u32, h: u32) -> ImageF32 {
        let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
        for y in 0..h {
            for x in 0..w {
                let fx = f64::from(x) / f64::from(w - 1);
                let fy = f64::from(y) / f64::from(h - 1);
                let dx = fx - 0.62;
                let dy = fy - 0.35;
                let disc = (-((dx * dx + dy * dy) / 0.004).exp() * 3.0).exp();
                let base = 0.02 + 0.5 * fx * fy + 0.1 * fx;
                let v = (base + 2.0 * disc).min(4.0);
                let p = img.pixel_mut(x, y);
                p[0] = (v * 1.05) as f32;
                p[1] = (v * 0.95) as f32;
                p[2] = (v * 0.85) as f32;
            }
        }
        img
    }

    fn params() -> FilmChainParams {
        FilmChainParams::new(test_profile()).unwrap()
    }

    #[allow(dead_code)]
    pub(crate) fn synthetic_photo_for_debug(w: u32, h: u32) -> ImageF32 {
        synthetic_photo(w, h)
    }

    #[test]
    fn chain_renders_finite_sane_output() {
        let img = synthetic_photo(96, 64);
        let out = render(&img, &params()).unwrap();
        let mut max = 0.0f32;
        for v in out.data() {
            assert!(v.is_finite());
            max = max.max(*v);
        }
        // Real tonal action: not all black, not all blown.
        assert!(max > 0.01, "output must have brightness, max={max}");
        let non_zero = out.data().iter().filter(|v| **v > 0.001).count();
        assert!(
            non_zero > out.data().len() / 10,
            "output must not be near-black ({non_zero}/{})",
            out.data().len()
        );
    }

    #[test]
    fn chain_is_deterministic() {
        let img = synthetic_photo(80, 60);
        let p = params();
        let a = render(&img, &p).unwrap();
        let b = render(&img, &p).unwrap();
        assert_eq!(a.data(), b.data(), "same params must be bit-exact");
    }

    #[test]
    fn tiled_equals_monolithic_bytes() {
        // The §L gate: tiled render == monolithic render, byte for byte,
        // for several strip heights — including 1 (pathological tiling).
        let img = synthetic_photo(96, 64);
        let p = params();
        let mono = render(&img, &p).unwrap();
        for strip in [1u32, 3, 7, 16, 64] {
            let tiled = render_tiled(&img, &p, strip).unwrap();
            assert_eq!(
                mono.data(),
                tiled.data(),
                "strip_h={strip}: tiled render differs from monolithic"
            );
        }
    }

    #[test]
    fn disabled_chain_is_identity() {
        let img = synthetic_photo(16, 16);
        let p = params()
            .with(Adjustments {
                enabled: false,
                ..Default::default()
            })
            .unwrap();
        let out = render(&img, &p).unwrap();
        assert_eq!(out.data(), img.data());
    }

    #[test]
    fn brighter_input_prints_brighter() {
        // End-to-end tone polarity through the whole chain.
        let p = params();
        let dark = ImageF32::from_interleaved(4, 4, Channels::Rgb, vec![0.05; 4 * 4 * 3]);
        let bright = ImageF32::from_interleaved(4, 4, Channels::Rgb, vec![0.6; 4 * 4 * 3]);
        let o_dark = render(&dark, &p).unwrap();
        let o_bright = render(&bright, &p).unwrap();
        let m_dark: f64 = o_dark.data().iter().map(|v| f64::from(*v)).sum::<f64>() / 48.0;
        let m_bright: f64 = o_bright.data().iter().map(|v| f64::from(*v)).sum::<f64>() / 48.0;
        assert!(
            m_bright > m_dark,
            "brighter input must print brighter: {m_bright} vs {m_dark}"
        );
    }

    // ── A2.1: the threaded executor ─────────────────────────────────────

    /// The §L gate extension: chunks are just another strip partition, so
    /// ANY worker count must reproduce the monolithic bytes. 512 rows at
    /// workers=4 means real 128-row chunks on real threads — the split
    /// path executes regardless of the runner's core count.
    #[test]
    fn threaded_equals_monolithic_bytes() {
        let img = synthetic_photo(96, 512);
        let p = params();
        let mono = render(&img, &p).unwrap();
        for workers in [2u32, 3, 4, 8, 16] {
            let out = render_threaded(&img, &p, workers).unwrap();
            assert_eq!(
                mono.data(),
                out.data(),
                "workers={workers}: threaded render differs from monolithic"
            );
        }
        // workers=0 (auto → one per logical core) is a valid partition too.
        let auto = render_threaded(&img, &p, 0).unwrap();
        assert_eq!(mono.data(), auto.data(), "auto workers differ");
    }

    /// More workers than rows: the clamp must hold and the bytes with it.
    #[test]
    fn threaded_tiny_image_clamps_workers() {
        let img = synthetic_photo(24, 16);
        let p = params();
        let mono = render(&img, &p).unwrap();
        for workers in [2u32, 8, 64] {
            let out = render_threaded(&img, &p, workers).unwrap();
            assert_eq!(
                mono.data(),
                out.data(),
                "workers={workers} on a 16-row image differs"
            );
        }
    }

    /// Margin-heavy configs (veiling on, wide σ) engage the worker cap —
    /// fewer chunks, each carrying its own big margin set — and the bytes
    /// must still match the monolithic reference.
    #[test]
    fn threaded_margin_heavy_matches() {
        let img = synthetic_photo(96, 512);
        let p = params()
            .with_adjustments(
                Adjustments::default(),
                LensParams {
                    veiling_amount: 0.5,
                    veiling_sigma_mm: 40.0, // wide bloom → hundreds of margin rows
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(p.lens.veiling_amount > 0.0);
        let mono = render(&img, &p).unwrap();
        for workers in [2u32, 4, 8] {
            let out = render_threaded(&img, &p, workers).unwrap();
            assert_eq!(
                mono.data(),
                out.data(),
                "workers={workers} (margin-heavy) differs from monolithic"
            );
        }
    }

    /// The memory invariant of the worker cap: n chunks need
    /// `h + n·2·m_total` plane rows, and that must stay ≤ 2× the
    /// monolithic footprint `h + 2·m_total`.
    #[test]
    fn worker_cap_bounds_total_scratch() {
        for (h, m) in [
            (1u32, 0u32),
            (1, 7),
            (16, 3),
            (512, 321),
            (1700, 60),
            (2048, 0),
        ] {
            for req in [1u32, 2, 3, 8, 64] {
                let n = req.clamp(1, h.max(1)).min(worker_cap(h, m));
                assert!(n >= 1, "h={h} m={m} req={req}: worker count must be ≥ 1");
                if m > 0 {
                    assert!(
                        h as u64 + 2 * m as u64 * n as u64 <= 2 * (h as u64 + 2 * m as u64),
                        "h={h} m={m} n={n}: total plane rows exceed 2× the monolithic footprint"
                    );
                }
            }
        }
    }

    // ── A2.2: the fast grain path through the executors ────────────────────────

    /// The §L contract extends to the fast field: any strip partition (tiled
    /// or threaded) reproduces the fast monolithic bytes exactly.
    #[test]
    fn fast_tiled_equals_monolithic_bytes() {
        let img = synthetic_photo(96, 64);
        let p = params().with_grain_fast(true);
        let mono = render(&img, &p).unwrap();
        for strip in [1u32, 3, 7, 16, 64] {
            let tiled = render_tiled(&img, &p, strip).unwrap();
            assert_eq!(
                mono.data(),
                tiled.data(),
                "strip_h={strip}: fast tiled render differs from fast monolithic"
            );
        }
    }

    #[test]
    fn fast_threaded_equals_monolithic_bytes() {
        let img = synthetic_photo(96, 512);
        let p = params().with_grain_fast(true);
        let mono = render(&img, &p).unwrap();
        for workers in [2u32, 3, 4, 8] {
            let out = render_threaded(&img, &p, workers).unwrap();
            assert_eq!(
                mono.data(),
                out.data(),
                "workers={workers}: fast threaded render differs from fast monolithic"
            );
        }
        let auto = render_threaded(&img, &p, 0).unwrap();
        assert_eq!(mono.data(), auto.data(), "auto workers differ (fast)");
    }

    /// Fast and full renders are different realizations (the label is honest),
    /// and both are deterministic — the preview/export split never mixes bytes.
    #[test]
    fn fast_differs_from_full_but_both_deterministic() {
        let img = synthetic_photo(96, 64);
        let pf = params();
        let pfast = params().with_grain_fast(true);
        let full_a = render(&img, &pf).unwrap();
        let full_b = render(&img, &pf).unwrap();
        let fast_a = render(&img, &pfast).unwrap();
        let fast_b = render(&img, &pfast).unwrap();
        assert_eq!(full_a.data(), full_b.data(), "full must be deterministic");
        assert_eq!(fast_a.data(), fast_b.data(), "fast must be deterministic");
        let differing = (0..full_a.data().len())
            .filter(|&i| full_a.data()[i] != fast_a.data()[i])
            .count();
        assert!(
            differing > full_a.data().len() / 4,
            "fast and full must differ ({differing}/{})",
            full_a.data().len()
        );
    }
}
