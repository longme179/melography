# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (session 13 — M5 restructure planned, PR pending owner review) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M5 — Controlled beta v0.1, restructured into M5-A (depth & speed) →
M5-B (beta engineering)** (session 13, owner-gated). Rationale: the
owner's first-functional-run verdict — *works; incomplete features;
small bugs; slow-but-optimizable preview; Film Room not good yet* —
plus four explicit asks (bundled famous LUTs esp. Rec709; lightcraft as
reference; Nvidia/Radeon/Intel-Arc support; careful planning incl. edge
cases). A beta is a user-facing gate: M5-A fixes the product first.

M5-A slices (full detail in PLAN §2 M5-A):
- **A1 preview transport correctness (P0)** — three code-verified
  defects: FilmLab renders full-res via `maxDim: 0` (downscale
  identity) and discards the ~100 MB frame; the preview cache key
  omits `max_dim` (full-res poisoning of the slot); stock select never
  re-renders; the `busy` gate swallows rapid commits.
- **A2 preview speed** — the M2 §5 plan executed in spec order: strip
  threading → grain fast path (§L gates unchanged) → GPU spatial
  kernels; §4 re-pinned honestly afterwards.
- **A3 bundled LUT library** — Tier 1 self-generated (camera-log→Rec709
  from published formulas; PFE-style from our own print stage) +
  Tier 2 license-gated externals; research in `docs/research/luts.md` §6.
- **A4 GPU adapter UX + diagnostics** — auto/manual picker, backend
  order, device-loss recovery, local-only diagnostics (R-GPU-1).
- **A5 lightcraft borrowings** — `docs/research/comparables.md` §6
  (Apache-2.0/MIT dual, GPL-compatible).

## Branch / PR / HEAD

- Working branch: `agent/m5-planning` (from main @ 5f2d34f = PR #33)
- This PR: PLAN.md M5 restructure + `luts.md` §6 + `comparables.md` §6
  + this STATE re-pin + session log 13 — **owner review required**
- Prior sessions 10b/11/12 (build config, functional, CSP/IPC): all
  merged (PRs #31, #32, #33); the app builds, opens, imports, adjusts
  and previews on the owner's Fedora 44 (owner-confirmed this session)

## Session 13 delivered (this PR — planning only, no code changes)

1. ✅ Environment re-verified after another reset: the "346 modified
   files" in the clone were 100644→100755 mode noise (0 insertions/
   0 deletions) — hard-reset onto main @ 5f2d34f, nothing lost
2. ✅ Root-cause diagnosis of "Film Room not good yet" + "slow preview"
   — three real defects verified in code (PLAN M5-A A1), not guesses
3. ✅ LUT sourcing research (web): Tier-1/Tier-2 candidates + licenses
   (G'MIC CeCILL; Pat David to-verify; YahiaAngelo MIT; slay CC BY);
   Rec709-confusion guard documented
4. ✅ lightcraft researched (Apache-2.0/MIT) — borrowings list +
   differentiation confirmed (it ships no LUT support, no film
   emulation)
5. ✅ PLAN.md: M5 → M5-A/M5-B + graph, R-LUT-2/R-GPU-1 rows, risk #9
   updated + new risk #11 (bundled-LUT licensing)

## Local verification (this machine — docs-only PR)

- All edits are documentation; no Rust/UI code touched → the standard
  CI gates do the real verification (validate_docs, fmt, clippy, tests,
  secret_scan, provenance, error_inventory, app-bundle)
- Sandbox limits (checked this session): node/npm/python3 present;
  **no cargo** — Rust verification via CI, as in sessions 10–12

## Environment (reproducibility)

Sandbox resets between sessions — restore pattern: PAT →
`/home/z/my-project/.secrets/gh_token` (chmod 600); clone; if the
reset leaves mode-noise ("M" on every file, 0/0 diffs) hard-reset onto
origin/main (`git checkout -f main && git reset --hard origin/main &&
git clean -fdq`); `npm ci` in `apps/melography-app/ui` when UI work
resumes.

## Next five atomic actions

1. Owner reviews this planning PR → merge → M5-A starts with **A1**
   (small, P0, unblocks every interaction; branch
   `agent/m5a1-preview-transport`)
2. A1 acceptance: command-boundary validation (256..=2560, 0 =
   `Settings.preview_max_dim`) + `max_dim` in the cache key + a single
   render path via `viewport.refresh()` + last-wins coalescing;
   regression tests in `tests/command_flow.rs` + render tests
3. A2.1 strip threading next (cheapest big win), in parallel with the
   A3 Tier-1a baker skeleton — independent files, one PR per slice
4. Owner-device probes once A1 lands: preview p50/p95 at bounded
   maxDim on the Fedora 44 machine → re-pin §4 with real numbers
5. Pat David license verification (patdavid.org) before any Tier-2
   bundling — never bundle unverified terms (risk #11)
