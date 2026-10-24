# Spike S2 — lcms2 cross-platform golden consistency + webview double-transform (issue #4)

**Status:** golden battery **complete and green** (commit `90f247e`,
session 02 verification); the **webview double-transform leg is open**
(issue #4 stays open until that leg lands). Nothing unmeasured is
claimed.
**Date:** 2026-10-24 · **Harness:** `crates/color` (fast paths + CIEDE2000
oracle + vendored-static lcms2 2.19 via lcms2-sys) + `crates/cli` golden
subcommand.

## Part 1 — golden battery (COLOR_PIPELINE §8 rows 1/2/4/5) — DONE

Measured on this sandbox and enforced identically in CI (3-OS matrix):
the exact same tests run on GitHub runners; cross-OS **bit-identity
hashes** are committed (Linux reference; CI validates win/mac —
`crates/color/tests/golden/golden_transforms.json`).

| Row | Gate | Measured |
|---|---|---|
| 1. fast-path vs lcms2 (AP1→display-class transforms) | ΔE00 mean ≤ 0.1, max ≤ 0.5 | **PASS** (CI-enforced) |
| 2. encode→decode roundtrip | ≤ 0.5/255 | **PASS** — worst measured 31/65535 on the ICC path |
| 4. neutral ramp stays neutral | ΔE00 ≤ 0.1 across ramp | **PASS** |
| 5. dither banding | ≤ 1 LSB | **PASS** |

CIEDE2000 was pinned to the Sharma et al. (2005) test vectors; during
cross-verification against colour-science two implementation bugs
(T-term, RT-term) were found in our first draft and fixed before the
battery was trusted — recorded in the session log.

## Part 2 — webview double-transform detection — OPEN (honest)

COLOR_PIPELINE §5 specifies the method: render a known AP1 pattern to
the preview surface, capture the surface back, and require the composed
transform to match `lcms2(AP1→display)` within tolerance. That needs a
**live webview on a display** — not possible headless in this sandbox
(no X/wayland webview stack). Plan:

- **CI (linux):** xvfb + webkit2gtk once the M1 app shell exists (the
  probe rides the app harness, not a bare webview).
- **Windows/macOS:** owner devices (OPEN_QUESTIONS #8 gap stands).
- **Interim guarantee:** the S1 harness documents that the webview leg
  is unmeasured; ADR-0001's fallback trigger remains armed.

Issue #4 remains open until this leg produces evidence; the lcms2
cross-platform half is closed by the battery above.

## Reproduce

```
cargo test -p melography-color            # golden battery + CIEDE2000 vectors
cargo run -p melography-cli -- golden     # battery report
```
