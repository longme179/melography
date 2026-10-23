---
name: wgpu-and-cpu-parity
description: Use when writing or reviewing any GPU (WGSL) kernel or the preview transport. Guarantees the CPU oracle stays authoritative and GPU divergence is measured, not assumed.
---

# wgpu & CPU Parity

## Prerequisites
- CPU oracle implementation + golden hashes exist BEFORE the GPU mirror (skill `film-science-and-profiles` / `color-management`).
- `wgpu` pinned in workspace; naga shader validation wired into CI.

## Workflow
1. Port the stage to WGSL mirroring the CPU math exactly (same operation order where practical; document intentional reorderings that alter rounding).
2. Shader hygiene: explicit bindings; no uninitialized reads; guard tiling margins per stage `halo()`; integer/float conversions explicit; no f16 unless the stage has a measured-error exemption (COLOR_PIPELINE §8).
3. Add the stage to the parity harness: run CPU and GPU on golden inputs; report max|Δ| and ΔE00; compare against the published per-stage tolerance.
4. Tiled-vs-monolithic: run the GPU stage tiled with halo margins; must equal the monolithic GPU output within float tolerance; CPU tiled must be bit-exact.
5. Determinism: same input+seed → identical GPU output across repeated runs (adapter-allowlist documented; report adapter/driver in test output — divergence across vendors is a bug to investigate, not noise).
6. Preview transport (S1 design): measure frame pacing (p50/p95), readback cost, and verify no color drift vs CPU display path; log numbers, never "feels smooth".
7. Register new adapters/driver quirks discovered in `docs/agent/sessions/` notes and, if systemic, a GPU-quirks appendix in ARCHITECTURE.md.

## Verification checklist
- [ ] Parity report in PR: max|Δ| + ΔE00 per stage vs tolerance
- [ ] Tiled ≡ monolithic (CPU bit-exact / GPU within tolerance)
- [ ] Repeat-run determinism on CI adapter recorded
- [ ] No giant buffers or unbounded pools (memory budget §PLAN 4)
- [ ] CI GPU tests run ONLY where a real GPU exists — never fake GPU results

## Common pitfalls
- "GPU is the source of truth" — it never is; CPU oracle owns correctness.
- Forgetting halo margins → tile seams visible only on some content (test with grain/halation golden images).
- Reading back full-res frames for preview (blows the IPC/memory budget).
- Silent driver differences (e.g., fast-math on some backends) — keep math conservative.

## Links
`docs/ARCHITECTURE.md` §5–§6 · `docs/COLOR_PIPELINE.md` §8 · ADR-0001 (S1)

## Expected outputs
Kernel + parity numbers + tiled test + (if transport touched) latency measurements.
