# MELOGRAPHY — Releasing, Signing & Update Delivery

**Version:** Phase-0 procedures (no release exists; nothing here has executed) · **Authority:** ADR-0005
**Golden rule:** the agent builds and documents; **only the owner releases.** Any release job requires the protected `release` GitHub Environment with owner approval.

## 1. Version & channel model

- SemVer `MAJOR.MINOR.PATCH` (+ `-beta.N` prerelease suffix on the beta channel), app version sourced from a single `version.json`/tauri.conf source of truth (defined at M0).
- **stable**: latest non-prerelease GitHub Release; manifest URL `https://github.com/longme179/melography/releases/latest/download/latest.json`.
- **beta**: tagged `vX.Y.Z-beta.N` prereleases carrying `latest-beta.json`; opt-in in Settings.
- Channel ID is embedded in every build; the updater refuses a manifest whose channel ≠ build channel (no cross-channel drift, no beta→stable accidents via manifest swap).

## 2. Artifact matrix (per release)

| OS | Artifact | Updater-eligible | OS signing |
|---|---|---|---|
| Linux x86_64 | `MELOGRAPHY_<v>_amd64.AppImage` (+ `.sig`) | ✅ self-contained | — (updater sig only) |
| Linux | `.deb`, `.rpm` | ❌ (visible notice + manual guidance) | — |
| Windows x64 | `MELOGRAPHY_<v>_x64-setup.exe` (+ `.sig`) | ✅ | Authenticode if owner provisions cert |
| macOS | `MELOGRAPHY_<v>_aarch64.dmg`, `_x64.dmg` (+ `.sig`) | ✅ | codesign + notarization if owner provisions |

`latest.json` (stable) / `latest-beta.json` (beta) enumerate platforms, URLs, signatures, and the release notes digest; manifest schema validated by a CI check before any publish step (schema defined at M5 with the real updater; do not hand-write manifests).

## 3. The three signature layers (never conflated)

1. **Updater signatures** (Tauri v2 updater scheme — verify exact current key format at Spike S4): prove artifact integrity to installed clients. Keypair generated **by the owner**; private key only in GitHub Secrets / owner signing env; public key embedded in builds.
2. **OS code signing** (Windows Authenticode / macOS Developer ID): OS trust prompts. Budget-gated (OPEN_QUESTIONS #5); absence never breaks layer 1.
3. **macOS notarization**: Apple's scan; requires Apple Developer account (owner-gated).

## 4. Release procedure (owner-executed gates in **bold**)

1. Milestone exit criteria green in CI (PLAN.md); CHANGELOG entry reviewed.
2. **Owner approves** "release candidate" issue → release branch cut.
3. CI release workflow (created M5, gated): builds matrix → runs full test suite on artifacts → attaches to a **draft** GitHub Release.
4. **Owner verifies** draft release (install smoke on own machines) → **owner publishes**.
5. Stable channel manifest promotion happens automatically *only* for non-prerelease publishes; beta manifest attaches to its prerelease only.
6. Post-release: watch the next auto-check cycle health (update check is opt-in; no telemetry — health = issue tracker only).

## 5. Failure & rollback strategy (realistic)

- **Bad stable release:** publish superseding `X.Y.Z+1` that reverts the change (static SemVer updaters reject downgrades — "rollback" = supersede, documented honestly in-app).
- **Interrupted update:** staged-install semantics — verify artifact SHA-256 before swap; on mismatch abort and keep current version; user re-triggers.
- **Compromised/lost signing key:** owner rotates (new keypair → GitHub Secrets), publishes transition release signed with **both** keys (updater accepts either during window), then old key retired. Runbook here is executed by the owner; agent provides tooling and verification steps only.
- **Downgrade attack / replay:** version monotonicity + signature coverage of manifest content + HTTPS GitHub origin.

## 6. CI security rules (enforced from Phase 0)

- `permissions: read-all` default; job-scoped minimal writes; **no** release/publish permissions outside the gated `release` job.
- Actions pinned by commit SHA (verified 2026-10-24: `checkout@3d3c42e5aac5ba805825da76410c181273ba90b1` v7.0.1, `setup-python@6d4fd22b20465ccc0f7354476588e8df7c9ca0e0` v7.1.0, `upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9` v7.0.2, `cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9` v6.1.0).
- No `pull_request_target`; no workflow that publishes on push; secrets only in Environments; artifact retention 14 days (non-release); concurrency cancel-on-update.
- Supply chain: `cargo-deny` (licenses/advisories/bans) once Rust exists; lockfiles committed; `THIRD_PARTY_NOTICES.md` generated and diffed in CI.

## 7. What is explicitly NOT promised

No auto-update without consent (checks are opt-in; installs always ask) · no silent channel switching · no telemetry/analytics in any artifact · no update from anywhere but this repository's Releases · Linux distro packages do not self-update (stated in-app) · no byte-for-byte metadata guarantees on export.
