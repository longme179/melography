# MELOGRAPHY Skills Index

Project-authored, agent-oriented workflows. **Skills are advisory** — they cannot override `AGENTS.md` §2 (safety rules), licensing constraints, owner instructions, or the Phase gates. This harness has **no skill auto-discovery**: open the relevant `SKILL.md` manually before starting a task it covers (fallback invocation is the documented default).

| Skill | Trigger — when to use | Precedence | Related source (pinned) | Version |
|---|---|---|---|---|
| `session-handoff` | Start/end of every working session | Above all others at session boundaries | superpowers methodology (adapted) | 1.0 |
| `architecture-decisions` | Structural/irreversible technical choices → ADRs | Over ad-hoc reasoning | ADR-0001 as worked example | 1.0 |
| `film-science-and-profiles` | Film-chain stages, grain, stock profile data | Over intuition in film math | FILM_MODEL.md; Newson et al.; darktable/spektrafilm study | 1.0 |
| `color-management` | Any pixel math, ICC, display/export paths | Over "looks right" judgments | ADR-0002; COLOR_PIPELINE.md | 1.0 |
| `lut-engine` | LUT parse/validate/interpolate/library/UI | Over lenient parsing | luts.md; OCIO format discipline | 1.0 |
| `wgpu-and-cpu-parity` | WGSL kernels, preview transport, GPU tests | Over GPU-as-truth | ADR-0001 S1; ARCHITECTURE §5 | 1.0 |
| `onnx-model-vetting` | Before any model ships/downloads; model manager changes | Blocks model integration | ADR-0004; AI_MODELS.md | 1.0 |
| `cross-platform-ci` | Workflow/runner/cache changes | Over CI improvisation | RELEASING §6; ADR-0005 | 1.0 |
| `secure-release-and-update` | Release, signing, updater, artifacts | Blocks publish paths | ADR-0005; RELEASING.md | 1.0 |
| `license-and-asset-vetting` | Copying code; new deps; any asset bundling | Blocks merges with unvetted reuse | ADR-0003; REFERENCES.md | 1.0 |
| `desktop-ui-design` | Any UI surface, tokens, i18n, a11y | Over web-trend imports | UX_SPEC; UX_DESIGN_TOKENS; taste-skill (adapted) | 1.0 |
| `reference-research` | Adding/consulting third-party repos, papers, datasets | Over ad-hoc citations | manifest.toml; REFERENCES.md | 1.0 |

## Skill format contract

Each `skills/<name>/SKILL.md`: YAML front matter (`name`, `description` stating *when to use*) → Prerequisites → Workflow → Verification checklist → Common pitfalls → Links → Expected outputs. Updates bump the table's Version column and mention the change in the PR.

## Third-party skill sources (status)

`obra/superpowers` (MIT) and `leonxlnx/taste-skill` (MIT) are **pinned references** (see `docs/research/REFERENCES.md` §A): their *methodology* and *design judgment* are adapted into the project-authored skills above. Their installers/hooks are never executed; no plugin auto-discovery is claimed. `anthropics/skills` has no license file → format convention reference only, no content copied. `build-your-own-x` (CC0 by README) is a learning index; linked tutorials carry their own licenses.
