---
name: architecture-decisions
description: Use when a technical choice affects MELOGRAPHY's structure, dependencies, performance contracts, or security posture and is not reversible in a single commit. Produces or updates an ADR.
---

# Architecture Decisions (ADRs)

## Prerequisites
- Evidence gathered BEFORE deciding: verified versions/licenses (`docs/research/REFERENCES.md`), measured spikes where feasible, alternatives actually enumerated.
- Read existing ADRs (never contradict an accepted ADR silently — supersede it).

## Workflow
1. Frame the decision as one question with measurable success criteria.
2. Enumerate ≥2 real options (a decision without an alternative is an assumption).
3. Score with declared weights; tag every cell **[E] evidence / [S] spike / [A] assumption** — never let [A] cells carry the decision alone.
4. Decide, or explicitly mark "provisional, gated by <spike/threshold>".
5. Write `docs/adr/NNNN-slug.md`: Context / Options / Decision / Consequences / Review triggers.
6. Update `docs/agent/CONTEXT_INDEX.md` mapping and any spec that references the old state.
7. Commit `docs(adr): NNNN …`; if it overturns an accepted ADR, mark the old one `Superseded by NNNN` in its header (never delete).

## Verification checklist
- [ ] Every load-bearing claim has an [E]/[S] tag or is explicitly [A] with a spike plan
- [ ] Review trigger states the measurable condition to reopen the decision
- [ ] Consequences include at least one honest negative
- [ ] Cross-referenced docs updated (PLAN traceability row if requirement-level)

## Common pitfalls
- Fashion-driven choices ("newest framework") without a fallback path.
- Hiding uncertainty: if a spike hasn't run, the ADR says so.
- Deciding without the owner when the choice is in AGENTS §10 (ask-first list: license, signing budget, third-party code, public promises).

## Links
`docs/adr/` (all) · `docs/ARCHITECTURE.md` · ADR-0001 (worked example with scoring matrix)

## Expected outputs
A merged ADR file + updated CONTEXT_INDEX + (if provisional) a spike issue with thresholds.
