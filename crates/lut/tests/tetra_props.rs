//! Tetrahedral interpolation property tests (luts.md §2 + §5, issue #7):
//! identity invariant ≤ 1e-6 · neutral ramps (no chroma drift) · boundary
//! cases (all 8 corners, exact lattice points, cell faces) · domain
//! clamping + flags · tetrahedra-agreement on shared faces · monotonic
//! transfer sanity for a monotone LUT · 1D per-channel semantics.
//!
//! Also covers the encoding wrap/unwrap round-trips (encoding.rs) and the
//! intensity mix + tiled≡monolithic stage properties (issue #7 matrix).

use melography_core::{Channels, ImageF32};
use melography_lut::{
    cube::{CubeKind, CubeLut},
    encoding::{roundtrip_error, wrap_to, LutEncoding},
    sample, sample1d, sample3d,
    stage::{InsertionPoint, LutApply},
};
use std::sync::Arc;

fn lut_from_table(size: u32, dmin: [f32; 3], dmax: [f32; 3], table: Vec<f32>) -> CubeLut {
    CubeLut {
        kind: CubeKind::Lut3d,
        size,
        domain_min: dmin,
        domain_max: dmax,
        title: None,
        table,
        warnings: Vec::new(),
    }
}

/// Identity 3D LUT: table[lattice] = lattice coordinate (normalized).
fn identity_lut(n: u32) -> CubeLut {
    let mut table = Vec::with_capacity((n as usize).pow(3) * 3);
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                table.push(i as f32 / (n - 1) as f32);
                table.push(j as f32 / (n - 1) as f32);
                table.push(k as f32 / (n - 1) as f32);
            }
        }
    }
    lut_from_table(n, [0.0; 3], [1.0; 3], table)
}

struct XorShift(u64);
impl XorShift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

// ── identity invariant (luts.md §2: ≤ 1e-6 f32) ────────────────────────────

#[test]
fn identity_lut_reproduces_input_within_1e_6() {
    for n in [2u32, 3, 8, 33] {
        let lut = identity_lut(n);
        let mut rng = XorShift(0xfeed_face_0000_1234 + u64::from(n));
        let mut worst: f32 = 0.0;
        for _ in 0..4000 {
            let v = [rng.unit(), rng.unit(), rng.unit()];
            let (out, clamped) = sample3d(&lut, v);
            assert!(!clamped.any());
            for c in 0..3 {
                worst = worst.max((out[c] - v[c]).abs());
            }
        }
        assert!(
            worst <= 1e-6,
            "identity invariant broken at N={n}: max|Δ| = {worst:.3e}"
        );
    }
}

// ── neutral ramp: gray in → gray out, monotone (no chroma drift) ───────────

#[test]
fn neutral_ramp_stays_neutral_and_monotone() {
    // LUT = gentle S-curve on each channel independently (no crosstalk).
    let n = 33u32;
    let curve = |x: f32| x * x * (3.0 - 2.0 * x); // smoothstep
    let lut = identity_lut(n);
    let mut table = lut.table.clone();
    for idx in 0..(n as usize).pow(3) {
        for ch in 0..3 {
            table[idx * 3 + ch] = curve(lut.table[idx * 3 + ch]);
        }
    }
    let lut = lut_from_table(n, [0.0; 3], [1.0; 3], table);

    let mut prev = -1.0f32;
    for step in 0..=256 {
        let g = step as f32 / 256.0;
        let (out, _) = sample3d(&lut, [g, g, g]);
        // Chroma drift: channels must stay equal (identical inputs to a
        // symmetric-per-channel LUT).
        assert!(
            (out[0] - out[1]).abs() <= 1e-6 && (out[1] - out[2]).abs() <= 1e-6,
            "chroma drift at g={g}: {out:?}"
        );
        assert!(out[0] >= prev - 1e-7, "non-monotone at g={g}");
        prev = out[0];
    }
}

// ── boundary cases: corners, lattice points, faces, tetrahedra agreement ───

#[test]
fn exact_lattice_points_hit_table_entries() {
    let lut = identity_lut(5);
    for k in 0..5 {
        for j in 0..5 {
            for i in 0..5 {
                let v = [i as f32 / 4.0, j as f32 / 4.0, k as f32 / 4.0];
                let (out, clamped) = sample3d(&lut, v);
                assert!(!clamped.any());
                for c in 0..3 {
                    assert!(
                        (out[c] - v[c]).abs() <= 1e-6,
                        "lattice point {v:?} → {out:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn all_eight_corners_and_domain_edges() {
    // Constant-corner LUT: each corner maps to a distinct marker value.
    let mut table = vec![0.0f32; 24];
    let corner = |i: usize, j: usize, k: usize| -> [f32; 3] {
        [
            i as f32 * 0.1 + 0.5,
            j as f32 * 0.2 + 0.1,
            k as f32 * 0.3 + 0.05,
        ]
    };
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                let c = corner(i, j, k);
                let base = (i + j * 2 + k * 4) * 3;
                table[base..base + 3].copy_from_slice(&c);
            }
        }
    }
    let lut = lut_from_table(2, [0.0; 3], [1.0; 3], table);
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                let v = [i as f32, j as f32, k as f32];
                let (out, clamped) = sample3d(&lut, v);
                assert!(!clamped.any());
                let want = corner(i, j, k);
                for c in 0..3 {
                    assert!(
                        (out[c] - want[c]).abs() <= 1e-6,
                        "{v:?} → {out:?} want {want:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn tetrahedra_agree_on_shared_faces() {
    // Random crosstalk LUT; sample pairs straddling the fx==fy ordering
    // plane (where tetrahedra change) and assert continuity.
    let n = 9u32;
    let mut rng = XorShift(0x1234_5678_9abc_def0);
    let mut table = Vec::with_capacity((n as usize).pow(3) * 3);
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let (r, g, b) = (
                    i as f32 / (n - 1) as f32,
                    j as f32 / (n - 1) as f32,
                    k as f32 / (n - 1) as f32,
                );
                table.push(0.3 * r + 0.2 * g + 0.1 * b);
                table.push(0.1 * r + 0.4 * g + 0.2 * b);
                table.push(0.2 * r + 0.1 * g + 0.5 * b);
            }
        }
    }
    let lut = lut_from_table(n, [0.0; 3], [1.0; 3], table);
    let cell = 1.0 / (n - 1) as f32;
    let eps = 1e-5;
    let mut pairs = 0;
    for _ in 0..2000 {
        // Lattice cell origin with margins (avoid outer edge).
        let bi = 1 + (rng.next() % (n as u64 - 2)) as usize;
        let bj = 1 + (rng.next() % (n as u64 - 2)) as usize;
        let bk = 1 + (rng.next() % (n as u64 - 2)) as usize;
        let ox = bi as f32 * cell;
        let oy = bj as f32 * cell;
        let oz = bk as f32 * cell;
        // Fractions with fx ≈ fy (the shared face of the x-first and
        // y-first tetrahedra families), fz anywhere below.
        let t = rng.unit();
        let fz = rng.unit();
        let a = [ox + t + eps, oy + t, oz + fz];
        let b = [ox + t - eps, oy + t, oz + fz];
        let oa = sample3d(&lut, a).0;
        let ob = sample3d(&lut, b).0;
        for c in 0..3 {
            assert!(
                (oa[c] - ob[c]).abs() <= 1e-4,
                "discontinuity across tetra face: {a:?} {b:?} → {oa:?} {ob:?}"
            );
        }
        pairs += 1;
    }
    assert!(pairs == 2000);
}

// ── domain handling ─────────────────────────────────────────────────────────

#[test]
fn out_of_domain_inputs_clamp_with_flags() {
    let lut = identity_lut(4);
    // Below min.
    let (out, cl) = sample3d(&lut, [-0.5, 0.25, 0.75]);
    assert!(cl.r && !cl.g && !cl.b);
    assert_eq!(out, [0.0, 0.25, 0.75]);
    // Above max.
    let (out, cl) = sample3d(&lut, [0.25, 1.5, 0.75]);
    assert!(!cl.r && cl.g && !cl.b);
    assert_eq!(out, [0.25, 1.0, 0.75]);
    // All channels.
    let (_, cl) = sample3d(&lut, [-1.0, 2.0, -3.0]);
    assert!(cl.r && cl.g && cl.b);
    // Just inside — no clamp.
    let (_, cl) = sample3d(&lut, [0.0, 1.0, 0.5]);
    assert!(!cl.any());
}

#[test]
fn non_unit_domain_scales_correctly() {
    // Identity over domain [-0.5, 1.5]: endpoints map to themselves.
    let mut table = Vec::new();
    let (dmin, dmax) = (-0.5f32, 1.5f32);
    for k in 0..3 {
        for j in 0..3 {
            for i in 0..3 {
                table.push(dmin + (dmax - dmin) * i as f32 / 2.0);
                table.push(dmin + (dmax - dmin) * j as f32 / 2.0);
                table.push(dmin + (dmax - dmin) * k as f32 / 2.0);
            }
        }
    }
    let lut = lut_from_table(3, [dmin; 3], [dmax; 3], table);
    let (out, _) = sample3d(&lut, [dmin, dmin, dmin]);
    for o in out {
        assert!((o - dmin).abs() <= 1e-6);
    }
    let (out, _) = sample3d(&lut, [dmax, dmax, dmax]);
    for o in out {
        assert!((o - dmax).abs() <= 1e-6);
    }
    let (out, _) = sample3d(&lut, [0.0, 0.0, 0.0]);
    for c in 0..3 {
        assert!(out[c].abs() <= 1e-6, "midpoint of [-0.5,1.5] is 0: {out:?}");
    }
}

// ── 1D semantics ────────────────────────────────────────────────────────────

#[test]
fn one_d_per_channel_shaper() {
    let lut = CubeLut {
        kind: CubeKind::Lut1d,
        size: 3,
        domain_min: [0.0; 3],
        domain_max: [1.0; 3],
        title: None,
        // Channel curves: identity × 0.5 for R, identity for G, gamma-ish for B.
        table: vec![
            0.0, 0.0, 0.0, //
            0.25, 0.5, 0.25, //
            0.5, 1.0, 1.0,
        ],
        warnings: Vec::new(),
    };
    // 0.5 with N=3 lands exactly on row 1 → returns the row verbatim.
    let (out, cl) = sample1d(&lut, [0.5, 0.5, 0.5]);
    assert!(!cl.any());
    assert_eq!(out, [0.25, 0.5, 0.25]);
    // 0.75 lands midway between rows 1 and 2 (g = 1.5).
    let (out, _) = sample1d(&lut, [0.75, 0.75, 0.75]);
    // R: 0.25 + 0.5·(0.5−0.25) = 0.375; G: 0.5 + 0.5·(1−0.5) = 0.75;
    // B: 0.25 + 0.5·(1−0.25) = 0.625.
    assert!((out[0] - 0.375).abs() <= 1e-6, "{out:?}");
    assert!((out[1] - 0.75).abs() <= 1e-6, "{out:?}");
    assert!((out[2] - 0.625).abs() <= 1e-6, "{out:?}");
    // Out-of-range clamps per channel independently.
    let (_, cl) = sample1d(&lut, [-1.0, 0.5, 2.0]);
    assert!(cl.r && !cl.g && cl.b);
}

// ── encoding contracts (encoding.rs) ────────────────────────────────────────

#[test]
fn encoding_wrap_roundtrips_within_tolerance() {
    // Fully invertible encodings (sign-symmetric transfers / linear toe):
    let invertible = [
        LutEncoding::SrgbDisplay,
        LutEncoding::Rec709DisplayLinear,
        LutEncoding::Rec709Gamma2Point4,
        LutEncoding::DisplayP3Display,
        LutEncoding::Acescct,
    ];
    let mut rng = XorShift(0x00c0_ffee_0000_0001);
    for enc in invertible {
        let mut worst = 0.0f32;
        for _ in 0..500 {
            // In-gamut-ish AP1 values (plus small negatives for robustness).
            let v = [
                rng.unit() * 1.2 - 0.05,
                rng.unit() * 1.2 - 0.05,
                rng.unit() * 1.2 - 0.05,
            ];
            let err = roundtrip_error(v, enc).unwrap();
            worst = worst.max(err);
        }
        assert!(
            worst <= 2e-4,
            "roundtrip too lossy for {enc:?}: {worst:.3e}"
        );
    }
    // ACEScc is invertible for x ≥ 0 by spec (S-2014-005: negatives encode
    // to the bottom code — a documented clamp, not a bug). Check the
    // non-negative domain + the negative-clamp semantics explicitly.
    let mut worst = 0.0f32;
    for _ in 0..500 {
        let v = [rng.unit() * 1.2, rng.unit() * 1.2, rng.unit() * 1.2];
        worst = worst.max(roundtrip_error(v, LutEncoding::Acescc).unwrap());
    }
    assert!(worst <= 2e-4, "ACEScc roundtrip (x ≥ 0): {worst:.3e}");
    let enc_v = wrap_to([-0.5, -0.5, -0.5], LutEncoding::Acescc).unwrap();
    let dec = melography_lut::unwrap_from(enc_v, LutEncoding::Acescc).unwrap();
    assert_eq!(dec, [0.0, 0.0, 0.0], "negatives decode to 0 (spec clamp)");
}

#[test]
fn acescct_known_values() {
    // Published fixed points (S-2016-001 + reference implementations):
    // mid-gray 0.18 → 0.4135884; 0.002 → linear segment; X_BRK → Y_BRK.
    let v = wrap_to([0.18, 0.18, 0.18], LutEncoding::Acescct).unwrap();
    assert!((v[0] - 0.413_588_4).abs() <= 1e-5, "{v:?}");
    let v = wrap_to([0.007_812_5, 0.0, 0.5], LutEncoding::Acescct).unwrap();
    assert!((v[0] - 0.155_251_1).abs() <= 1e-5, "{v:?}");
    assert!((v[1] - 0.072_905_53).abs() <= 1e-5, "{v:?}");
    assert!((v[2] - 0.497_716_9).abs() <= 1e-5, "{v:?}");
    // ACEScc: 2^-15 breakpoint continuity and negative clamp.
    let v = wrap_to([-1.0, 0.0, 1.0], LutEncoding::Acescc).unwrap();
    assert!((v[0] - (-0.358_447_8)).abs() <= 1e-5, "{v:?}");
    assert!((v[1] - (-0.358_447_8)).abs() <= 1e-4, "{v:?}");
    assert!((v[2] - 0.554_794_5).abs() <= 1e-5, "{v:?}");
}

#[test]
fn not_implemented_encodings_refuse_explicitly() {
    let err = wrap_to([0.5; 3], LutEncoding::AgxLog).unwrap_err();
    assert!(err.to_string().contains("not implemented"));
    let err = wrap_to([0.5; 3], LutEncoding::Custom1dShaper).unwrap_err();
    assert!(err.to_string().contains("not implemented"));
    // Vocabulary still parses + serializes (sidecar stability).
    let s = serde_json::to_string(&LutEncoding::AgxLog).unwrap();
    assert_eq!(s, "\"agx-log\"");
    let back: LutEncoding = serde_json::from_str(&s).unwrap();
    assert_eq!(back, LutEncoding::AgxLog);
    assert_eq!(
        LutEncoding::from_str_fixed("rec709-gamma2.4").unwrap(),
        LutEncoding::Rec709Gamma2Point4
    );
    assert!(LutEncoding::from_str_fixed("rec709-log").is_err());
}

// ── stage layer: intensity, insertion order, tiled ≡ monolithic ────────────

fn gradient_image(w: u32, h: u32) -> ImageF32 {
    let mut img = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
    for y in 0..h {
        for x in 0..w {
            let px = img.pixel_mut(x, y);
            px[0] = x as f32 / (w - 1) as f32;
            px[1] = y as f32 / (h - 1) as f32;
            px[2] = ((x + y) as f32) / ((w + h - 2) as f32);
        }
    }
    img
}

fn lut_apply_with_identity_lut(intensity: f32, encodings: (LutEncoding, LutEncoding)) -> LutApply {
    LutApply::new(
        "0000hash",
        InsertionPoint::Finishing,
        intensity,
        encodings.0,
        encodings.1,
    )
}

#[test]
fn intensity_zero_is_identity_and_one_is_full_lut() {
    // LUT = constant red (0.8, 0.1, 0.1) everywhere.
    let mut table = vec![0.0f32; 24];
    for base in (0..8).map(|i| i * 3) {
        table[base] = 0.8;
        table[base + 1] = 0.1;
        table[base + 2] = 0.1;
    }
    let lut = lut_from_table(2, [0.0; 3], [1.0; 3], table);
    let img = gradient_image(24, 16);

    let stage = lut_apply_with_identity_lut(
        0.0,
        (
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
    );
    let (out0, _) = stage.apply(&lut, &img).unwrap();
    // Linear→linear identity wrap: intensity 0 ⇒ unchanged.
    assert_eq!(out0.data(), img.data());

    let stage = lut_apply_with_identity_lut(
        1.0,
        (
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
    );
    let (out1, _) = stage.apply(&lut, &img).unwrap();
    // The constant LUT output (0.8, 0.1, 0.1) is in the *declared* space —
    // the AP1 result is its unwrap (Rec.709 ≠ AP1: Bradford D65↔ACES-white
    // adaptation, so grays/primaries shift; never hardcode).
    let const_ap1 =
        melography_lut::unwrap_from([0.8, 0.1, 0.1], LutEncoding::Rec709DisplayLinear).unwrap();
    let (px_chunks, _) = out1.data().as_chunks::<3>();
    for px in px_chunks {
        for (g, want) in px.iter().zip(const_ap1) {
            assert!((g - want).abs() <= 1e-6, "{px:?} want {const_ap1:?}");
        }
    }

    // 0.5 ⇒ halfway between input and LUT output (linear encodings, AP1 mix).
    let stage = lut_apply_with_identity_lut(
        0.5,
        (
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
    );
    let (outm, _) = stage.apply(&lut, &img).unwrap();
    let (src_chunks, _) = img.data().as_chunks::<3>();
    let (mid_chunks, _) = outm.data().as_chunks::<3>();
    for (src, mid) in src_chunks.iter().zip(mid_chunks) {
        let want = [
            0.5 * (src[0] + const_ap1[0]),
            0.5 * (src[1] + const_ap1[1]),
            0.5 * (src[2] + const_ap1[2]),
        ];
        for c in 0..3 {
            assert!(
                (mid[c] - want[c]).abs() <= 1e-6,
                "{src:?} → {mid:?} want {want:?}"
            );
        }
    }
}

#[test]
fn srgb_encoded_lut_wraps_transfer_and_primaries() {
    // LUT doubles its (encoded) input. Declared srgb-display: the stage
    // must encode AP1→sRGB code values, double them, decode back.
    // Gray 0.5 AP1 → sRGB code ≈ 0.73536 → ×2 → 1.0 (clamped? no: table
    // maps linearly, values may exceed 1) → decode.
    let mut table = Vec::new();
    for k in 0..2 {
        for j in 0..2 {
            for i in 0..2 {
                table.push(2.0 * i as f32);
                table.push(2.0 * j as f32);
                table.push(2.0 * k as f32);
            }
        }
    }
    let lut = lut_from_table(2, [0.0; 3], [1.0; 3], table);
    let img = ImageF32::from_interleaved(1, 1, Channels::Rgb, vec![0.5, 0.5, 0.5]);
    let stage =
        lut_apply_with_identity_lut(1.0, (LutEncoding::SrgbDisplay, LutEncoding::SrgbDisplay));
    let (out, report) = stage.apply(&lut, &img).unwrap();
    let px = &out.data()[..3];
    // AP1 0.5 gray → sRGB code c = 0.73535698; out_code = 2c → linear
    // ≈ srgb_to_linear(1.47071) ≈ ... must be > linear(1.0)=1.0 (values
    // beyond code 1.0 decode with the sign-symmetric extension).
    let c = melography_color::srgb_tf::linear_to_srgb(0.5);
    let expected = melography_color::srgb_tf::srgb_to_linear(2.0 * c);
    assert!(
        (px[0] - expected).abs() <= 1e-5,
        "got {px:?} expected {expected}"
    );
    assert_eq!(report.clamped_pixels, 0);
}

#[test]
fn tiled_equals_monolithic_and_insertion_order_holds() {
    // Point op with halo 0: splitting into tiles must be byte-identical.
    let lut = identity_lut(9);
    let img = gradient_image(64, 48);
    let stage =
        lut_apply_with_identity_lut(0.7, (LutEncoding::SrgbDisplay, LutEncoding::SrgbDisplay));

    let (mono, _) = stage.apply(&lut, &img).unwrap();

    // Tile into 16×16 blocks with 0-margin (halo() is 0 for LUTs).
    let mut tiled = ImageF32::alloc(img.width(), img.height(), Channels::Rgb, 0.0);
    let (tw, th) = (16u32, 16u32);
    for ty in (0..img.height()).step_by(th as usize) {
        for tx in (0..img.width()).step_by(tw as usize) {
            let w = tw.min(img.width() - tx);
            let h = th.min(img.height() - ty);
            let mut tile = ImageF32::alloc(w, h, Channels::Rgb, 0.0);
            for y in 0..h {
                for x in 0..w {
                    let src = img.pixel(tx + x, ty + y);
                    let dst = tile.pixel_mut(x, y);
                    dst.copy_from_slice(src);
                }
            }
            let (tout, _) = stage.apply(&lut, &tile).unwrap();
            for y in 0..h {
                for x in 0..w {
                    let src = tout.pixel(x, y);
                    let dst = tiled.pixel_mut(tx + x, ty + y);
                    dst.copy_from_slice(src);
                }
            }
        }
    }
    assert_eq!(mono.data(), tiled.data(), "tiled ≢ monolithic");
}

#[test]
fn apply_chain_orders_by_insertion_point() {
    use melography_lut::stage::apply_chain;
    // Two LUTs: A doubles (finishing), B adds 0.1 then halves (pre-film).
    // Order must be pre-film first regardless of recipe order.
    let double = {
        let mut t = Vec::new();
        for k in 0..2 {
            for j in 0..2 {
                for i in 0..2 {
                    t.extend_from_slice(&[2.0 * i as f32, 2.0 * j as f32, 2.0 * k as f32]);
                }
            }
        }
        lut_from_table(2, [0.0; 3], [1.0; 3], t)
    };
    let shift_half = {
        let mut t = Vec::new();
        for k in 0..2 {
            for j in 0..2 {
                for i in 0..2 {
                    t.extend_from_slice(&[
                        (i as f32 + 0.2) / 2.0,
                        (j as f32 + 0.2) / 2.0,
                        (k as f32 + 0.2) / 2.0,
                    ]);
                }
            }
        }
        lut_from_table(2, [0.0; 3], [1.0; 3], t)
    };
    let luts: Vec<(String, CubeLut)> = vec![("aaaa".into(), double), ("bbbb".into(), shift_half)];
    let resolve = |s: &LutApply| -> Result<CubeLut, melography_lut::LutError> {
        luts.iter()
            .find(|(h, _)| *h == s.lut_hash)
            .map(|(_, l)| l.clone())
            .ok_or_else(|| melography_lut::LutError::Missing {
                hash: s.lut_hash.clone(),
            })
    };
    let img = ImageF32::from_interleaved(1, 1, Channels::Rgb, vec![0.4, 0.4, 0.4]);
    // Recipe order deliberately reversed; canonical order must apply.
    let stages = vec![
        LutApply::new(
            "aaaa",
            InsertionPoint::Finishing,
            1.0,
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
        LutApply::new(
            "bbbb",
            InsertionPoint::PreFilm,
            1.0,
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
    ];
    let (out, reports) = apply_chain(&stages, &resolve, &img).unwrap();
    // 0.4 --B--> (0.4+0.2)/2 = 0.3 --A--> 0.6
    let px = &out.data()[..3];
    assert!((px[0] - 0.6).abs() <= 1e-6, "{px:?}");
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].insertion, InsertionPoint::PreFilm);
    assert_eq!(reports[1].insertion, InsertionPoint::Finishing);
}

#[test]
fn clamping_report_counts_pixels() {
    // LUT with unit domain; feed AP1 image that encodes outside [0,1]
    // (linear encodings make the wrap transparent).
    let lut = identity_lut(4);
    let img = ImageF32::from_interleaved(2, 1, Channels::Rgb, vec![-0.5, 0.5, 0.5, 1.5, 0.2, 0.2]);
    let stage = lut_apply_with_identity_lut(
        1.0,
        (
            LutEncoding::Rec709DisplayLinear,
            LutEncoding::Rec709DisplayLinear,
        ),
    );
    let (out, report) = stage.apply(&lut, &img).unwrap();
    assert_eq!(report.clamped_pixels, 2);
    // Expected values computed through the public encoding API (the AP1↔709
    // matrix is not identity — D65 vs ACES white adaptation).
    let enc = LutEncoding::Rec709DisplayLinear;
    let w0 = wrap_to([-0.5, 0.5, 0.5], enc).unwrap(); // R wraps below 0 → clamps
    let want0 = melography_lut::unwrap_from([w0[0].max(0.0), w0[1], w0[2]], enc).unwrap();
    let w1 = wrap_to([1.5, 0.2, 0.2], enc).unwrap(); // R wraps above 1 → clamps
    let want1 = melography_lut::unwrap_from([w1[0].min(1.0), w1[1], w1[2]], enc).unwrap();
    for c in 0..3 {
        assert!(
            (out.pixel(0, 0)[c] - want0[c]).abs() <= 1e-6,
            "px0 {:?} want {want0:?}",
            out.pixel(0, 0)
        );
        assert!(
            (out.pixel(1, 0)[c] - want1[c]).abs() <= 1e-6,
            "px1 {:?} want {want1:?}",
            out.pixel(1, 0)
        );
    }
    // Dispatch helper sanity (interpolated value — tolerance, not bitwise).
    let (o2, _) = sample(&lut, [0.2, 0.2, 0.2]);
    for c in 0..3 {
        assert!((o2[c] - 0.2).abs() <= 1e-6, "{o2:?}");
    }
}

// Keep an anchor to Arc so the CLI/app layers' sharing pattern stays tested.
#[test]
fn lut_is_shareable_via_arc() {
    let lut = Arc::new(identity_lut(4));
    let (out, _) = sample3d(&lut, [0.5, 0.5, 0.5]);
    assert!((out[0] - 0.5).abs() <= 1e-6);
}
