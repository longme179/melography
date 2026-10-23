---
name: film-science-and-profiles
description: Use when implementing or modifying film-chain stages (curves, crosstalk, halation, grain, print/scan) or stock profile data. Enforces the physical model, determinism contract, and honest provenance labels.
---

# Film Science & Profiles

## Prerequisites
- Read `docs/FILM_MODEL.md` fully (stage math is normative) and `docs/research/film-color.md` + `grain.md` for the evidence base.
- Tests-first discipline: every stage lands with property tests + a golden case before review.

## Workflow
1. Locate the stage in the A–I chain; read its contract (inputs/outputs/units) — stages are ordered for physical reasons; reordering requires justification recorded in the PR.
2. Implement the CPU oracle first (`melography-film`); write property tests: curve monotonicity, no-NaN/Inf at parameter extremes, energy bounds (halation ε ≤ 0.05), density bounds.
3. Implement GPU mirror (`melography-gpu`) only after CPU golden hashes exist.
4. Mark every artistic parameter `[ART]` in code docs + UI strings; physical parameters cite their source (paper/section) in the module doc.
5. Stock profiles: edit JSON data, never code. Validate against `schemas/stock-profile.schema.json`; update `profile_version` on any parameter semantics change; migration path in `melography-presets`.
6. Provenance: `measured` requires dataset + fit report in `tools/provenance.json`; otherwise `artist-authored`/`experimental` — the UI badge depends on this honesty.
7. Run the film-statistical gates (FILM_MODEL §L): grain RMS/PSD/ρ, halation radius, determinism, tiled≡monolithic.

## Verification checklist
- [ ] Property tests for the stage green (monotonic, bounded, deterministic given seed)
- [ ] Tiled-vs-monolithic equality test added and green
- [ ] Preview/export grain statistics within ±10%/±15%/±0.05 targets
- [ ] Profile schema validation + version bump + migration note
- [ ] No branded stock names/data without permission (ADR-0003)

## Common pitfalls
- Applying halation after scanning (wrong stage — it happens at exposure); using lens bloom as halation substitute.
- Pixel-grid-dependent grain (breaks zoom determinism) — grain is a function of film coords, seed, params ONLY.
- Skipping the print/scan second nonlinearity ("filter look" instead of "film look").
- Tweaking constants in code instead of profile data.

## Links
`docs/FILM_MODEL.md` · `docs/research/grain.md` · `docs/research/film-color.md` · ADR-0003

## Expected outputs
Stage implementation + tests + golden hashes; profile JSON + schema validation; updated quality-evidence numbers in the PR description.
