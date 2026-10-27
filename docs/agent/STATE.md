# MELOGRAPHY — Agent State

**Updated:** 2026-10-27 (session 21 — PR #40's mono parity divergence root-caused and fixed: the WGSL respond replicated the mono density into ch 1/2 while the CPU writes [D, 0, 0]; the grain σ(D) luminance read saw D vs 0.25·D — CPU behavior unchanged, the shader now mirrors the CPU plane; session log 21) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview
transport, PR #35), the viewport hotfix (PR #36), A2.1 strip threading
(PR #37) and A2.2 grain fast path (PR #38) are ALL MERGED.** The
owner's 12-core re-measure (2026-10-26): preview p50 2048×1536 went
3.4–4.4 s (M2) → 0.32–0.47 s (A2.1+A2.2, 7–10×); 2/5 stocks inside
the ≤ 400 ms cold budget, worst +18 % — re-pinned in PLAN §4.
**A2.3 (GPU spatial kernels) is in PR #39** (CI 38/38 green after the
session-19 retrigger cleared a transient ort-sys DNS flake on the macOS
runner; awaiting owner review). **A2.4 (full-chain GPU preview wiring)
is implemented on `agent/m5a24-gpu-wiring`, stacked on PR #39**:
persistent `GpuContext` (all pipelines compiled once, process-wide
slot) + the A4 adapter policy (Auto score = device-type rank ×100 +
backend rank; ByFingerprint pins the Settings pick, honest error when
absent), a vignette kernel (per-column/row mm offset LUTs, cross-check
2.2e-7), veiling as the halation h+v dispatch in a blend combine (the
exact `apply_veiling_mix` formula; halation payloads stay
byte-identical), the film_pixel kernel stage-split (split ≡ fused gated
BIT-EXACT), `gpu_chain::render_gpu` sequencing vignette → veiling →
excitation → halation → response → FULL-model grain → print over the
persistent context, the app preview leg trying GPU first with the
threaded fast-grain CPU fallback per frame (never blocks, never errors
to the user), the `GpuRuntime` fallback contract (no adapter / pinned
absent / 3-strike session breaker, all with honest reasons), the
Settings Graphics picker + status rows (`gpu_status`), full-chain
parity ≤ 2e-3 vs the CPU monolithic render (all stocks, stage
isolation, k regimes, determinism, identity), and the owner-run
`gpu_chain_perf` evidence leg. A3–A5 (minus A4's policy core, now
shipped) remain. **Session 21 (2026-10-27) closed the CI saga:** the
gate's round 4 already caught+fixed a dead-halation M2 bug (session 20
log); rounds 8–15 chased a second, mono-only 3.924e-2 divergence —
root cause: the WGSL respond REPLICATED the mono density into channels
1/2 while the CPU writes `[D, 0, 0]` (print reads layer 0 only, so
every post-print gate stayed green, but the grain σ(D) luminance read
saw D vs 0.25·D — a constant −35% amplitude bias on every mono grain
cell; classic-monochrome-400's C3 anchor saturates its H&D shoulder,
making D = 1.84 flat and the bias config-invariant). Fix: the shader
mirrors the CPU plane (no replication); CPU behavior deliberately
unchanged; the mono model follow-ups (C3 anchor over inactive layers,
σ-curve knots unreachable via 0.25·D) documented in FILM_MODEL §G for
an owner decision. The Leg A/B/C diagnostics stay as standing tests —
legC now gates the pre-print respond plane (≤ 1e-3), the one place
print-clamped divergence can hide.

## Session 20 delivered (this PR — A2.4 full-chain GPU preview wiring)

1. ✅ `crates/gpu/src/context.rs` — persistent GpuContext (device+queue+
   all 5 pipelines compiled once), AdapterPolicy (Auto score / ByFingerprint),
   ranked_adapters, global init/reset/reuse, uncaptured-error parking,
   shared compute_pipeline/read_back + AcquiredDevice
2. ✅ `crates/gpu/src/vignette.rs` — WGSL vignette kernel (offset-LUT
   positions) + run_vignette_on + naga validation
3. ✅ `crates/gpu/src/spatial.rs` — halation V-shader blend mode (veiling);
   dispatch factoring for the persistent-context `_on` variants
4. ✅ `crates/gpu/src/film_pixel.rs` — stage split (excite/respond/printStage,
   flags bits 4–6; fused = 7, math unchanged) + `_on` variant
5. ✅ `crates/film/src/gpu_chain.rs` — the full-chain composition (feature
   `gpu-chain`), grain_fast forced false (GPU = full-quality §G), split≡fused
   bit-exact unit test
6. ✅ Parity gates ≤ 2e-3 vs the CPU monolithic render (all stocks,
   stage isolation, k regimes, determinism, identity, honest skip) +
   `gpu_chain_perf` owner-run leg
7. ✅ App: GpuRuntime fallback contract, render_pipeline GPU-first film leg,
   `gpu_status` command + Settings Graphics picker/status (EN/VI),
   `gpu_adapter` setting, ipc-contract sample
8. ✅ Docs: PLAN A2.4 progress + §4 re-pin note, FILM_MODEL §B GPU mirrors,
   session log 20, STATE re-pin

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
- Python cross-check (`a24_gpu_wiring_check.py`: vignette 2.2e-7,
  veiling blend 1.1e-7, halation blend-flag value-identity) + WGSL lint
  over all five shaders (`a24_wgsl_lint.py`) + tsc/vite/i18n/contrast/
  blit gates: ALL PASS
- **CI (this PR): 15 rounds through session 20/21** — round 4 caught
  the dead-halation M2 bug (fixed); rounds 8–15 the mono divergence
  (root cause + fix in session 21: WGSL mono respond replication vs
  the CPU [D,0,0] plane; grain σ(D) saw D vs 0.25·D — see session log
  21). Expected on the fix head: mono battery ~1e-5 (kernel measured
  8.3e-7–1.2e-6 by legB; the saturated spline region is flat-exact on
  both sides). The stacked A2.3 head (PR #39, commit 1d78e3f) is
  38/38 green after the session-19 retrigger; Metal spatial parity
  there: grain worst 8.583e-6, halation 2.384e-7.
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

1. CI green on the PR #40 fix head → owner review of **PR #39 (A2.3)**
   then **PR #40 (A2.4, stacked)** — no merge without approval
2. Owner runs the GPU perf evidence on real hardware:
   `cargo test --release -p melography-film --features gpu-chain
   --test gpu_chain_perf -- --ignored --nocapture` → PLAN §4 GPU-side
   re-pin (plus the A2.3 `spatial_perf` leg)
3. Owner decision (documented in FILM_MODEL §G, not blocking): the mono
   C3-anchor convention (inactive layers' logs saturate the H&D) and
   the σ(D) luminance read (0.25·D vs d₀ for mono)
4. A3 Tier-1a camera-log→Rec709 baker (ARRI LogC3/LogC4, S-Log3, V-Log,
   CLog3, F-Log from published transfer functions; golden vectors;
   vendor LUTs dev-time-only, never bundled)
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
