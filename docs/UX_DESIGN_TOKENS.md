# MELOGRAPHY — UX Design Tokens

**Version:** Phase-0 (CSS custom properties + Rust/Svelte parity) · **Date:** 2026-10-24
**Rationale:** a *neutral, recessive* dark workspace so the photograph — the only saturated object on screen — is judged accurately. One accent color; color-meaning reserved for provenance/warning states. WCAG AA minimum everywhere.

## Color — dark theme (default)

```css
:root {
  /* surfaces (neutral, slightly cool) */
  --bg-app:        #0E0F11;   /* app background            L*≈5  */
  --bg-viewport:   #101112;   /* around canvas             L*≈6  */
  --bg-panel:      #16181B;   /* panel rail                L*≈9  */
  --bg-elevated:   #1D2024;   /* cards, popovers           L*≈13 */
  --bg-hover:      #24282D;
  --bg-active:     #2B3036;

  /* content */
  --fg-primary:    #E7E9EC;   /* 15.8:1 on bg-app  ✓AAA */
  --fg-secondary:  #A6ADB4;   /*  8.1:1            ✓AAA */
  --fg-tertiary:   #7A8188;   /*  4.9:1            ✓AA  */
  --fg-disabled:   #565C63;   /*  2.6:1 (disabled exempt) */

  /* lines */
  --stroke-subtle: #23262A;
  --stroke-strong: #34383E;
  --stroke-focus:  --accent-400;

  /* single accent (selection, focus, primary action) — warm amber, photograph-adjacent but not a camera-primary hue */
  --accent-400:    #E8A33D;   /* on bg-panel 7.9:1 ✓AAA */
  --accent-500:    #D18A28;
  --accent-emph:   #F2BE66;   /* on dark text */

  /* semantic (sparing use) */
  --ok:            #4CB782;   /* measured provenance, success */
  --info:          #57A9C4;   /* reference-fitted */
  --neutral-info:  #7A8CC0;   /* artist-authored */
  --warn:          #E0B34C;   /* experimental, "encoding assumed", clipping */
  --danger:        #D9614C;   /* errors, destructive */
}
```

**Viewport surround** is `--bg-viewport` — deliberately darker than panels to frame the image; never pure black (glare) nor gray (muddies shadow judgment). **No decorative color** anywhere in chrome: gradients, glows, and shadows are banned in panel surfaces.

## Light theme (parity set)

`--bg-app #F4F5F6 · --bg-panel #FFFFFF · --fg-primary #1A1D21 · accent #A66A14 (darkened for AA) · semantic hues same families, darkened variants`. High-contrast theme: pure #000/#FFF pairs + 3px focus ring. Theme tokens swap only; spacing/type identical.

## Typography

| Token | Value | Use |
|---|---|---|
| `--font-ui` | `Inter, "Segoe UI", system-ui, sans-serif` (+ VI subset check) | UI text |
| `--font-mono` | `"JetBrains Mono", "Cascadia Code", ui-monospace, monospace` | values, curves, EXIF |
| `--type-h1` | 600 20px / 1.3 | screen titles |
| `--type-h2` | 600 16px / 1.35 | panel section |
| `--type-body` | 400 13.5px / 1.5 | controls (desktop density) |
| `--type-small` | 400 12px / 1.45 | captions, badges |
| `--type-data` | mono 12.5px, `font-variant-numeric: tabular-nums` | sliders/readouts |

## Spacing, radius, elevation

```
--space-1: 4px   --space-2: 8px   --space-3: 12px   --space-4: 16px
--space-5: 24px  --space-6: 32px  --space-7: 48px   (4px base grid)
--radius-s: 4px  --radius-m: 8px  --radius-l: 12px  (cards only)
--shadow-panel: 0 1px 0 rgba(0,0,0,.4)              (flat chrome, one hairline)
--shadow-pop:   0 8px 24px rgba(0,0,0,.5)           (popovers only)
```

## Controls

| Element | Spec |
|---|---|
| Slider | track 4px, thumb 14px, focus ring 2px `--accent-400` + 2px offset; value tooltip on drag; double-click reset |
| Panel tab | height 36px, icon+label, active = 2px underline accent (not fill) |
| Button | height 30px (primary filled accent; secondary hairline); min width 72px |
| Badge | 11px, 4px radius pill, 2px inset border over translucent bg (provenance: `--ok/--info/--neutral-info/--warn`) |
| Checkbox/Radio | 16px, 3px gap to label |
| Focus | always visible: 2px accent ring + 2px offset; `:focus-visible` only (mouse users spared) |
| Min target | 32×32px interactive |

## Motion

`--motion-fast: 120ms ease-out` (hover/press) · `--motion-panel: 200ms cubic-bezier(.2,.7,.3,1)` (panel switch) · reduced-motion: all durations → 0 except opacity.

## i18n & RTL notes

VI strings measured up to +35% length vs EN → layouts must not fixed-width truncate; test both locales in component gallery (M4). RTL not in scope (EN/VI are LTR) but token names keep logical (start/end) conventions to keep the door open.

## Provenance color contract (cross-doc reference)

`measured → --ok` · `reference-fitted → --info` · `artist-authored → --neutral-info` · `experimental → --warn` — identical mapping in stock browser, LUT library, AI model cards (UX_SPEC §5–§7).
