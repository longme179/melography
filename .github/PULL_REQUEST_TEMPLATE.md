# Pull Request

**Type:** `docs:` / `feat:` / `fix:` / `chore:` / `ci:` / `test:` —
**Scope guard:** does this PR implement a product feature without milestone authorization? (Phase 0 = research/planning only; M0+ requires owner approval recorded in the referenced issue.)

## Summary

(what and why — 3–8 lines; link the issue/ADR this implements)

## Evidence

- [ ] Commands actually run (paste real output/summary lines, incl. failures & "not run" reasons)
- [ ] Validation: `python3 scripts/validate_docs.py` (Phase 0) / `cargo fmt --check && cargo clippy -- -D warnings && cargo test --workspace` (M0+)
- [ ] CI run URL on this branch with honest job states (skipped ≠ passed)
- [ ] Measurements attached where claims are quantitative (latency, ΔE00, memory) — no invented numbers

## Safety & licensing

- [ ] No secrets/tokens/keys in diff (self-scanned)
- [ ] Any third-party code: headers preserved + THIRD_PARTY_NOTICES + tools/provenance.json (ADR-0003)
- [ ] Any asset (LUT/image/profile/weights): individually license-cleared (ADR-0003 assets rule)
- [ ] No publish/release side effects (ADR-0005)

## Docs & memory

- [ ] Affected specs/ADRs updated (CONTEXT_INDEX still correct)
- [ ] docs/agent/STATE.md + session log updated (if session-scoped change)

## Reviewer checklist (maintainer)

- [ ] Claims match evidence; no fabricated completion
- [ ] Tests cover the changed contract (golden/property/fuzz as applicable)
- [ ] Phase gate respected (no unauthorized milestone work, no main merge without approval)
