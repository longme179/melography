# ADR-0005: Packaging, Update Security & Release Gating

**Status:** Accepted · **Date:** 2026-10-24
**Related:** `docs/RELEASING.md` (procedures), ADR-0001 (S4 spike)

## Context

Public GitHub repository, desktop app on 3 OSes, safe self-updates from GitHub Releases, no telemetry, owner-only release authority, agent must never hold production private keys, and Phase 0 must create skeletons only — no production release.

## Decisions

### 1. Channel model

- **stable** — GitHub Releases latest (non-prerelease). Canonical updater manifest endpoint: `https://github.com/longme179/melography/releases/latest/download/latest.json` (GitHub `/releases/latest` excludes prereleases — to be re-verified live in S4).
- **beta** — separate prerelease track with its own signed manifest (`latest-beta.json` attached to tagged prereleases), selected in Settings; never mixed into the stable endpoint.
- Channels are distinct artifacts + manifests; the updater refuses manifest channel mismatches (build-embedded channel ID must match manifest channel).

### 2. Updater technology & signatures

- **Tauri v2 updater plugin** (if Tauri is confirmed by S1/S4) producing signed artifacts (`latest.json` + platform signatures) — exact signing scheme (minisign-style keypair) verified against current plugin docs in S4; **do not rely on remembered details**.
- **Three signing layers, never conflated:** (a) *updater cryptographic signatures* — prove artifact integrity to installed clients; keypair generated **by the owner** at M5 time, private key only in owner-controlled GitHub Secrets/signing environment; (b) *OS code signing* — Windows Authenticode, macOS codesign — depends on owner budget (OPEN_QUESTIONS #5); (c) *macOS notarization* — Apple-specific, budget-gated. Absence of (b)/(c) degrades first-run trust prompts but never breaks updater integrity (a).
- Key rotation: public keys are embedded in builds; rotation = new keypair + new release with both signatures during transition window + owner runbook in RELEASING.md. Loss of private key = publish a superseding signed release with the new key through a bootstrap release (documented; owner-executed).

### 3. Anti-downgrade / anti-replay / integrity

- SemVer monotonicity: updater rejects lower versions (standard static manifests) — **rollback strategy = supersede** (publish x.y.z+1 reverting the bad change), never imaginary "downgrade support".
- Signatures cover manifest content (version + URLs + per-platform digests); HTTPS + GitHub-hosted origin; replay attempts (old manifest) fail signature/version checks.
- Interrupted updates: Tauri updater's staging semantics + our post-install self-check (verify SHA-256 of staged artifact before swap; abort cleanly on mismatch); AppImage keeps the previous version until swap completes.

### 4. Platform packaging matrix

| Target | Package | Self-update |
|---|---|---|
| Linux x86_64 | **AppImage (primary channel)** | ✅ built-in |
| Linux | .deb / .rpm | ❌ — visible notice + manual-download guidance (repo-based updates are the distro's business) |
| Windows x64 | NSIS installer (+ optional MSI later) | ✅ via updater (restarts app) |
| macOS arm64 + x86_64 | .app in .dmg (universal or per-arch) | ✅ via updater (restarts app) |

Packages without in-place updates must say so visibly in their release notes and in-app (master prompt §03).

### 5. Release gating (hard rules in CI)

- `.github/workflows/ci.yml` never publishes; a separate **gated release workflow** (created at M5, not before) runs only on version tags, requires the protected GitHub Environment `release` with **owner approval**, and has `contents: write` only in that job.
- Tags, releases, `latest.json` promotion, stable-manifest changes, public installers, any distribution action: **explicit owner authorization recorded beforehand** (issue/chat) — restated from AGENTS.md §2.
- Signing secrets live only in GitHub Secrets / the owner's signing environment; the agent never generates, receives, or sees production private keys. Public keys are distributable and committed.

### 6. Build & supply-chain security

- Workflow hardening: pinned action SHAs (verified in Phase 0), `permissions: read-all` default + job-scoped writes, least-privilege `GITHUB_TOKEN`, no `pull_request_target` on untrusted content, artifact retention limits, concurrency cancellation, path filters.
- Untrusted-input hardening tracked as implementation issues: image parsers (fuzz), LUT/loader validation + size caps, sidecar deserialization (deny-by-default unknown fields → preserved-but-inert), model-archive path-traversal checks, URL allowlist for model/update endpoints, secret scanning in CI.

## Consequences

- Users get safe, consent-based, verifiable updates; owner keeps sole release authority; agent can never ship anything alone.
- Linux distro packages are second-class by design (AppImage primary) — documented trade-off.
- All updater behavior claims remain **design** until S4's dummy-channel roundtrip produces live evidence (recorded in the spike report + session log).
