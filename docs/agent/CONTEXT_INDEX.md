# MELOGRAPHY — Context Index

Problem → file map. Cold-start reading order for a new agent session: **AGENTS.md → docs/agent/STATE.md → this file → latest session log → the target document below.**

## By problem domain

| I need to… | Open |
|---|---|
| Understand the project, safety rules, session protocol | `AGENTS.md` |
| Know current status / branch / PR / next actions | `docs/agent/STATE.md` |
| Understand the system design, crates, traits, tiling, errors | `docs/ARCHITECTURE.md` |
| Get the math of color stages, ICC, tolerances, oracle | `docs/COLOR_PIPELINE.md` + ADR-0002 |
| Get the math of film emulation (curves/halation/grain/print) | `docs/FILM_MODEL.md` + `docs/research/grain.md` |
| Understand LUT formats, interpolation, encoding contracts | `docs/research/luts.md` (+ `docs/FILM_MODEL.md` §insertion points) |
| Decide/use AI models, budgets, vetting | `docs/AI_MODELS.md` + ADR-0004 + `docs/research/ai.md` |
| Build/review UI, tokens, a11y, i18n | `docs/UX_SPEC.md` + `docs/UX_DESIGN_TOKENS.md` |
| Package, sign, release, update, rollback | `docs/RELEASING.md` + ADR-0005 |
| See the roadmap, milestones, traceability, risks | `docs/PLAN.md` |
| Copy third-party code or bundle an asset | `docs/adr/0003-licensing.md` + `skills/license-and-asset-vetting/SKILL.md` |
| Add/verify a reference repo/paper/dataset | `docs/research/REFERENCES.md` + `references/manifest.toml` + `skills/reference-research/SKILL.md` |
| Write/change a workflow or CI behavior | `.github/workflows/ci.yml` + `skills/cross-platform-ci/SKILL.md` |
| Find what the owner still must decide | `docs/research/OPEN_QUESTIONS.md` |
| Verify why a stack/color/AI choice was made | ADR-0001 / 0002 / 0004 (+ spike reports at M0: `docs/research/spikes/`) |

## ADR map

0001 technology stack (Tauri provisional, egui fallback) · 0002 color (ACEScg + lcms2) · 0003 licensing (GPL-3.0-or-later + reuse matrix) · 0004 local AI (parameter predictors, vetting gate) · 0005 packaging/updates (channels, signing layers, gating).

## Skills quick map

Boundary rituals → `session-handoff` · decisions → `architecture-decisions` · film math → `film-science-and-profiles` · any color math → `color-management` · LUTs → `lut-engine` · GPU → `wgpu-and-cpu-parity` · models → `onnx-model-vetting` · CI → `cross-platform-ci` · release → `secure-release-and-update` · reuse → `license-and-asset-vetting` · UI → `desktop-ui-design` · references → `reference-research`. Index: `skills/README.md`.

## Minimal cold-start sets

- **M0 implementer:** AGENTS → STATE → ADR-0001 → ARCHITECTURE (§2, §3) → PLAN (M0) → skills: wgpu-and-cpu-parity, color-management.
- **M2 film engineer:** AGENTS → STATE → FILM_MODEL → grain.md → film-color.md → skills: film-science-and-profiles.
- **Release manager (owner-gated):** AGENTS → ADR-0005 → RELEASING → skills: secure-release-and-update, cross-platform-ci.
