# HEIF/AVIF go-no-go probe (PLAN M0 item 8, R-IN-2) — **GO** for decode

**Date:** 2026-10-24 · **Harness:** `spikes/heif-probe` (standalone,
zero third-party crates; FFI written against the installed libheif C
headers; `build.rs` links via pkg-config and fails loudly with the
required package list when the backend is missing — that failure is
the go/no-go answer for that machine).

## Verdicts

| Capability | Verdict | Reason |
|---|---|---|
| HEIC decode (camera imports) | **GO** | system libheif (LGPL, **dynamic link only**) + libde265 decode-only plugin |
| AVIF decode | **GO** | dav1d decode-only plugin (BSD) |
| HEIC/AVIF **encode** | **NO-GO by policy** | product exports stay JPEG/PNG/TIFF/WebP (R-OUT-1); HEVC encoding would drag in x265 (GPL) + HEVC patent exposure for zero product need |

Licensing posture (ADR-0003, GPL-3.0-or-later project): LGPL libheif
consumed as a **dynamically linked system library** (LGPL permits this
for any host license; we never statically link, never ship our own
libheif, and never enable an encoder). The M1 `io` crate gains a thin
optional `libheif` feature; until then the probe holds the evidence.

## Measured (this sandbox: libheif 1.23.4, 64×48 fixtures)

| Format | bytes in | decode | output |
|---|---|---|---|
| HEIC (real HEVC payload) | 677 | **3.32 ms** | 64×48 interleaved RGB24, stride 192 |
| AVIF (real AV1 payload) | 484 | **1.02 ms** | same |

Tests (4/4): dimensions + plausible pixel content vs the deterministic
gradient recipe (lossy tolerance ±48), garbage input rejected, version
+ linkage asserted.

## Fixtures & provenance (a bug was caught here)

`tools/gen_fixtures.py` generates both fixtures and **refuses to write
them if the two payloads are identical or wrongly branded** — the
first generation attempt produced a mislabeled .avif (both files HEVC,
same sha256); the guard now hard-fails on that. Current fixtures
(sha256 in `fixtures/provenance.json`):

- `gradient.heic` — real HEVC via the dev-only pillow-heif wheel
  (x265; used once on the dev machine, never distributed; images are
  data, not code)
- `gradient.avif` — real AV1 via ffmpeg `libaom-av1`

## Reproduce

```
cd spikes/heif-probe && cargo test    # 4/4 on any machine with libheif-dev + plugins
cargo run                              # JSON report
```
CI: dedicated ubuntu job (apt `libheif-dev libheif-plugin-libde265
libheif-plugin-dav1d`). macOS/Windows runtime behavior is an owner-
device gap (documented, OPEN_QUESTIONS #8) — the FFI surface is
platform-neutral C.
