# Spike S3 — ort/ONNX Runtime CPU packaging (issue #5, ADR-0004)

**Status:** complete on this machine; 3-OS enforcement via the CI matrix
(rust-tests job includes the s3 tests; per-OS numbers come from CI
logs and are to be appended below after the first green run — the
assertions are already enforced there).
**Date:** 2026-10-24 · **Harness:** `spikes/s3-ort` (workspace member;
ort 2.0.0-rc.15, download-binaries, CPU EP) + project-authored dummy
model (`scripts/gen_dummy_onnx.py`, 171 bytes, sha256-pinned in three
places).

## Design notes discovered the hard way (all verified against source)

1. **IR version:** onnx Python ≥1.23 writes IR 14 by default; the ORT
   bundled with ort rc.15 supports **max IR 13**. The generator now pins
   `ir_version=13` — the first test run failed with "Unsupported model
   IR version: 14" and the model was regenerated + re-pinned
   (sha256 `b22a9c3d…`).
2. **One environment per process:** the rc.15 wrapper forbids
   re-initializing after teardown. Cold init is therefore measured
   **exactly once** (mirroring the real app: one env per process);
   warm session re-creation (the model-manager path) is measured 5×.
3. `try_extract_tensor` returns `(Shape, &[f32])` in rc.15.

## Measured (this sandbox: linux x86_64, 2 cores, 2 GB-class runner)

| Metric | Value | ADR-0001/0004 gate | Verdict |
|---|---|---|---|
| cold init (env + first session) | **14.07 ms** | ≤ 300 ms target | PASS, 21× margin |
| warm session re-create (5 runs) | 0.36–0.56 ms | — | model switching is free |
| warm invocation median (50 runs) | **0.011 ms** (max 0.167 ms) | ~200 ms analysis budget | PASS, trivially |
| binary footprint | 37.1 MB (static-linked ORT inside the executable, `download-binaries` default) | reported, not gated | dynamic-link comparison is an M3 packaging decision |
| correctness | Y = X + 0.25 verified; sha256 asserted at runtime | mandatory | PASS |

## Consequence for ADR-0004

CPU-EP parameter-predictor inference is **packaging-viable on all
three OSes** pending their CI numbers (the same assertions run there
with generous sanity bounds: cold < 1000 ms, median < 500 ms — the
*measured* values are the evidence). AI remains optional-by-design in
the product (ADR-0004); this spike only clears the packaging risk
(PLAN §5 risk 3) with real numbers.

## Reproduce

```
cargo run -p s3-ort-spike --release    # JSON report
cargo test -p s3-ort-spike             # CI-enforced form
```
