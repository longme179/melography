# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (session 15 — A1 merged (PR #35); `p.width=v.width` banner hotfix shipped, PR pending owner review) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5-A — Creative depth & preview speed** (session 13 restructure,
owner-gated; full slice detail in PLAN §2 M5-A). **A1 (preview transport
correctness, P0) is MERGED (PR #35)** — owner confirmed improvements.
**Session 15 hotfix for the `p.width=v.width` banner is implemented on
`agent/session15-viewport-unbound-canvas`** — the minified
`originalCanvas.width = original.width` in Viewport.blit() threw on
every render with compare off (canvas unbound outside its `{#if}`); all
canvas writes now go through the single path `lib/blit.ts` (typed
no-ops for unbound canvas / bad frames), the compare canvas mounts
whenever an original exists (CSS-hidden while compare off), and the new
CI gate `ui_blit_smoke.mjs` locks both the guards and the single path.
A2–A5 remain.

## Session 15 delivered (this PR — A1 hotfix)

1. ✅ Root-caused against the production bundle: `p.width=v.width` =
   Viewport's `originalCanvas.width = original.width`; compare canvas
   only `bind:this`-bound inside `{#if $compareOn && original}` while
   renderPass loads `original` unconditionally → TypeError into the
   banner on every pass with compare off (pre-existing, not an A1
   regression; A1 didn't touch it)
2. ✅ `ui/src/lib/blit.ts` — single RGBA→canvas path; unbound canvas /
   missing frame / degenerate dims / truncated buffer = typed no-ops
   leaving previous pixels untouched; tsc-checked (it lives in .ts)
3. ✅ Compare canvas mounts whenever an original EXISTS, CSS-hidden
   while compare off (element lifecycle = data lifecycle; toggle-on is
   instant, was a blank half until next commit); reactive draw covers
   the post-flush bind
4. ✅ Stale `edited`/`original` dropped on image close (resize-after-
   close could layout an unmounted canvas); failed passes banner only
   while an image is open (close-mid-render no longer pops home screen)
5. ✅ Filmstrip routes through `putRgba`; `PreviewFrame.rgba:
   Uint8ClampedArray<ArrayBuffer>` (TS 5.7+ generics — old code only
   passed because tsc never checked inside .svelte)
6. ✅ `scripts/ui_blit_smoke.mjs` CI gate: real blit.ts via workspace
   esbuild + stub DOM (unbound canvas = the exact crash, 27 asserts) +
   single-blit-path scan (no direct canvas resize outside blit.ts)

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

- UI: `npm run check` (tsc), `npm run build`, `build:pseudo`,
  `ui_i18n_audit.mjs`, `ui_contrast_audit.mjs`, `ui_blit_smoke.mjs`
  (new) — green locally; minified bundle grep: unguarded
  `X.width=Y.width` GONE
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

1. Owner reviews PR (A1 hotfix) → merge → **A2.1 strip threading**
   (cheapest big win; the tiled≡mono gate already proves
   order-independence)
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
