---
name: session-handoff
description: Use at the START and END of every MELOGRAPHY working session to verify reality, resume authorized work, and leave verifiable state for the next agent. Prevents stale-memory continuation and fabricated-progress claims.
---

# Session Handoff

## Prerequisites
- Read access to the repository; `MELOGRAPHY_GH_TOKEN` in env (or authorized connection) when pushing.
- Read `AGENTS.md` first (it links here).

## Workflow — session START
1. `git fetch origin --prune`; inspect `git status`, current branch vs `origin/main`, open PRs/issues (`gh api` or web).
2. Read in order: `docs/agent/STATE.md` → `docs/agent/CONTEXT_INDEX.md` → latest `docs/agent/sessions/*.md` → relevant ADR/skill.
3. **Trust but verify:** if STATE claims work the repo does not show (missing files/commits/CI), treat STATE as stale — record the discrepancy and reconcile before continuing.
4. State session scope to the owner **in Vietnamese**; create/resume branch `agent/<topic>` only if authorized (STATE or owner).
5. Check toolchain reality: `git --version`, `python3 --version`, (M0+) `rustc --version` vs `rust-toolchain.toml`.

## Workflow — session END / checkpoint
1. Run available checks: `python3 scripts/validate_docs.py` (Phase 0); M0+ adds `cargo fmt --check && cargo clippy -- -D warnings && cargo test --workspace`.
2. Review `git diff --stat`; whitespace check `git diff --check`; secret self-scan (AGENTS §3).
3. Update `docs/agent/STATE.md` (branch, PR, head SHA, next five atomic actions, UTC timestamp).
4. Append session log `docs/agent/sessions/YYYY-MM-DD-NN.md` (append-only; increment NN per session per day).
5. Commit (small, conventional) and push; record pushed SHA + test evidence in the log.
6. Vietnamese recap to owner: what is done, what failed, what is next. **Never claim unexecuted work.**

## Verification checklist
- [ ] STATE.md matches reality (branch/SHA/PR verified against remote)
- [ ] Session log written with real command outcomes (including "not run" + why)
- [ ] No secrets in diff; working tree clean after commit
- [ ] Next-five-actions are atomic and unambiguous

## Common pitfalls
- Continuing a stale STATE ("PR open" when it was merged/closed) — always re-check remote.
- Writing "tests passed" without a transcript — paste the actual summary line.
- Updating STATE but forgetting the session log (or vice versa) — both are required.

## Links
`AGENTS.md` §7–§8 · `docs/agent/STATE.md` · `docs/research/OPEN_QUESTIONS.md`

## Expected outputs
Updated STATE.md, appended session log, pushed commits with recorded SHAs, Vietnamese recap.
