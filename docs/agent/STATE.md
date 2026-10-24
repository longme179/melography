# MELOGRAPHY — Agent State

**Updated:** 2026-10-24 (M4 session 07, in flight) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M4 (UX polish & hardening)** — in progress on branch
`agent/m4-ux-hardening` (issues #23 screens · #24 session model ·
#25 hardening · #15 i18n/a11y). Session 06 (PR #26, merged): durable
primitive + kill-during-save battery + undo/redo + autosave. Session 07
(#23 part 1, this branch): settings/recents/user-preset stores, the
film-resume fix, CCT+tint color panel, S1 Home, S5 Presets, S7 Settings,
panel rail + filmstrip + full keyboard map, headless-check repair.
**Next: session 08 = #23 part 2 (S6 batch queue, WB eyedropper, persona
walkthroughs), then #15 (i18n/a11y sweep + pseudo-locale CI), then #25
(hardening: budgets, fuzz, secret scan, kill-export battery).**

## Branch / PR / HEAD

- Working branch: `agent/m4-ux-hardening` (from main @ 740c9d5)
- M3: merged (PR #22); issues #13/#14 closed completed
- M4 part 1: merged (PR #26); issue #24 core delivered (kill-export
  battery remains, tracked under #25)
- M4 part 2 (session 07): 2 commits on the branch — PR to open
- Issue permissions: PAT has them; branch protection on `main` =
  owner-side (unchanged).

## Session 07 delivered (details in session log 07)

1. ✅ `color::tone`: `gains_from_cct_tint` + `estimate_tint`
   (Planckian-locus synthesis, UX_SPEC §6; round-trip tests; honest
   D-series-vs-Planckian neutral documented)
2. ✅ App settings store (UX_SPEC §7): durable, clamped, corrupt-safe
3. ✅ App recents store (UX_SPEC §3): thumbs, purge-missing, traversal-safe
4. ✅ User presets (UX_SPEC §2 S5): sidecar-shaped snapshots, stable ids
5. ✅ `current_stages` + `apply_pipeline_stages` — one serialization, one
   apply path; **fixes the M2/M3 film-resume gap** (FilmStock stages
   were saved but never re-applied on open); missing stock = amber badge
6. ✅ Commands: settings/recents/presets/stock_thumbs/wb_from_cct_tint/
   lens-in-film/cct-tint estimates
7. ✅ Headless check repaired (capability split + build.rs pattern;
   broken since M1)
8. ✅ UI: Slider, panel rail (Basic/Color/Film/LUT/Lens/Grain·Halation/AI
   with honest not-available rows), S1 Home, S5 Presets, S7 Settings
   (live language/theme), filmstrip, keyboard map §8 complete,
   Z-hold-100% loupe

## Local verification (this machine)

- color 36 ✓ + presets ✓ (touched crates; rest unchanged since 37/37 —
  CI owns the full matrix; sandbox disk cannot hold two full builds)
- App headless: clippy `-D warnings` clean; 22 lib + 3 integration ✓
- App gui (sysroot + RUSTFLAGS -L for lld): check ✓ clippy ✓ tests ✓
- UI: tsc ✓ vite build ✓ · fmt clean everywhere

## Environment (reproducibility)

Sandbox reset before session 07: rustup 1.99.1 reinstalled; sysroot
rebuilt via batch apt download + dpkg-deb -x + symlink retarget
(`/home/z/my-project/scripts/sysroot_rebuild.sh`); **toolchain 1.99
defaults to lld which ignores LIBRARY_PATH → RUSTFLAGS="-L …"**
(`scripts/appenv.sh` has the full env). Disk: shared
CARGO_TARGET_DIR + drop incremental/large binaries between phases.

## Next five atomic actions

1. Push the branch, open the M4-part-2 PR (owner review required)
2. Session 08 (#23 part 2): S6 batch export queue (worker thread +
   events + UI rows), WB eyedropper, persona walkthrough notes
3. #15 sweep: embedded strings out of M2/M3 components, pseudo-locale
   CI, a11y audit checklist
4. #25 hardening: memory budget measurement, fuzz battery, secret scan,
   kill-during-EXPORT battery
5. Keep PLAN.md M4 progress + STATE.md re-pinned after each slice
