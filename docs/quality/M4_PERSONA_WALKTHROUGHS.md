# M4 Persona Walkthroughs (P1–P5) — UX_SPEC §1 acceptance

**Date:** 2026-10-24 (session 08, #23 part 2) · **Spec:** `docs/UX_SPEC.md` §1
· **Method:** each walkthrough was executed against the real code paths
(components, commands, tests) — every claim names where it lives. Status
is honest: **pass**, or **deferred** with its tracking issue.

The personas drive every screen decision; their "failure to avoid" lists
are the acceptance bar. This document is the M4 evidence that the bar is
met (or explicitly tracked).

---

## P1 — Lan, casual photographer

> Open photo → pick a look → tweak one slider → export.
> Fears: jargon; module stacks; film-science vocabulary on first screen.

### Walkthrough

| Step | Where it lives | Verified by |
|---|---|---|
| 1. Drop a photo anywhere on Home | `Home.svelte` window-wide Tauri drag-drop | manual path in code; `onOpenPath` → `open_image` |
| 2. First screen shows photos, not modules | S1 Home = recents + drop zone + format chips; no pipeline vocabulary | component survey |
| 3. One-tap look: filmstrip under the photo | `Filmstrip.svelte` — original + every stock, click applies | `stock_thumbs` command; one undo step (apply_preset path) |
| 4. One slider: Exposure | Basic panel `Slider.svelte` (EV −5..+5, double-click reset, typed values) | `BasicPanel.svelte`; `set_tone` round-trip tests |
| 5. Before/after is obvious | hold-B original, Space compare-split with draggable divider | `Viewport.svelte`; §8 map |
| 6. Export | `ExportDialog.svelte` — factual format matrix, no re-encoding surprises | `export_image` → shared `export::run_export` |

**Status: PASS.** No screen Lan can reach shows chain/stage/encoding
jargon; Film Lab depth is one tab away, never in her path.

---

## P2 — Minh, creator

> Repeatable, trustworthy color; LUTs; batch export.
> Fears: **silent color-space lies; unexplained clipping.**

### Walkthrough

| Step | Where it lives | Verified by |
|---|---|---|
| 1. Build a look, save it as a preset | S5 Preset manager → `user_presets.rs` (sidecar-shaped stages, stable ids) | 4 unit tests incl. Vietnamese-name sanitize |
| 2. Queue a folder of photos with that preset | S6 `BatchQueue.svelte` adder (look = preset or current recipe) + `batch_add` | batch tests: `run_one_renders_and_notifies`, `snapshot_shape` |
| 3. Keep editing while the queue renders | worker thread decoupled from `EditorState` (`batch::spawn_worker`) — no command holds the editor lock during a render | `run_one` acquires/releases around the render; `batch://update` events |
| 4. Pause / cancel / retry | global pause (`batch_pause` — next file holds), cancel queued, retry **keeps settings** (stages live on the job) | `cancel_retry_remove_clear_state_machine` |
| 5. Disk-full error is actionable | row shows `io.disk_full` + "choose folder & retry" (`batch_retry` with `new_dest_dir`) | `error_mapping_is_actionable` (ENOSPC → hint) |
| 6. Outputs never overwrite | `reserve_output_path` suffixes `-2`, `-3`… | `reserved_paths_never_overwrite` |
| 7. Clipping is never silent | LUT domain clamp count → toolbar warn badge; export report carries `warnings` | `full_render_reports_clamping`; ExportDialog surfaces warnings |
| 8. Preview == export | export routes exactly like the preview: disabled film → LUT leg, disabled LUTs skipped (**two M2/M3 inconsistencies fixed this session**) | `film_active_only_when_enabled`, `resolve_film_disabled_keeps_flag` |
| 9. A recipe whose stock went missing never renders wrong colors | add-time registry check → row error `stock.missing` + hint | `missing_stock_is_an_honest_error`, `missing_stock_detected` |
| 10. LUT + film-stack honesty | film path cannot apply LUT stages yet → row warning `export.lut_skipped_by_film`, never silent | `WARN_LUT_SKIPPED_BY_FILM` + warning surfaced in rows and single-export toast |

**Status: PASS.** Every "silent lie" class has either a test or an
explicit surfaced warning. The one true gap (LUT insertion points inside
the film chain) is *shown to the user* and tracked in PLAN (spatial/insertion
follow-up).

---

## P3 — Hưng, film enthusiast

> Push/pull, halation physics, grain spectra, print vs scan paths.
> Fears: **fake "measured" labels; physics-flavored marketing.**

### Walkthrough

| Step | Where it lives | Verified by |
|---|---|---|
| 1. Push/pull, dev contrast, print exposure/contrast | Film Lab panel (`FilmLab.svelte`) → `set_film` → M2 chain stages A–I | M2 §L gate tests (PR #20) |
| 2. Grain & halation physics | Grain·Halation panel; σ-calibrated per stock (FILM_MODEL §G/§C); recipe-level overrides documented as follow-up | `grain.profileNote` honesty string; chain tests |
| 3. Print vs scan path toggle | Film Lab `inversion_path` | stock profile schema (`#11`) |
| 4. Provenance is honest | stock browser shows "artist-authored — no measured calibration" prominently | `StockJson.provenance_text` per UX_SPEC §5 |
| 5. Resume keeps the film chain | sidecar stages re-applied on open (the M2/M3 film-resume gap was fixed in session 07) | `apply_pipeline_stages` single apply path |
| 6. Batch keeps film looks faithful | the queue runs the SAME `cmd_film_apply` the CLI/CI exercise | `run_export_identity_png` + film leg via `FilmApplyOptions` |

**Status: PASS** (carried from M2/session-07 work; nothing in S6
regressed it — the shared export path reuses the same CLI legs).

---

## P4 — LUT author

> Import/validate `.cube`, declare encodings.
> Fears: **silent reinterpretation of LUT color space.**

### Walkthrough

| Step | Where it lives | Verified by |
|---|---|---|
| 1. Import with declared in/out encodings | `ImportLutDialog.svelte` → `import_lut` (content-addressed, SHA-verified) | lut crate suites; import warnings (non-monotonic, tiny grid, domain) |
| 2. Insertion point + intensity | LUT panel; `LutApply` carries both into sidecars, presets AND batch jobs | `StageEntry::Lut` round-trips |
| 3. No silent reinterpretation | undeclared = warned, never guessed silently; export re-imports the stored blob (byte-identical) | `lut.missing`/checksum paths; `error.rs` taxonomy |
| 4. Batch with LUT-only recipes | LUT leg of the standalone export (tone + first enabled LUT) | export.rs routing tests |

**Status: PASS.**

---

## P5 — low-vision user (cross-cutting)

> Screen reader, high contrast, keyboard-only.
> Fears: **canvas-only UI with no accessible semantics.**

### Walkthrough

| Step | Where it lives | Verified by |
|---|---|---|
| 1. Full keyboard path | §8 map: O · E · [ ] · 0/1/2 · +/− · Space · Ctrl+S/Z/Y · B · Z; `Tab` ring visible; inputs exempt | `App.svelte` `onKey` + theme focus tokens |
| 2. Batch is keyboard-reachable | Toolbar button (focusable, labeled) + Esc returns; per-row buttons are real buttons | `Toolbar.svelte`, `BatchQueue.svelte` |
| 3. Render status is announced | batch live region (`aria-live=polite`): "Rendering file 2 of 5" / "Batch finished — 5 exported" | `BatchQueue.svelte` `$: live` |
| 4. Errors are announced | error rows `role=alert`; ErrorBanner for command errors | components |
| 5. High-contrast theme | dark/light/contrast token sets; the viewport stays dark in all (judging photographs) | `theme.ts`, Settings |
| 6. Canvas has semantics | viewport `role=img` + aria-label; status strip renders as text, not pixels | `Viewport.svelte` |
| 7. Min touch targets / contrast ≥ AA | tokens enforce (UX_DESIGN_TOKENS) | token audit |

**Status: PASS for the walkthrough;** the *systematic* a11y audit
(screen-reader pass on all three OSes, pseudo-locale CI, full catalog
sweep of M2/M3 strings) is issue **#15** — tracked, not claimed.

---

## Honest deferrals (tracked, not hidden)

| Item | Tracking |
|---|---|
| LUT insertion points inside the film chain (today: surfaced warning) | PLAN spatial/insertion follow-up |
| Kill-during-EXPORT battery (same durable primitive, export writer) | #25 |
| Full a11y/i18n audit + pseudo-locale CI | #15 |
| Perf/memory budgets, fuzz battery, secret scan | #25 |
