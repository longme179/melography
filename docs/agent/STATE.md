# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (session 17 — A1 (PR #35), the session-15 hotfix (PR #36) and A2.1 strip threading (PR #37) are ALL MERGED; this session implements A2.2 grain fast path on `agent/m5a2-grain-fastpath`) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview transport
correctness, PR #35), the session-15 viewport hotfix (PR #36) and A2.1
(strip threading, PR #37) are MERGED.** **A2.2 (grain fast path) is
implemented on `agent/m5a2-grain-fastpath`** — `FastGrain` in
`crates/film/src/grain.rs`: three octaves of correlated band-limited value
noise on absolute film coordinates (Δ = 8/16/32 µm), weights
`(1,t,t)` with `t = clamp(−0.8931·psd_slope − 0.6782, 0, 0.45)`,
Cholesky layer mixing, the full model's own self-calibration and
k-supersampling; §L gates apply unchanged (new fast gates: RMS, PSD
incl. all bundled stocks, ρ, zoom, byte-exact tiling/threading,
determinism); preview leg + stock thumbs opt in via
`FilmChainParams::with_grain_fast(true)`, export/CLI stay full-model by
construction; UI-labelled "grain (fast preview)" (EN/VI). Design and
gate expectations were established in a bit-exact Python prototype
outside the repo (`~/my-project/scripts/a22_*.py`). A2.3–A5 remain.

## Session 17 delivered (this PR — A2.2 grain fast path)

1. ✅ `FastGrain` (grain.rs): tent-interpolated value noise, 3 octaves
   incommensurate with the 6 µm gate grid (commensurate ratios lock the
   sample phase and punch spectral holes — measured, that is how the
   octave set was chosen); ±√3 uniform node marks (unit variance, no
   Box–Muller cost), Cholesky-mixed per layer ⇒ ρ by construction
2. ✅ Amplitude self-calibration through the exact kernel (same
   320²@6 µm/8×8 statistic as the full model) — one-step exact by
   linearity; per-pixel σ(D) at pixel centres (documented approximation;
   identical on uniform patches = every §L gate)
3. ✅ `GrainStage` enum dispatch in `chain.rs`; `FilmChainParams.grain_fast`
   + `with_grain_fast()`; fast margin_px = 0 (no neighbour rows read)
4. ✅ Preview wiring: `EditorState::film_leg()` + `stock_thumbs` set the
   flag (render cache key unchanged — the flag is a renderer property,
   constant for every preview render); export path
   (`export.rs` → `cmd_film_apply` → CLI) never sets it
5. ✅ Gates: fast RMS (0.3/1.0/1.6), fast PSD (test profile −0.832 vs
   −0.869, 4.2 %), all-bundled-stocks PSD (worst 9 %, tol 15 %), ρ,
   zoom, chain determinism hash, fast tiled≡mono + threaded≡mono byte
   gates, fast≠full honest-label test; perf legs
   `resolve-fast` + `preview-fast-threaded-2048x1536` (release-ignored,
   the §4 re-pin evidence for the owner's machine)
6. ✅ UI label "grain (fast preview)" in FilmLab (EN/VI); PLAN A2.2
   progress; FILM_MODEL §G/§L updated; session log 17

## Session 14 delivered (PR #35 — A1, merged)

1. ✅ `resolve_max_dim` at the command boundary (`0` →
   `Settings.preview_max_dim`, `256..=2560`, else typed
   `render.max_dim_out_of_range`) — applied to BOTH `render_preview` and
   `original_frame` (compare parity)
2. ✅ `set_settings` clamps on save (`settings.clamped()` → `pub(crate)`) —
   closes the in-session out-of-range fallback hole
3. ✅ `preview_key(base, stack, film, tone, max_dim)` — max_dim folded
   into the slot key; the slot caches the **bounded** preview image on
   both legs (no-film: downscale once before caching; hit = blit; ≥35 MP
   sources now fit the budget and cache at all)
4. ✅ One render path: `FilmLab` takes `onChange` like every panel, wired
   to `viewport.refresh()`; stock select re-renders; no panel invokes
   `render_preview` directly
5. ✅ Viewport last-wins coalescing (mid-render requests → one queued
   re-run at the latest bound/params; failed-pass drain, no retry loop)
   + original-frame reuse keyed on `(sourcePath, dim)`
6. ✅ Regression tests for every A1 defect class (validation bounds,
   key separation both legs, no cross-size stale hits, bounded slot,
   bounded film compose); EN/VI catalogs + inventory row 39;
   `error_inventory.py` PASS; PLAN progress line; session log 14

## Local verification (this machine)

- Rust: **not run locally (sandbox has no cargo — BUILDING.md)**; this
  PR is verified by CI fmt/clippy/tests ×3 OS; code written
  rustfmt-canonical and self-reviewed for borrowck
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

1. Owner reviews **PR #38 (A2.2 grain fast path)** — CI is the Rust
   verifier (sandbox has no cargo)
2. Owner runs the perf evidence on real hardware:
   `cargo test --release -p melography-film --test perf -- --ignored
   --nocapture` → compare `preview-threaded-2048x1536` vs
   `preview-fast-threaded-2048x1536` p50/p95 → re-pin PLAN §4
3. A2.3 GPU spatial kernels (WGSL grain gather + halation, the
   `film_pixel` parity-harness pattern) after the §4 re-measure
4. A3 Tier-1a camera-log→Rec709 baker (ARRI LogC3/LogC4, S-Log3, V-Log,
   CLog3, F-Log from published transfer functions; golden vectors;
   vendor LUTs dev-time-only, never bundled)
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
