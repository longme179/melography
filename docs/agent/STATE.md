# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (session 16 — A1 merged (PR #35); A2.1 strip threading shipped, PR pending owner review; the session-15 `p.width=v.width` hotfix is a parallel PR) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview transport
correctness, P0) is MERGED (PR #35).** **A2.1 (strip threading) is
implemented on `agent/m5a2-strip-threading`** — `chain::render_threaded`
chunks the image across `std::thread::scope` workers, byte-identical for
any worker count (chunks are strips; §L gate extended), preview leg
switched, export sequential, four new regression gates. **In parallel,
session 15 (branch `agent/session15-viewport-unbound-canvas`) fixes the
owner-reported `p.width=v.width` banner** — single blit path
`ui/src/lib/blit.ts` + CI smoke gate; the two PRs touch disjoint files.
A2.2–A5 remain.

## Session 16 delivered (this PR — A2.1 strip threading)

1. ✅ `chain::render_threaded(img, params, workers)` — contiguous
   chunks, one `std::thread::scope` worker each; `workers: 0` = one per
   logical core; no new dependencies; panics resume_unwind into the
   caller
2. ✅ Chunks are strips: the §L contract extends to any worker count —
   gates: threaded≡mono (workers 2/3/4/8/16 + auto, real 128-row
   chunks), tiny-image clamping, margin-heavy (veiling σ=40 mm, cap
   engaged), `worker_cap` scratch invariant `h + n·2·m ≤ 2(h + 2·m)`
3. ✅ The strip body extracted UNCHANGED into `render_strip` shared by
   render / render_tiled / render_threaded; `ChainStages::resolve`
   holds the once-per-render stages (plain data → Sync)
4. ✅ Preview leg (`render_pipeline`) → `render_threaded(_, 0)`;
   export keeps the sequential CLI path (R-REN-2, memory-budget suites
   untouched)
5. ✅ perf harness gains `preview-threaded-2048x1536` (ignored,
   release-run evidence for the §4 re-pin on the owner's machine);
   FILM_MODEL §L gains the threaded≡monolithic row

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

1. Owner reviews **PR #36 (session-15 hotfix)** and **PR #37 (A2.1)** —
   disjoint files, any merge order (STATE/PLAN may need a trivial
   rebase whichever lands second)
2. Owner runs the perf evidence on real hardware:
   `cargo test --release -p melography-film --test perf -- --ignored
   --nocapture` → compare `preview-2048x1536` vs
   `preview-threaded-2048x1536` p50/p95 → re-pin PLAN §4
3. A2.2 grain fast path (FILM_MODEL §G approximation; §L statistical
   gates apply unchanged); A2.3 GPU spatial kernels after re-measuring
3. A3 Tier-1a camera-log→Rec709 baker (ARRI LogC3/LogC4, S-Log3, V-Log,
   CLog3, F-Log from published transfer functions; golden vectors;
   vendor LUTs dev-time-only, never bundled)
4. Owner-device probes once A1 lands: preview p50/p95 at bounded maxDim
   on the Fedora 44 machine → re-pin §4 with real numbers (known
   trade-off to watch: no-film viewport resize re-renders once)
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
