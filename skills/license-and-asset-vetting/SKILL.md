---
name: license-and-asset-vetting
description: Use before copying any third-party code, adding any dependency, or bundling any asset (LUT, image, profile, model weights, font). Runs the ADR-0003 policy matrix and records provenance.
---

# License & Asset Vetting

## Prerequisites
- Read ADR-0003 (policy matrix is normative). Project license: GPL-3.0-or-later.
- Understand the four separated rights: reading a paper ≠ using a standard ≠ copying code ≠ redistributing assets.

## Workflow
1. **Classify the item:** code / asset / dependency / standard-spec / paper.
2. **Code & dependencies:** verify license at the *pinned revision* (licenses change); REUSE allowed for GPL-3.0(-or-later) and MIT/Apache/BSD/ISC; AGPL → no copying; no-license/unclear → no copying (study + reimplement + cite). For every reuse: keep original headers, add `THIRD_PARTY_NOTICES.md` entry, add `tools/provenance.json` entry (URL + SHA + files + license).
3. **Assets** (LUTs, film scans, ICC profiles, datasets, weights, fonts): asset license is independent of its repo's code license. Find the asset-specific license text. Verified trap: spektrafilm code GPL-3.0 but LUTs "no resale" — assets like that NEVER ship. Record: source, license, redistribution terms, modifications.
4. **Papers/specs:** re-implementing an algorithm from a paper is fine (cite paper); copying reference *code* follows rule 2; redistributing *figures/data* follows rule 3.
5. **Trademarks:** generic original names only for shipped profiles; manufacturer names in citations only (ADR-0003).
6. Update `cargo-deny` config for new dependencies (licenses/advisories/bans) and let CI enforce it (M0+).
7. If any doubt remains after the above, mark BLOCKED in `docs/research/OPEN_QUESTIONS.md` (or a new entry) with the exact missing fact — do not guess.

## Verification checklist
- [ ] License verified at pinned revision (not HEAD of a moved repo)
- [ ] Headers preserved; NOTICE + provenance entries added
- [ ] Assets individually cleared (not inherited from repo license)
- [ ] `cargo-deny`/license CI checks updated and green
- [ ] Stock names original; no endorsement implication

## Common pitfalls
- Trusting the GitHub API `license` field alone (NOASSERTION/nested LICENSES dirs need file reading).
- Bundling "example" LUTs/images from a repo whose license covers code only.
- Vendoring code "temporarily" without provenance — temp becomes permanent.

## Links
ADR-0003 · `docs/research/REFERENCES.md` · `references/manifest.toml` · `tools/provenance.json` (M0+)

## Expected outputs
Provenance entries + NOTICE updates + (if blocked) an open question with the missing fact.
