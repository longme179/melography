//! Tetrahedral interpolation (CPU oracle implementation, luts.md §2).
//!
//! **Why tetrahedral:** trilinear interpolation visibly degrades saturated
//! gradients (it averages across the cube diagonal); tetrahedral is the
//! industry floor for look LUTs (OCIO, darktable, Resolve all use it).
//!
//! **Geometry (independent derivation, not a transcription of OCIO):** the
//! unit cell of the LUT lattice decomposes into 6 tetrahedra sharing the
//! main-diagonal edge `(0,0,0)–(1,1,1)`. A point `f = (fx,fy,fz)` inside the
//! cell falls in exactly one tetrahedron, selected by the *sorted order* of
//! its fractional coordinates. Within the containing tetrahedron with
//! vertices `v0..v3`, barycentric interpolation is exact:
//!
//! ```text
//! out = c(v0) + fx·(c(vx)−c(v0)) + fy·(c(vy)−c(v0)) + fz·(c(vz)−c(v0))
//! ```
//!
//! where `(vx, vy, vz)` are the corner *deltas* of the path from v0 to v3
//! along the sorted axes. The six cases enumerate the 3! orderings of
//! `(fx, fy, fz)`. Exact-lattice inputs (a fractional coordinate == 0 or two
//! equal fractions) are boundary-stable: adjacent tetrahedra agree on shared
//! faces by construction (both interpolate the same triangle).
//!
//! The independent cross-check is `tools/lut_oracle.py`, which solves the
//! barycentric system per-tetrahedron from scratch and must agree with this
//! implementation to ≤ 1e-6 on the shared fixture set (CI re-generates the
//! fixture; the Rust test only reads it).
//!
//! **Boundary policy (luts.md §2):** inputs are clamped to the declared
//! domain *before* interpolation; the clamp is *reported* (per-pixel flags
//! aggregated into an `ApplyReport` by the stage layer, surfaced in the UI
//! as "LUT domain clipping"). 1D tables interpolate piecewise-linearly in
//! the declared domain with per-channel shaper semantics (row `i` column
//! `ch` maps domain input `ch` → output channel `ch`).

use crate::cube::{CubeKind, CubeLut};

/// Per-sample clamp report (aggregated by callers; cheap branchless-ish).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClampFlags {
    pub r: bool,
    pub g: bool,
    pub b: bool,
}

impl ClampFlags {
    #[must_use]
    pub fn any(self) -> bool {
        self.r || self.g || self.b
    }
}

/// Sample the 3D LUT at `rgb` (declared-domain values). Clamps out-of-range
/// inputs and returns the clamped sample plus which channels clamped.
///
/// # Panics
/// Panics if `lut.kind != Lut3d` — the type-level API keeps kinds separate
/// at the stage layer; this is a caller-contract assertion.
#[must_use]
pub fn sample3d(lut: &CubeLut, rgb: [f32; 3]) -> ([f32; 3], ClampFlags) {
    assert_eq!(lut.kind, CubeKind::Lut3d, "sample3d on a 1D LUT");
    let n = lut.size;
    let last = (n - 1) as f32;

    // Clamp to the declared domain (luts.md §2 boundary policy).
    let mut clamped = ClampFlags::default();
    let mut v = [0.0f32; 3];
    for c in 0..3 {
        let x = rgb[c];
        let lo = lut.domain_min[c];
        let hi = lut.domain_max[c];
        if x < lo {
            v[c] = lo;
            clamped.r |= c == 0;
            clamped.g |= c == 1;
            clamped.b |= c == 2;
        } else if x > hi {
            v[c] = hi;
            clamped.r |= c == 0;
            clamped.g |= c == 1;
            clamped.b |= c == 2;
        } else {
            v[c] = x;
        }
    }

    // Map to lattice coordinates: [dmin,dmax] → [0, N−1].
    let scale = |c: usize| {
        let span = lut.domain_max[c] - lut.domain_min[c];
        (v[c] - lut.domain_min[c]) / span * last
    };
    let gx = scale(0);
    let gy = scale(1);
    let gz = scale(2);

    let i = gx.floor();
    let j = gy.floor();
    let k = gz.floor();
    // Lattice cell origin. Clamp to N−2 (not N−1): an input exactly at the
    // upper domain bound floors to N−1, and the cell (N−1 … N) does not
    // exist — we instead use the last real cell (N−2 … N−1) with fraction
    // 1.0, which is exact (weight 1 on the upper corner) and keeps every
    // corner access in bounds. (N ≥ 2 is enforced by the parser; a direct
    // CubeLut constructor hitting N < 2 panics in grid3's debug_assert.)
    let i0 = (i as usize).min(n as usize - 2);
    let j0 = (j as usize).min(n as usize - 2);
    let k0 = (k as usize).min(n as usize - 2);
    let fx = gx - i0 as f32;
    let fy = gy - j0 as f32;
    let fz = gz - k0 as f32;

    // Corner colors (red-fastest layout: (i + j·N + k·N²)·3).
    let c = |di: usize, dj: usize, dk: usize| -> [f32; 3] { lut.grid3(i0 + di, j0 + dj, k0 + dk) };
    let c000 = c(0, 0, 0);
    let c100 = c(1, 0, 0);
    let c010 = c(0, 1, 0);
    let c110 = c(1, 1, 0);
    let c001 = c(0, 0, 1);
    let c101 = c(1, 0, 1);
    let c011 = c(0, 1, 1);
    let c111 = c(1, 1, 1);

    // Six-tetrahedra enumeration by fractional ordering. Each case is a
    // path 000 → (first axis) → (first+second axes) → 111; the output is
    // the *increment sum* along the path (barycentric weights: 1−u, u−v,
    // v−w, w — sum to 1 by construction; verified by the identity-invariant
    // property tests AND the independent Python oracle).
    let step = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], u: f32, v: f32, w: f32| {
        // out = a + u·(b−a) + v·(c−b) + w·(d−c)
        [
            a[0] + u * (b[0] - a[0]) + v * (c[0] - b[0]) + w * (d[0] - c[0]),
            a[1] + u * (b[1] - a[1]) + v * (c[1] - b[1]) + w * (d[1] - c[1]),
            a[2] + u * (b[2] - a[2]) + v * (c[2] - b[2]) + w * (d[2] - c[2]),
        ]
    };
    let out = if fx >= fy && fy >= fz {
        // path: 000 → 100 → 110 → 111 (x, then y, then z)
        step(c000, c100, c110, c111, fx, fy, fz)
    } else if fx >= fz && fz >= fy {
        // path: 000 → 100 → 101 → 111 (x, then z, then y)
        step(c000, c100, c101, c111, fx, fz, fy)
    } else if fy >= fx && fx >= fz {
        // path: 000 → 010 → 110 → 111 (y, then x, then z)
        step(c000, c010, c110, c111, fy, fx, fz)
    } else if fy >= fz && fz >= fx {
        // path: 000 → 010 → 011 → 111 (y, then z, then x)
        step(c000, c010, c011, c111, fy, fz, fx)
    } else if fz >= fx && fx >= fy {
        // path: 000 → 001 → 101 → 111 (z, then x, then y)
        step(c000, c001, c101, c111, fz, fx, fy)
    } else {
        // fz >= fy >= fx — path: 000 → 001 → 011 → 111 (z, then y, then x)
        step(c000, c001, c011, c111, fz, fy, fx)
    };
    (out, clamped)
}

/// Sample the 1D LUT with per-channel piecewise-linear semantics:
/// input channel `ch` maps through column `ch` of the table, indexed by the
/// channel's own value scaled into `[domain_min[ch], domain_max[ch]]`.
///
/// (Adobe's 1D tables predate per-channel use in look pipelines; OCIO's
/// FileTransform applies exactly these semantics, and shaper chains rely on
/// them — luts.md §2 "1D shaper".)
#[must_use]
pub fn sample1d(lut: &CubeLut, rgb: [f32; 3]) -> ([f32; 3], ClampFlags) {
    assert_eq!(lut.kind, CubeKind::Lut1d, "sample1d on a 3D LUT");
    let n = lut.size;
    let last = (n - 1) as f32;
    let mut out = [0.0f32; 3];
    let mut clamped = ClampFlags::default();
    for ch in 0..3 {
        let lo = lut.domain_min[ch];
        let hi = lut.domain_max[ch];
        let x = rgb[ch];
        let (xc, did) = if x < lo {
            (lo, true)
        } else if x > hi {
            (hi, true)
        } else {
            (x, false)
        };
        match ch {
            0 => clamped.r = did,
            1 => clamped.g = did,
            _ => clamped.b = did,
        }
        let g = (xc - lo) / (hi - lo) * last;
        // Upper-edge guard mirrors sample3d: clamp to the last *cell*, not
        // the last row (fraction 1.0 at the domain top keeps access in
        // bounds and is exact).
        let i0 = (g.floor() as usize).min(n as usize - 2);
        let f = g - i0 as f32;
        let base0 = i0 * 3 + ch;
        let base1 = (i0 + 1) * 3 + ch;
        out[ch] = lut.table[base0] * (1.0 - f) + lut.table[base1] * f;
    }
    (out, clamped)
}

/// Dispatch by kind.
#[must_use]
pub fn sample(lut: &CubeLut, rgb: [f32; 3]) -> ([f32; 3], ClampFlags) {
    match lut.kind {
        CubeKind::Lut3d => sample3d(lut, rgb),
        CubeKind::Lut1d => sample1d(lut, rgb),
    }
}
