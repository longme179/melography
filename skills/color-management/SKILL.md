---
name: color-management
description: Use whenever touching pixel math, ICC handling, color space conversions, display paths, or export transforms. Prevents the classic silent-color-corruption bugs (double transforms, encoding mismatches, blended transfer-encoded pixels).
---

# Color Management

## Prerequisites
- Read `docs/COLOR_PIPELINE.md` (normative) and ADR-0002. Know the current stage contracts table by heart before editing stage code.

## Workflow
1. Identify which stage contract row (S0–S5) your change touches; state the input/output space+encoding of every value you compute.
2. Fast paths (hand-written matrices) require a golden test against lcms2 in the same PR — fast paths are optimizations, never replacements.
3. Any new LUT insertion point or AI output that produces color must declare its encoding and convert at documented boundaries.
4. Never blend transfer-encoded pixels where linear compositing is required; never clamp negatives before a stage that needs them (gamut mapping happens at S4/S5).
5. Add/extend golden-oracle cases for the change: patch ΔE00 (mean/max), roundtrip identity, near-black gradient banding (dither on), alpha premultiply roundtrip.
6. GPU parity: run the per-stage tolerance check; if exceeded, fix or publish a revised tolerance WITH a note in the test file (silent tolerance inflation is forbidden).
7. UI-visible color warnings (untagged file, assumed encoding, domain clipping) must fire exactly when the pipeline records them — no UI-only color claims.

## Verification checklist
- [ ] Stage contract table updated if boundaries changed
- [ ] ΔE00 gates green: fast-path ≤0.1/0.5; GPU-vs-CPU ≤0.5/1.0 (mean/max)
- [ ] Roundtrip identity ≤ 1/255·0.5
- [ ] Double-transform detection test still passes (S2 spike artifact, kept as regression)
- [ ] No f16 anywhere new unless the measured-error test proves it safe

## Common pitfalls
- Assuming webview/canvas is color-neutral — it isn't until S2 proves it per platform.
- "It looks right on my monitor" — only oracle numbers count.
- Forgetting EXIF orientation re-application (double rotate) or non-application.
- Treating ΔE00 as film-authenticity proof — it is a pipeline-correctness metric only.

## Links
`docs/COLOR_PIPELINE.md` · ADR-0002 · `docs/research/film-color.md` §3 · skill `wgpu-and-cpu-parity`

## Expected outputs
Implementation + golden tests + (if contracts changed) COLOR_PIPELINE.md update + parity numbers in PR.
