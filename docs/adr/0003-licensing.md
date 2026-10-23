# ADR-0003: Licensing & Code Reuse Policy

**Status:** Accepted (owner decision recorded 2026-10-24, chat; awaiting one-click confirmation on the Phase-0 PR — OPEN_QUESTIONS #1) · **Date:** 2026-10-24

## Context

The master prompt required an ADR contrasting MIT, Apache-2.0, GPL-3.0-or-later, and dual-licensing, with a provisional clean-room policy while undecided. During Phase 0 the owner made the decision in chat: *this is a personal, non-commercial passion project; the owner wants the freedom to reuse code from GPL film-science projects (darktable, spektrafilm, agx-emulsion) and accepts changing the project's own license to enable it.*

The four rights being separated (master prompt §13): reading an algorithm/paper; using a public standard; copying source code; redistributing assets (LUT files, scans, model weights). **This ADR governs the third and fourth.**

## Decision

**MELOGRAPHY is licensed GPL-3.0-or-later.** The owner is (currently) the sole copyright holder; relicensing freedom exists today and shrinks as external contributors arrive (no CLA planned; contributions accepted under GPL-3.0-or-later by default — a future CLA decision is the owner's).

### Code reuse policy (active immediately)

| Source license | Reuse? | Conditions |
|---|---|---|
| GPL-3.0 / GPL-3.0-or-later (darktable, RawTherapee, spektrafilm code, agx-emulsion) | ✅ | Preserve original copyright headers; log in `THIRD_PARTY_NOTICES.md` (created with the first reused file, M0+); record source URL + pinned revision in `tools/provenance.json`; the containing module stays GPL (whole project is GPL — trivially satisfied) |
| MIT / Apache-2.0 / BSD / ISC (wgpu, tauri, ort, lcms2, image, egui, …) | ✅ | Same NOTICE/provenance logging; license texts preserved/accumulated in `THIRD_PARTY_NOTICES.md` |
| AGPL-3.0 (RapidRAW) | ❌ no copying | Architectural study only (documented why: keeps network-copyleft obligations out of a desktop app's supply chain and avoids mixed-copyleft review burden) |
| No license file / "research only" / unclear | ❌ no copying | Study the *paper/spec*, re-implement, cite. Verified examples found this session: `Li-Chongyi/Zero-DCE` (no LICENSE), `anthropics/skills` (no LICENSE) |
| Assets (LUT files, film scans, ICC profiles, datasets, model weights) | ⚠️ individually | Assets carry licenses **independent of the repository they live in**. Verified example: spektrafilm code = GPL-3.0 but its LUTs = custom "commercial use, free share, no resale" — those LUTs must never ship with MELOGRAPHY. Every asset needs its own clearance recorded in `tools/provenance.json` before distribution. |

### Why not MIT / Apache-2.0 / dual?

- MIT/Apache would forbid the GPL-code reuse the owner explicitly wants for the film-science core (darktable's negadoctor/filmic and spektrafilm's spectral model are the highest-value references).
- Dual-license (GPL + commercial) is only meaningful with a CLA/ownership discipline and commercial intent — owner explicitly declined commercial focus; revisit if that changes (this ADR is then superseded).

### Supply chain & trademarks

- Dependency checks: `cargo-deny` (licenses + advisories + bans) + `cargo audit`; lockfiles committed; npm license tooling when the UI tree appears (M0).
- `THIRD_PARTY_NOTICES.md` accumulates all third-party licenses from the first dependency/reuse (M0), generated + reviewed in CI.
- Stock profiles: **generic original names only** (e.g., "Warm 400 Negative"); manufacturer names appear only in documentation/citations, never implying endorsement (trademark hygiene, unchanged from master prompt).

## Consequences

- **Positive:** maximum reuse of the best film-science implementations; single copyleft story; personal non-commercial use unaffected.
- **Negative:** future closed-source/commercial derivatives require rewriting/removing GPL portions (owner accepted); some developers avoid GPL projects.
- **Neutral:** GPL-3.0 is compatible with all selected dependencies (MIT/Apache/BSD) and with Tauri/wgpu/ort stacks [verified in REFERENCES.md].

## Review triggers

Owner commercial intent changes; a desired dependency with incompatible license appears; first external contributor questions contribution terms.
