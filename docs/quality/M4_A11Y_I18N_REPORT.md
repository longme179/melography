# M4 i18n & Accessibility Report — UX_SPEC §10 acceptance (issue #15)

**Date:** 2026-10-24 (session 09) · **Spec:** `docs/UX_SPEC.md` §10 (plus §8
announcement and §9 error-localization lines) · **Method:** every claim was
verified by a script that parses the real source (`scripts/ui_i18n_audit.mjs`,
`scripts/ui_contrast_audit.mjs`) or by a build that exercises the real
pipeline (`vite build --mode pseudo`). Both audits run in CI on every push;
their outputs below are the committed evidence.

---

## 1. i18n — zero embedded strings

**Gate:** UX_SPEC §10 "All UI strings in catalogs (`en`, `vi`) — zero
embedded English". Enforced by `scripts/ui_i18n_audit.mjs` (CI step
"UI i18n audit"), which scans all 22 `.svelte` files for visible text nodes
and label attributes that bypass `t()`.

| Check | Result | Evidence |
|---|---|---|
| Catalog parity EN↔VI | **323 = 323 keys**, no dups, no empties | audit gate 1 |
| `{x}` placeholder parity | all match | audit gate 2 |
| VI diacritics coverage | **95.0%** of letter-bearing VI values carry diacritics (gate ≥ 80%) | audit gate 3 |
| VI length growth | worst 3.00× (gate ≤ 3.5×) | audit gate 4 |
| Rust error-code coverage | **28/28** `AppError::new` codes have `error.code.*` entries in both catalogs | audit gate 5 — 17 codes were missing before this session |
| Embedded strings in markup | **0** across 22 files (allowlist: brand, format names, language endonyms, SI units, paths — each justified below) | audit gate 6 |

### 1.1 What was swept this session

* **`FilmLab.svelte`** (the last M2-era unlocalized surface): every label,
  tooltip, and the provenance honesty line now goes through the catalogs;
  all knobs converted to the shared `Slider` (double-click reset, arrow
  nudge, typed values, **locale-aware numbers**, physical tooltips — the
  UX_SPEC §4 "every slider" contract now holds on every panel).
* **`StockBrowser.svelte`**: title/close/empty/aria strings localized;
  backend enum values (provenance, confidence, polarity, layers, path)
  display through the new `tEnum(prefix, value)` helper — known values
  localize, unknown values fall back to the raw token (data-driven enums
  never crash and never leak a raw i18n key).
* **`Settings.svelte`**: log-level option labels localized
  (`settings.loglevel.*`).
* **Error taxonomy**: 17 previously-uncovered `AppError` codes received EN/VI
  guidance entries (list in the audit gate 5 output).
* **Locale-aware numbers** (UX_SPEC §10 "number/date locale-aware"): new
  `fmtNum()` in `lib/i18n.ts` (VI = comma decimal separator, e.g. `1,5`)
  used by Viewport (render ms, zoom %), TonePanel (suggestion values),
  ModelManager (model sizes); BatchQueue's MB formatter now follows the
  live language; `Slider` already did (session 07).

### 1.2 The allowlist (deliberate, each entry justified)

| Entry | Reason |
|---|---|
| `MELOGRAPHY` | brand name |
| `English` / `Tiếng Việt` | language endonyms — the i18n convention is each language in its own name |
| `PNG` `JPEG` `TIFF` `HEIC / AVIF` `WebP` | file-format names, universal in VI photo software |
| `EV` `MB` `kB` `ms` `K` | photographic / SI units (VI usage matches) |
| `~/.melography` `https://…` `MELOGRAPHY_PROFILES` | paths, URLs, env-var names (technical identifiers) |

## 2. Pseudo-locale build (UX_SPEC §10 "pseudo-locale test build in CI")

* `vite build --mode pseudo` (npm script `build:pseudo`) sets
  `import.meta.env.MODE === "pseudo"` in `lib/i18n.ts`; every catalog string
  is then accented (a→á, e→é …) and **proportionally padded ≥ 40%**
  (`[̃ stríng ẊẊẊ…]`) at module init — placeholders `{x}` stay intact.
* Verified in the built bundle: the wrap loop is present and unconditional
  in the pseudo build and fully tree-shaken from the normal build
  (different content hashes, `191.70 kB` vs `191.45 kB`).
* Both builds run in CI (`app-build` job); the audit's gate 7 proves the
  transform lengthens and accents (a fixed `+n` padding would under-stress
  long sentences — the proportional form is the honest one).
* `body { overflow-wrap: break-word }` guarantees the padded strings wrap
  instead of overflowing horizontally.

## 3. WCAG AA contrast — computed, not assumed

`scripts/ui_contrast_audit.mjs` parses the `app.css` tokens for **all three
themes** and computes WCAG 2.1 ratios for every pair the UI paints
(54 checks). **All pass.** Two real bugs were found and fixed:

| Bug (before) | Ratio | Fix | After |
|---|---|---|---|
| light theme: primary-button label (ink `#141414` on `--accent-500 #96650e`) | **3.65 < 4.5** | `--accent-500: #aa7a20` | 4.84 |
| light theme: `--fg-tertiary #6b6f76` on `--bg-elevated` | **4.28 < 4.5** | `--fg-tertiary: #61656c` | 4.96 |
| dark theme: `--fg-tertiary #7a8188` on panel/elevated | 4.51 / **4.14** (zero headroom / fail once the elevated pair is checked) | `--fg-tertiary: #82898f` | 5.02 / 4.61 |

Committed full table: run `node scripts/ui_contrast_audit.mjs` (or read the
CI log). Notes: `--fg-disabled` is WCAG-exempt (disabled controls); the
viewport canvas is image content, not UI text; the focus ring / active-tab
accent is held to the 3.0 UI-component gate.

## 4. Screen-reader announcements (UX_SPEC §8)

| Surface | Mechanism | Where |
|---|---|---|
| Render status | `role="status" aria-live="polite"`, visually-hidden, **debounced 400 ms** so a burst of parameter changes announces once — "Rendered in {ms} ms" / "Đã kết xuất trong {ms} ms" | `Viewport.svelte` |
| Confirmations ("Recipe saved", "Added to queue") | toast gained `role="status" aria-live="polite"` | `App.svelte` |
| Batch progress/errors | `aria-live` summary + per-row `role="alert"` errors (session 08) | `BatchQueue.svelte` |
| Blocking errors | `role="alert" aria-live="assertive"` (existing) | `ErrorBanner.svelte` |
| WB pick armed | `role="status"` banner (existing) | `Viewport.svelte` |
| Toolbar / icon buttons | `aria-label` on every glyph-only button; ✕ close buttons now say "Close"/"Đóng" instead of the literal glyph | `Toolbar.svelte`, dialogs |

## 5. Keyboard & focus (UX_SPEC §8)

The session-07 keyboard map is unchanged and complete. This session's
additions/fixes (all verified by the zero-warning `vite build` — the Svelte
compiler's a11y linter is now **clean repo-wide**, it was not before):

| Fix | Where |
|---|---|
| Home recent rows: `<li role="button">` → real `<button>` inside the `li` (keyboard + focus come native; a `li` may not carry `role=button`) | `Home.svelte` |
| **Esc now closes every dialog** (Export and Import-LUT were missing it; Esc is suppressed while a dialog's action is in flight) | `ExportDialog.svelte`, `ImportLutDialog.svelte`, `Settings.svelte`, `PresetManager.svelte`, `StockBrowser.svelte` |
| Backdrop-click dismissal removed from Settings/PresetManager (accidental dismissal of an unsaved draft is the worse failure; Esc + Close button remain) | `Settings.svelte`, `PresetManager.svelte` |
| StockBrowser: `aria-modal`, Esc, and initial focus moves to the Close button on open | `StockBrowser.svelte` |
| Viewport `tabindex="0"` removed — `role="img"` is non-interactive and the keyboard map is global (window listeners); the a11y warning is gone without losing any key | `Viewport.svelte` |
| Form-label without a control → `div` (compiler warning) | `Settings.svelte` |
| Dead CSS removed (unused selectors from the session-07 restructure); the off-palette local `button.primary` override removed so the audited global primary style applies | `TonePanel.svelte` |

## 6. Other §10 non-negotiables

| Requirement | Status |
|---|---|
| VI diacritics render/wrap | fonts are system stacks with full VI coverage; `overflow-wrap: break-word` globally; catalog diacritics gate at 95% |
| Reduced motion respected | `@media (prefers-reduced-motion: reduce)` kills all transitions/animations; no autoplay animation exists by design (compare is a manual toggle) |
| High-contrast theme variant | `data-theme="contrast"` (AAA-aiming) — audited above |
| Min touch targets 32 px | `button`, `select`, `input` → `min-height: 32px`; rail tabs bumped; **one documented exemption**: the Slider value readout (`.val`) — a click-to-type text surface whose row is the manipulation target |
| DPI-aware cursors/icons | **zero raster icons in the UI** — every glyph is font-rendered (scales with DPI/zoom by construction); cursors are OS system cursors |
| Full-screen preview keeps captions | no dedicated full-screen mode exists; viewport status labels are always rendered (structurally satisfied) |
| Focus ring always visible | `:focus-visible { outline: 2px solid var(--accent-400) }` in every theme (session 07, re-verified by the contrast table) |

## 7. Honest gaps (tracked)

* **Live screen-reader verification** (NVDA / Orca / VoiceOver on the three
  OSes) has not been performed — the sandbox has no SR. The aria structure
  is in place and CI-enforced where checkable; owner-device SR pass is
  queued for the M5 beta checklist (#16).
* The pseudo build proves compile + transform + wrap, not pixel-level
  overflow (no visual-regression infra); the proportional padding plus
  global wrap rule is the mitigation. A screenshot-diff harness is a
  post-beta candidate, deliberately not scope-crept into M4.
* Menu bar / window title come from Tauri defaults; they contain no
  product strings beyond the app name (verified — nothing to localize).
