# MELOGRAPHY — Agent State

**Updated:** 2026-10-24 (M4 session 09, #15 delivered pending review) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M4 (UX polish & hardening)** — in progress on branch
`agent/m4-ux-hardening` (issues #23 ✅ · #24 core ✅ · #15 ✅ pending
review · #25 remaining). Session 06 (PR #26, merged): durable primitive +
kill-during-save battery + undo/redo + autosave. Session 07 (PR #27,
merged): settings/recents/user-preset stores, film-resume fix, CCT+tint
panel, S1 Home, S5 Presets, S7 Settings, panel rail + filmstrip + full
keyboard map. Session 08 (PR #28, merged): ONE standalone export path
(fixed two preview≠export lies), S6 batch queue, WB eyedropper, P1–P5
persona walkthroughs; #23 closed by owner. **Session 09 (#15, this
branch): i18n/a11y complete — zero embedded strings (FilmLab +
StockBrowser + Settings swept, `tEnum` enum localization, 17 missing
error-code entries added → 28/28), locale-aware numbers (`fmtNum`,
VI comma decimals), pseudo-locale CI (`--mode pseudo`, proportional
padding) + `scripts/ui_i18n_audit.mjs`, computed WCAG AA contrast
(`scripts/ui_contrast_audit.mjs`, 54 pairs × 3 themes — **2 real AA
failures found & fixed**), SR announcements (debounced render status,
toast aria-live), Esc on every dialog, Svelte a11y linter clean
repo-wide, 32px targets, reduced-motion. Evidence:
`docs/quality/M4_A11Y_I18N_REPORT.md`. Next: #25 hardening (memory
budget, fuzz battery, secret scan, kill-during-EXPORT), then M5
(owner-gated).**

## Branch / PR / HEAD

- Working branch: `agent/m4-ux-hardening` (recreated from main @ a866980)
- M4 part 1: merged (PR #26); issue #24 core delivered (kill-export
  battery remains, tracked under #25) — close comment posted, owner to
  close
- M4 part 2 (session 07): merged (PR #27)
- M4 part 2 cont. (session 08): merged (PR #28); #23 closed by owner
- M4 part 3 (session 09, #15): delivered on this branch — PR to open
  (owner review required)
- Issue permissions: PAT has them; branch protection on `main` =
  owner-side (unchanged).

## Session 09 delivered (details in session log 09 + quality report)

1. ✅ i18n: FilmLab → shared Slider + t() (last M2-era surface);
   StockBrowser t() + `tEnum` (enum→localized, raw fallback); Settings
   log levels; **28/28 Rust error codes covered** (17 added)
2. ✅ `fmtNum` locale numbers: Viewport render-ms/zoom, TonePanel,
   ModelManager sizes, BatchQueue MB — VI comma decimals
3. ✅ Pseudo-locale: `vite build --mode pseudo` accents + pads ≥40%
   proportionally; wrap loop tree-shake-verified in/out of the bundles
4. ✅ `scripts/ui_i18n_audit.mjs`: 7 gates (parity 323/323, placeholders,
   diacritics 95%, length ≤3.5×, error-code coverage, embedded-string
   scan 22 files 0 hits, pseudo sanity) — wired into CI app-build
5. ✅ `scripts/ui_contrast_audit.mjs`: WCAG AA computed for 54 pairs ×
   3 themes; **fixed light primary-button 3.65→4.84 and light
   fg-tertiary-on-elevated 4.28→4.96**; dark fg-tertiary headroom
6. ✅ A11y: Home recents are real buttons (li role=button removed); Esc
   closes ALL dialogs (Export/ImportLUT were missing); backdrop-click
   dismiss removed (draft-loss > one click); StockBrowser aria-modal +
   focus; toast + debounced render-status aria-live; viewport tabindex
   dropped (role=img non-interactive; keys are global); ✕ buttons say
   "Close"/"Đóng"; dead CSS + off-palette primary override removed
7. ✅ CSS: 32px touch-target floor (one documented exemption: Slider
   .val readout), `prefers-reduced-motion` rule, `overflow-wrap`
   (pseudo/VI safety)

## Local verification (this machine)

- UI: `tsc --noEmit` ✓ · `vite build` ✓ **zero Svelte a11y/unused
  warnings (was 7)** · `build:pseudo` ✓ (wrap present; normal build
  tree-shakes it)
- Audits: `ui_i18n_audit.mjs` PASS · `ui_contrast_audit.mjs` PASS
- No Rust changes this session → cargo matrix owned by CI (app-build
  runs fmt/clippy/tests + the two new audit steps + pseudo build)
- Sandbox: fresh clone (env reset between sessions); node_modules
  restored via npm ci; disk 9.3G free

## Environment (reproducibility)

Sandbox resets between sessions — restore pattern: PAT →
`/home/z/my-project/.secrets/gh_token` (chmod 600); clone; `npm ci` in
`apps/melography-app/ui`. Toolchain notes from sessions 06–08 (lld
RUSTFLAGS etc.) apply only to Rust GUI builds — none needed this session.

## Next five atomic actions

1. Push the branch, open the session-09 PR (#15 — owner review
   required); after merge, close #15 with the report link
2. #25 hardening: memory budget measurement (≤2 GiB @ 50MP), fuzz
   battery (decoders/LUT/sidecar), secret scan, kill-during-EXPORT
   battery (the durable primitive exists — export writer leg)
3. Keep PLAN.md M4 progress + STATE.md re-pinned after each slice
4. M5 packaging prep only after #25 lands (owner-gated regardless)
5. Owner-device follow-ups queued for beta: live SR pass (NVDA/Orca/
   VoiceOver), EN/VI screenshots, GPU budget legs (OPEN_Q #8)
