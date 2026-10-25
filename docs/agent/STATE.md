# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (session 14 — M5-A A1 preview transport shipped, PR pending owner review) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview transport
correctness, P0) is implemented on `agent/m5a1-preview-transport`** —
the three code-verified defects (full-res `maxDim: 0` renders with
discarded ~100 MB frames; cache key omitting `max_dim` → slot poisoning;
stock select never re-rendering + the busy gate swallowing commits) all
have fixes and regression tests. A2–A5 remain.

## Session 14 delivered (this PR — A1)

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

- UI: `npm run build`, `ui_i18n_audit.mjs`, `ui_contrast_audit.mjs`,
  pseudo-locale build — green locally
- `error_inventory.py` + `validate_docs.py` — PASS locally
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

1. Owner reviews PR (A1) → merge → **A2.1 strip threading** (cheapest
   big win; the tiled≡mono gate already proves order-independence)
2. A2.2 grain fast path (FILM_MODEL §G approximation; §L statistical
   gates apply unchanged); A2.3 GPU spatial kernels after re-measuring
3. A3 Tier-1a camera-log→Rec709 baker (ARRI LogC3/LogC4, S-Log3, V-Log,
   CLog3, F-Log from published transfer functions; golden vectors;
   vendor LUTs dev-time-only, never bundled)
4. Owner-device probes once A1 lands: preview p50/p95 at bounded maxDim
   on the Fedora 44 machine → re-pin §4 with real numbers (known
   trade-off to watch: no-film viewport resize re-renders once)
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
