# MELOGRAPHY — Agent State

**Updated:** 2026-10-24T00:00:00Z (end of Phase-0 session 01) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**Phase 0 (research/architecture/scaffolding)** — execution complete; **awaiting owner review of the draft PR and M0 authorization**. No M0 work started (gate: AGENTS.md §10).

## Branch / PR / HEAD

- Working branch: `agent/phase-0-foundations` (pushed)
- Draft PR: see session log / repo PR list — *the agent records the exact PR number+URL in the session log after creation; if a PR number is absent there, no PR exists yet.*
- HEAD at session end: recorded in `docs/agent/sessions/2026-10-24-01.md` (single source of truth for SHAs)
- `main`: single bootstrap commit `97210e0` (documented exception, REPO_AUDIT §2)

## Verified completed tasks (Phase 0)

1. ✅ Repository audit (empty, public, admin perms) — `docs/research/REPO_AUDIT.md`
2. ✅ One-time bootstrap of `main` (README/.gitignore/AGENTS.md only) — commit `97210e0`
3. ✅ 25+ references verified + pinned (licenses, SHAs, usage policy) — `REFERENCES.md` + `references/manifest.toml`
4. ✅ Five ADRs (0001 stack / 0002 color / 0003 licensing / 0004 AI / 0005 updates)
5. ✅ Core specs: ARCHITECTURE, COLOR_PIPELINE, FILM_MODEL, AI_MODELS, UX_SPEC, UX_DESIGN_TOKENS, RELEASING, PLAN (incl. traceability + risk register)
6. ✅ Twelve skills + index; memory protocol (STATE, CONTEXT_INDEX, session log); CLAUDE.md pointer; .editorconfig
7. ✅ Reference tooling: `scripts/fetch_refs.{sh,py}`; validation: `scripts/validate_docs.py`
8. ✅ CI skeleton (3-OS honest Phase-0 gates) + issue/PR templates
9. ✅ License decision recorded: **GPL-3.0-or-later** (owner, chat 2026-10-24; ADR-0003) — awaiting PR confirmation
10. ✅ Branch protection on `main` + `delete_branch_on_merge` (see session log for API outcomes)

## Active work

None — Phase 0 closed. Next work requires owner approval (M0) or revision requests on the PR.

## Blockers

- None technical. Governance blockers: (a) PR review, (b) M0 authorization, (c) GPL-3.0 confirmation click on PR, (d) remaining owner decisions in `docs/research/OPEN_QUESTIONS.md` (#3–#10 have safe defaults).

## Assumptions (conservative defaults in effect)

- GPL-3.0-or-later confirmed by owner in chat; treated as accepted pending PR confirmation.
- Tauri 2 provisional stack (ADR-0001) — egui fallback designed; Spike S1 decides at M0.
- EN+VI languages; generic stock names only; no OS code-signing until owner provisions; benchmark hardware class = integrated-GPU laptops (exact models pinned at M0).

## Risks (top — full register in PLAN §5)

1. Webview preview transport fails gates → fallback egui (designed, ADR-0001)
2. Film-model quality without calibration data → honest provenance labels + upgrade path
3. ort RC / ONNX packaging → CPU-first, AI optional
4. No macOS/Windows test devices → CI smoke + honest gap docs (OPEN_Q #8)
5. Solo scope creep → milestone gates + non-goals enforced in review

## Decisions awaiting owner

`docs/research/OPEN_QUESTIONS.md` — #2 (approve PR + authorize M0) is the critical path; #1 GPL confirmation; #3–#10 have defaults.

## CI status

Phase-0 `ci.yml` committed in this branch (docs validation on ubuntu + shell/python checks; 3-OS conditional matrix documented). **No CI run exists yet at time of writing** — first run triggers on PR creation; honest outcome recorded in session log afterwards.

## Next five atomic actions (for the next session, in order)

1. Owner reviews draft PR; confirm GPL-3.0-or-later (OPEN_Q #1) and approve/authorize M0 (OPEN_Q #2).
2. Address PR review comments on Phase-0 docs (small `docs:` commits on the same branch).
3. On M0 authorization: bootstrap Cargo workspace (`crates/` skeleton per ARCHITECTURE §2, `rust-toolchain.toml`, activate Rust CI gates).
4. Run Spike S1 (preview transport) — thresholds ADR-0001; record report in `docs/research/spikes/S1.md` + session log.
5. Run Spike S2 (lcms2 cross-platform + webview double-transform) — same reporting pattern; recalibrate PLAN §4 budgets from measurements.

## Environment notes for next agent

- Token: `MELOGRAPHY_GH_TOKEN` env (never in repo); remote `origin` uses credential helper reading a 600-perm file outside the tree.
- Local working copy path during Phase 0: `/home/z/my-project/melography` (agent sandbox; not part of the repository).
- `gh` CLI unavailable in Phase-0 environment — GitHub operations via `git` + REST API (`curl`).
