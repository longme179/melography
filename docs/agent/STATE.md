# MELOGRAPHY — Agent State

**Updated:** 2026-10-26 (session 18 — PRs #35/#36/#37/#38 ALL MERGED; the owner's 12-core perf re-measure is in and re-pinned in PLAN §4; this session implements A2.3 GPU spatial kernels on `agent/m5a23-gpu-spatial`) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview
transport, PR #35), the viewport hotfix (PR #36), A2.1 strip threading
(PR #37) and A2.2 grain fast path (PR #38) are ALL MERGED.** The
owner's 12-core re-measure (2026-10-26): preview p50 2048×1536 went
3.4–4.4 s (M2) → 0.32–0.47 s (A2.1+A2.2, 7–10×); 2/5 stocks inside
the ≤ 400 ms cold budget, worst +18 % — re-pinned in PLAN §4.
**A2.3 (GPU spatial kernels) is implemented on
`agent/m5a23-gpu-spatial`**: WGSL grain gather (the FULL §G model —
bit-portable u32 hash stream, presence as the exact u32 threshold
`ceil(p·2^32)`, k×k supersampling) + halation h+v (per-channel taps +
α, CPU skip semantics), `spatial_types.rs` plain-data packs,
`GrainField::gpu_params()` + `gpu_bridge::pack_halation_gpu_params()`
bridges, naga offline gates on every OS, CPU↔GPU parity ≤ 1e-3 on CI
adapters (5 stocks × k∈{1,2,4} + σ(D)=0 region + disabled + halation
variants + dispatch determinism), and an owner-run `spatial_perf`
evidence leg. Design validated BEFORE any Rust in a repo-external
Python cross-check (`~/my-project/scripts/a23_grain_wgsl_check.py`:
presence 200k draws, cell streams to 2.2e-9, k rule). A3–A5 remain.

## Session 18 delivered (this PR — A2.3 GPU spatial kernels)

1. ✅ `crates/gpu/src/spatial_types.rs` — `GrainGpuParams` /
   `HalationGpuParams` + packed layouts (u32 via `f32::from_bits` ↔
   WGSL `bitcast<u32>`) + layout tests; always compiled so
   `melography-film` packs without the graphics stack
2. ✅ `crates/gpu/src/spatial.rs` — 3 WGSL kernels (grain gather full
   model; halation h; halation v) + `run_grain`/`run_halation` in the
   film_pixel harness idiom + naga validation tests (every OS)
3. ✅ Bridges: `GrainField::gpu_params()` (a_norm/Cholesky/threshold/k
   resolved CPU-side in f64) + `pack_halation_gpu_params()` (activity
   = active layer ∧ α>0 ∧ non-identity)
4. ✅ `spatial_parity.rs` gates ≤ 1e-3 (see coverage above) +
   `spatial_perf.rs` (#[ignore]d owner-run: grain/halation at
   2048×1536 + 6000×4000 per stock, device-acquire baseline line)
5. ✅ Docs: PLAN A2.3 progress + §4 re-pin (owner numbers), FILM_MODEL
   §C2/§G GPU-mirror notes, session log 18
6. ✅ Explicitly NOT in this slice (next step): app wiring — persistent
   GPU context, full-chain GPU preview composition, adapter policy
   (A4), fallback contract; CPU stays the reference path everywhere

## Number analysis the owner asked for (2026-10-26 log)

- A2.1 threading: 5.4–7.0× (byte-identical); A2.2 fast grain: further
  1.15–1.76× (best on grain-heavy stocks: mono 1.76×; least
  cool-800-night 1.20×)
- resolve-fast 4.0–5.4 ms vs resolve 0.8–2.9 ms: the fast
  self-calibration costs ~2–4 ms extra but is one-shot per param set
  (cached in the app) — ≪ the 300–500 ms render; no fix warranted
- export 7.2–14.8 s @ 24 MP: within the ≤ 20 s budget; "max
  1.000–1.089" in the log is the perf test's output-sanity max pixel
  value, NOT a fast-vs-reference deviation
- Remaining gap to the ≤ 400 ms cold budget (3/5 stocks, worst 473 ms)
  is what A2.3 kernels + the full-chain GPU preview wiring target

## Local verification (this machine)

- Rust: **not run locally (sandbox has no cargo — BUILDING.md)**; this
  PR is verified by CI fmt/clippy/tests ×3 OS; code written
  rustfmt-canonical (calibrated against repo-passing forms) and
  self-reviewed for borrowck
- Python cross-check + WGSL lint (repo-external, `~/my-project/scripts/`):
  ALL PASS
- `validate_docs.py` — PASS locally (PLAN/FILM_MODEL/session log)
- Sandbox limits (unchanged): node/npm/python3 present; **no cargo** —
  Rust fmt/clippy/tests verified by CI; owner machine is the runtime
  verifier (BUILDING.md)

## Environment (reproducibility)

Sandbox resets between sessions — restore pattern: PAT →
`/home/z/my-project/.secrets/gh_token` (chmod 600); clone; if the
reset leaves mode-noise ("M" on every file, 0/0 diffs) hard-reset onto
origin/main (`git checkout -f main && git reset --hard origin/main &&
git clean -fdq`); `npm ci` in `apps/melography-app/ui` when UI work
resumes.

## Next five atomic actions

1. Owner reviews **PR #39 (A2.3 GPU spatial kernels)** — CI is the Rust
   verifier (sandbox has no cargo)
2. Owner runs the GPU perf evidence on real hardware:
   `cargo test --release -p melography-gpu --test spatial_perf
   -- --ignored --nocapture` → PLAN §4 GPU-side re-pin
3. Full-chain GPU preview wiring (persistent context, pixel + spatial
   kernels + vignette composition, CPU fallback contract) — the last
   A2 leg toward the re-pinned §4 preview budget
4. A3 Tier-1a camera-log→Rec709 baker (ARRI LogC3/LogC4, S-Log3, V-Log,
   CLog3, F-Log from published transfer functions; golden vectors;
   vendor LUTs dev-time-only, never bundled)
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
