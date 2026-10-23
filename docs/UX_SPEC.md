# MELOGRAPHY — UX Specification

**Version:** Phase-0 spec · **Date:** 2026-10-24
**Related:** `UX_DESIGN_TOKENS.md` (visual constants), `ARCHITECTURE.md` (system behind each screen)

## 1. Personas (drive every screen decision)

| Persona | Need | Failure to avoid |
|---|---|---|
| **P1 — Casual photographer** ("Lan"): open photo → pick a look → tweak one slider → export | One-click looks, obvious before/after | Jargon; module stacks; film-science vocabulary on first screen |
| **P2 — Creator** ("Minh"): repeatable, trustworthy color; LUTs; batch export | Precise controls, presets, export queue | Silent color-space lies; unexplained clipping |
| **P3 — Film enthusiast** ("Hưng"): push/pull, halation physics, grain spectra, print vs scan paths | Film Lab depth with honest provenance | Fake "measured" labels; physics-flavored marketing |
| P4 — LUT author (subset of P2): import/validate `.cube`, declare encodings | Validation feedback, provenance fields | Silent reinterpretation of LUT color space |
| P5 — Low-vision user (cross-cutting): screen reader, high contrast, keyboard-only | Full keyboard path, announced states | Canvas-only UI with no accessible semantics |

## 2. Screen inventory (7 screens + 2 dialogs)

```
S1 Home ── S2 Editor ── S6 Batch export
   │           │
   │           ├── S3 Film stock browser (overlay)
   │           ├── S4 Panels (right rail): Basic / Color / Film Lab / LUT / Lens / Grain·Halation / AI / Export
   │           └── S5 Preset manager (overlay)
S7 Settings (window) · D1 Import LUT dialog · D2 Model manager dialog
```

## 3. S1 Home / Recent & Import

- Recent files list (thumbnails, path, last-opened) + big "Open image" drop zone (drag-drop anywhere). Empty state: friendly explainer + supported-format chips (JPEG/PNG/TIFF/WebP; HEIC/AVIF badges show "experimental" per M0 codec matrix).
- Each recent entry shows a mini provenance line (recipe present? modified?). Broken file = inline error row, not a modal.

## 4. S2 Editor (the heart)

```
┌──────────────────────────────────────────────────────────────┐
│ toolbar: [◀Home] [Compare ▷|◁] [100%|Fit] zoom − + [Export…] │
├──────────────────────────────────────────────┬───────────────┤
│                                              │  PANEL RAIL   │
│              VIEWPORT (canvas)               │  [Basic][Color]│
│   pan: drag · zoom: wheel/pinch              │  [Film Lab]   │
│   compare: split slider (drag) / hold B      │  [LUT][Lens]  │
│   100% check: pixel loupe (hold Z)           │  [Grain][AI]  │
│                                              │  [Export]     │
│   status: render time · cache · zoom · ICC   │  (tab order   │
│   amber badges: "LUT domain clipping",       │   fixed ↑↓)   │
│   "encoding assumed", "clipped input"        │  …params…     │
├──────────────────────────────────────────────┴───────────────┤
│ filmstrip: [orig] [stock thumbs applied live to THIS photo]  │
└──────────────────────────────────────────────────────────────┘
```

- **Viewport correctness rules:** neutral dark surround (tokens); checkerboard under transparency; pan/zoom never triggers re-render *except* required new detail (progress subtle, cancelable); 100% preview pixel == export pixel within published tolerance (COLOR_PIPELINE §8) — a "100% loupe" badge guarantees it.
- **Compare modes:** split slider (default), side-by-side, hold-B temporary original. Split line draggable; label "original | edited" localized.
- **History:** undo/redo (Ctrl+Z / Ctrl+Shift+Z) infinite within session, bounded persistence; autosave indicator (dot, never modal).
- Zoom modes: Fit / Fill / 100% / 200% / free wheel. Min window 1024×640; layout reflows (panels collapse to icons under 1280 px width).

## 5. S3 Film stock browser

- Grid of stock cards: **swatch = user's own current photo, thumbnailed through the stock's pipeline** (cheap: incremental preview); name; provenance chip color-coded (`measured` green, `reference-fitted` teal, `artist-authored` blue, `experimental` amber); favorite star; crop/aspect of swatch = current viewport crop.
- Detail drawer (P3 depth): curve mini-plots (RGB D–logE), grain spectrum summary, halation radius, print/scan path, physical references + confidence + license line. Honesty rule: profiles without calibration show "artist-authored — no measured calibration" prominently.

## 6. S4 Panels (parameter surfaces — ranges & units are part of the contract)

| Panel | Controls (range · unit · default) |
|---|---|
| Basic | Exposure EV (−5..+5 EV, 0.01) · Contrast (−100..100) · Highlights/Shadows (−100..100) · Whites/Blacks (−100..100) |
| Color | Temperature (2000..12000 K) · Tint (−100..100) · Vibrance/Saturation (−100..100) · WB picker tool |
| Film Lab | Stock selector · Input assumption · Push/Pull (−3..+3 stops, ½-step) · Dev contrast (0.6..1.6) · Print exposure · Print contrast · Paper black/white point · Scan-vs-Print path toggle · Orange mask visibility · Cross-process `[ART]` · Bleach bypass `[ART]` — `[ART]` chips explained on hover |
| LUT | Library grid (provenance badges) · Insertion point (pre-film/film-input/print-output/finishing) · Intensity (0..100%) · Encoding declare (D1 on import) · warnings ("domain clipping", "encoding assumed") |
| Lens | Vignette (−100..100) · Veiling/diffusion (0..100) · CA `[ART off]` · Softness (0..100) |
| Grain·Halation | Grain amount (0..200%) · Size scale (0.5..2×) · Shadow weight · Layer correlation lock · Seed (dice) · Halation amount (0..100%) · radius (mm film) · red bias |
| AI | Baseline "Auto light" button (always) · model card (install/measure/delete) · suggestion preview (Accept/Adjust/Dismiss) · optional mask tools |
| Export | Format · quality/bit depth · profile & intent · dither toggle · metadata policy (EXIF/XMP/GPS strip ask) · resize · preset save |

Every slider: double-click resets; arrow keys nudge; typed exact values; localized number formatting; units in label. Tooltips explain the *physical* meaning where one exists ("Halation: red light reflecting off the film base during exposure").

## 7. S6 Batch export queue + S7 Settings

- Queue: rows (thumb, name, preset, size, status: queued/rendering/done/error+retry), global pause/cancel; errors are per-file with actionable text (disk full → open folder chooser). Retry keeps settings.
- Settings sections: General (language EN/VI, theme dark/light/contrast), Color (default export profile, soft-proof default), Performance (preview size, cache budget slider, GPU toggle + device info), Models (manager per AI_MODELS §6), Updates (channel: none/stable/beta — default *ask*, never silent), Privacy (offline mode toggle; "what leaves this machine: nothing" explainer), Advanced (log level, cache location).

## 8. Interaction & keyboard map (core)

`O` open · `Ctrl+S` save recipe · `B` hold-compare · `Z` hold-loupe 100% · `+/-` zoom · `0` fit · `1` 100% · `2` 200% · `[ ]` cycle panels · `Tab` panel focus ring (visible, 2px accent) · `Space` play/pause compare animation · `Ctrl+Z/Shift+Z` undo/redo · `E` export dialog. All actions reachable via keyboard; focus order documented per screen; screen-reader announcements for render status, export progress, errors.

## 9. States & errors (no dead ends)

Empty (each screen has a purposeful first-run state) · Loading (skeleton shimmer, never spinner-only > 2 s without status text) · Rendering (progress with cancel; stale-result badge if params changed mid-render) · Error (message + cause + next action, mapped from ARCHITECTURE §8 taxonomy, localized) · Update available (badge in Settings only, never modal; "install now / later" consent flow) · Model states (AI_MODELS §6).

## 10. Localization & accessibility (non-negotiables)

- All UI strings in catalogs (`en`, `vi`) from day one — **zero embedded English**; pseudo-locale test build in CI (M4).
- Vietnamese typography tested (diacritics render/line-break correctly at all sizes); number/date locale-aware.
- Contrast ≥ WCAG AA (tokens enforce); reduced-motion respected (no compare-animation autoplay); high-contrast theme variant; DPI-aware cursors/icons (SVG); min touch targets 32 px; full-screen preview mode keeps captions.
- The dark-neutral environment is *designed for judging photographs* — no decorative gradients in the viewport chrome; the UI recedes, the image speaks (see tokens rationale).
