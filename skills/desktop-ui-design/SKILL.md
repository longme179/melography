---
name: desktop-ui-design
description: Use when designing or implementing any UI surface (screens, panels, controls, states, i18n strings, a11y). Keeps the workspace color-judgment-safe, localized EN/VI, and accessible — adapting taste-skill principles for a desktop color-critical app.
---

# Desktop UI Design

## Prerequisites
- Read `docs/UX_SPEC.md` (screen contracts) + `docs/UX_DESIGN_TOKENS.md` (visual constants — tokens are the only source of visual truth).
- Provenance color meanings are fixed cross-doc (tokens §provenance): measured/reference-fitted/artist-authored/experimental.

## Workflow
1. Start from the screen contract in UX_SPEC (inventory §2, layout §4–§7); wireframe changes as text/ascii in the PR before pixel work.
2. **Color-judgment safety first:** no decorative color/gradient/glow near the viewport; neutral surround tokens only; the image is the only saturated element. Any UI element that could bias perception of the photo (strong hue fields, animated backgrounds) is rejected.
3. Tokens over values: no hardcoded colors/sizes in components; new visual needs → new token + rationale + EN/VI length check (VI +35%).
4. Interaction contract per control: range, unit, default, reset (double-click), nudge (arrows), typed values (UX_SPEC §6 table is normative).
5. States discipline: every surface has empty/loading/rendering/error/update states mapped from the error taxonomy (ARCHITECTURE §8); errors always offer a next action; no dead ends.
6. Keyboard & a11y: full keyboard path (map in UX_SPEC §8); focus visible (2px accent + offset); screen-reader announcements for async status; contrast AA verified against token pairs.
7. Honesty surfaces: provenance chips, "[ART]" tags with tooltips, "encoding assumed" ambers, clipped-input flags — these are features, not decoration; never hide ambiguity from the user.
8. Light theme + high-contrast variants updated in the same change (parity is required).
9. Screenshots (EN + VI, dark + light) attached to the PR for any visual change.

## Verification checklist
- [ ] No new hardcoded values; tokens used (or extended with rationale)
- [ ] VI strings render (diacritics, no truncation at +35% length)
- [ ] Keyboard-only walkthrough of the changed surface recorded
- [ ] Error/empty/loading states implemented + localized
- [ ] Contrast checks pass for new text/background token pairs

## Common pitfalls
- Web-trend imports (glassmorphism, big colorful heroes) that corrupt photographic judgment — this is a *tool*, not a landing page.
- English hardcoded "for now" — every string goes through catalogs from the first commit.
- Modal error dialogs for recoverable states (inline errors are the default).
- Hiding physics dishonesty ("measured" chip without data) — the UI is the honesty layer.

## Links
`docs/UX_SPEC.md` · `docs/UX_DESIGN_TOKENS.md` · pinned `taste-skill` reference (REFERENCES §A — principles adapted, not blindly applied)

## Expected outputs
Wireframe + token-compliant implementation + EN/VI screenshots + a11y walkthrough notes.
