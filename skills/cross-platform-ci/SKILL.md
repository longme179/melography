---
name: cross-platform-ci
description: Use when adding or modifying GitHub Actions workflows, runners, caches, or packaging steps. Enforces honest gates, pinned actions, least privilege, and never-green-by-lying CI.
---

# Cross-Platform CI

## Prerequisites
- Read `docs/RELEASING.md` §6 (security rules) and the existing `ci.yml` comments. ADR-0005 governs anything release-adjacent.

## Workflow
1. **Honesty rule first:** a job either runs real commands whose exit codes mean something, or it doesn't exist. Phase-conditional jobs use explicit `if:` conditions (e.g., Rust gates activate only when `Cargo.toml` exists) — never "pass" placeholders.
2. Actions pinned by full commit SHA with version comment (verified set in RELEASING §6; re-verify SHA provenance when bumping).
3. Permissions: workflow-level `permissions: read-all`; job-level minimal writes only where needed; `GITHUB_TOKEN` never escalated; no `pull_request_target` on untrusted input.
4. Runner matrix: ubuntu / windows / macos (macos-14+ arm64 and macos-13 x64 split when budget allows); GPU jobs only when a real GPU runner exists — otherwise CPU parity only, stated in the job summary.
5. Caching: keyed by lockfiles + toolchain; `concurrency` group with cancel-in-progress per ref; cache hygiene (no secrets in cache keys, bounded sizes).
6. Failure budget: every job produces an artifact or summary line with the numbers (test counts, parity ΔE, timings) — logs that only say "success" are a smell.
7. Release-adjacent jobs (packaging, updater dry-run) may build artifacts but NEVER publish; publish jobs live only in the M5 gated workflow behind the `release` Environment.
8. Test the workflow by pushing to an `agent/*` branch and reading the real run URL; record it in the PR.

## Verification checklist
- [ ] No new action without SHA pin + provenance note
- [ ] `if:` conditions match reality (Rust gates off until workspace exists, etc.)
- [ ] Matrix covers 3 OS; GPU absence explicitly stated where relevant
- [ ] Least-privilege permissions diff reviewed; no secrets in logs/artifacts
- [ ] Real run URL(s) attached to the PR with green/pink status honestly described

## Common pitfalls
- Workflows that go green while testing nothing (empty steps, always-true conditions).
- Tag-version pins (mutable) instead of SHA pins.
- Caching target/ across OSes with incompatible keys (wasted bandwidth, stale builds).
- Publishing side effects hidden in a "build" job (must not exist before M5 gate).

## Links
`.github/workflows/ci.yml` · `docs/RELEASING.md` §6 · ADR-0005 · skill `secure-release-and-update`

## Expected outputs
Workflow changes + live run evidence + updated RELEASING §6 if the rules evolve.
