# MELOGRAPHY — Codec Capability Matrix

**Version:** M1 (2026-10-24) · **Authority:** R-IN-1/R-IN-2/R-OUT-1 (PLAN §3), COLOR_PIPELINE §2/§6 · **Runtime introspection:** `melography codecs` (`crates/io/src/capabilities.rs`)

This file is the *factual* support statement — the app UI mirrors it exactly
(impossible combinations are disabled, never silently converted). The
M0→M1 history: decode matrix measured at M0; the M1 export column reflects
the implemented exporters in `crates/io/src/encode.rs`.

## Decode (stage S0)

| Format | Decode | Bit depth | EXIF orientation¹ | Embedded ICC | Alpha | Notes |
|---|---|---|---|---|---|---|
| JPEG | ✅ | 8 | ✅ applied at S0 | ✅ passthrough | — | via `image` |
| PNG | ✅ | 8/16 | ✅ | ✅ | ✅ | animated: first frame + UI notice |
| TIFF | ✅ | 8/16 | ✅ | ✅ | ✅ | uncompressed + deflate |
| WebP | ✅ | 8 | ✅ | ✅ | ✅ | lossy + lossless |
| HEIC | ✅ *experimental* | 8/10/12 | ✅ | ✅ | — | system libheif (LGPL, **dynamic link**) + libde265 decode-only; probe: 3.3 ms tiny-fixture |
| AVIF | ✅ *experimental* | 8/10/12 | ✅ | ✅ | ✅ | dav1d decode-only; probe: 1.0 ms tiny-fixture |
| RAW | ❌ by scope | — | — | — | — | V1 non-goal (master §02, R-IN-3) |

¹ Orientation is *applied* at S0 (baked, guarded by `orientation_applied`) and never re-applied later.

**HEIC/AVIF platform note (honest gap):** decode depends on the *system*
libheif + codec plugins being present. Linux CI verifies it; Windows/macOS
availability is verified at M5 packaging (bundle-or-detect decision there)
— until then the UI marks both formats "experimental" (UX_SPEC §3).

## Export (stage S5)

| Format | Export | Bit depth | ICC embed | Alpha | Options |
|---|---|---|---|---|---|
| PNG | ✅ | 8 / 16 | ✅ `iCC` chunk | ✅ | adaptive filter |
| JPEG | ✅ | 8 **only** | ✅ APP2 `ICC_PROFILE` (chunked, M1) | — | quality 1–100 |
| TIFF | ✅ | 16 **only**² | ❌ **gap → M4** | ❌ | uncompressed |
| WebP | ❌ v1 | — | — | — | R-OUT-1 keeps exports at JPEG/PNG/TIFF |
| HEIC/AVIF | ❌ by policy | — | — | — | encode NO-GO (probe: x265 GPL + HEVC patents, zero product need) |

² Per-format factual: the TIFF encoder (via `image`/`tiff` crate) writes
16-bit strips; an ICC tag (34675) writer is a documented M4 hardening task —
until then TIFF exports carry no profile and the export dialog says so.

**Determinism note (M1):** embedded ICC profiles are serialized with the
header creation dateTime **pinned** (`icc_bytes_deterministic`,
`crates/color/src/lcms.rs`) — lcms2 stamps "now" into fresh profiles, which
would make identical exports differ between runs (caught by the CLI E2E
test; batch-export determinism is a luts.md §5 gate).

## Verification

* Decode facts: `cargo test -p melography-io` + `melography codecs` (JSON).
* Export facts: `cargo test -p melography-cli --test apply_e2e`
  (all three formats round-trip, ICC APP2 re-parsed + compared byte-wise,
  impossible combos refused with explicit errors).
* HEIF/AVIF probe: `spikes/heif-probe` (4/4 tests) + `HEIF_PROBE.md`.
