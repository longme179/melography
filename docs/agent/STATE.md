# MELOGRAPHY — Agent State

**Updated:** 2026-10-24T09:30Z (end of M3 session 05) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M3 (AI assistance)** — complete on branch `agent/m3-ai-assist`,
draft PR open (issues #13/#14). Tone stage + non-ML baseline + model
manager + manifest gate v2 + ONNX eval harness shipped; baseline-only v1
(no model passed the gate — ADR-0004 §7 honest outcome); benchmark
0.205 vs naive 1.019 CI-enforced as the bar; budgets 49–61× under.
Also on the branch: the owner-requested README build/deploy guide
(PR #21 merged first — check its state before merging this branch).

## Branch / PR / HEAD

- Working branch: `agent/m3-ai-assist` (based on `agent/readme-build-docs`)
- PR #21 (README build/deploy + LICENSE): open/merged — **verify before M3 merge**
- M3 PR: open on push (see session log 05)
- Issue permissions: PAT has them; branch protection on `main` =
  owner-side (unchanged).

## M3 delivered (all on the branch; details in session 05 + M3_AI_REPORT)

1. ✅ Tone operator (`crates/color/tone.rs`): WB cone gains (Bradford,
   round-trip exact), EV, HL/SH masks, percentile shaping (post-EV
   knots); 17 tests; NaN-proof sanitize
2. ✅ Sidecar `StageEntry::Tone` + `SuggestionSource` provenance
3. ✅ Non-ML baseline (`crates/ai/auto_light.rs`): AI_MODELS §2; 13 tests
4. ✅ Manifest v2 + strict gate (12 rejection tests); `ai vet` CLI
5. ✅ Model manager (§6 states, sha256, atomic install, crash recovery);
   `ai install/models/delete` CLI; offline install path
6. ✅ ONNX runner + eval harness (feature `onnx`, ort rc.15); dummy
   fixture (CC0, project-generated); benchmark = the CI bar
7. ✅ CLI tone flags on apply/film apply; `ai suggest`
8. ✅ App: TonePanel (Accept/Adjust/Dismiss + provenance), ModelManager,
   EN/VI; export applies tone (preview ≡ export)
9. ✅ CI `ai-onnx` job (clippy+tests+benchmark+release budget)
10. ✅ docs: M3_AI_REPORT, AI_MODELS §3/§6 updates, PLAN §M3 re-pin

## Local verification (this machine)

- Workspace: `cargo test --workspace` **36/36 suites green**; fmt+clippy
  `-D warnings` clean (default + `--features onnx`)
- onnx: 36 lib + 6 eval tests green; release budget test green
  (60.6 ms / 49.3 ms @ 12 MP)
- App (gui features!): check ✓ · tests **8/8** · clippy ✓ · fmt ✓ — via a
  downloaded-.deb sysroot (PKG_CONFIG_PATH + LIBRARY_PATH +
  LD_LIBRARY_PATH; see session 05 "sandbox sysroot" note) — the sandbox
  has no root, so GTK/WebKit dev+runtime libs were extracted from apt
  downloads into /tmp/sysroot
- UI: npm ci + tsc + vite build clean
- E2E: chart → ai suggest (honest diagnosis) → film apply --tone-ev
  0.5 --tone-shadows 40 → sidecar stages [film_stock, tone] ✓;
  ai vet/install/models/delete round-trip ✓

## CI status

Expect `ai-onnx` first run on the PR (new job); all other jobs unchanged
since M2 green 13/13. The app-build job re-verifies the app tree
authoritatively.

## Next five atomic actions

1. Push `agent/m3-ai-assist`, open the draft PR (owner review required)
2. Close issues #13/#14 with evidence comments **after merge**
3. M4 planning: UX_SPEC panel set vs what exists (gap list)
4. GPU spatial kernels (M2 follow-up) — decide M4 slot
5. Owner: review PR #21 (README) if not yet merged
