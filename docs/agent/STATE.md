# MELOGRAPHY — Agent State

**Updated:** 2026-10-24 (end of M0 session 02) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M0 (functional skeleton + risk spikes)** — in progress on branch
`agent/m0-skeleton`; Phase 0 was reviewed and merged (PR #17, commit
`ce5108c`). All four spike harnesses + the HEIF/AVIF probe now exist
with locally-verified evidence; push + draft PR for M0 is the next
gate.

## Branch / PR / HEAD

- Working branch: `agent/m0-skeleton` (pushed)
- **Draft PR #18:** https://github.com/longme179/melography/pull/18 (awaiting owner review; do not merge without approval)
- Phase 0 PR #17 merged into `main` (`ce5108c`).
- Branch protection on `main`: still NOT applied (PAT lacks
  Administration:write, 403) — owner action stands. Issue comments are
  also 403 (PAT lacks Issues:write) — agent communicates via PR bodies.

## Verified completed tasks (M0 so far)

1. ✅ Cargo workspace bootstrap (9 crates, rust-toolchain 1.99.1,
   deny.toml) + **S2 golden battery green** — commit `90f247e`
   (ΔE00 fast-path vs lcms2, roundtrip 31/65535 worst, ramp, dither ≤1
   LSB, cross-OS hash pins; CIEDE2000 Sharma vectors, 2 bugs found in
   our draft during cross-verification)
2. ✅ Rust CI gates ACTIVE: fmt+clippy+tests now run (workspace
   exists) — first 3-OS runs happen on the M0 push
3. ✅ Spike S1 harness (CPU legs measured; GPU leg honest-skip in
   sandbox; `docs/research/spikes/S1_REPORT.md`)
4. ✅ Spike S2: lcms2 golden half DONE; **webview double-transform leg
   OPEN** (needs live webview; M1 app shell / owner devices) — issue #4
   stays open
5. ✅ Spike S3 harness (cold 14.1 ms, warm 0.011 ms; IR-13 pitfall
   fixed; one-env-per-process redesign) — `S3_REPORT.md`
6. ✅ Spike S4 harness (5/5: roundtrip + 3 negatives + target portability;
   wire formats verified against plugin fixtures) — `S4_REPORT.md`;
   `install()` deferred to M5 by design
7. ✅ HEIF/AVIF probe: **GO for decode** (HEIC 3.3 ms / AVIF 1.0 ms via
   system libheif+libde265/dav1d; encode NO-GO by policy) —
   `HEIF_PROBE.md`
8. ✅ cargo-deny gate activated in CI + verified locally (first run
   fixed: CDLA-Permissive-2.0 for webpki-root-certs; wildcards=allow
   with rationale)
9. ✅ PLAN §4 first re-pin from M0 measurements (GPU/webview lines stay
   provisional, marked as such)

## CI status

**GREEN (12/12 jobs) on `1668e76`** — the first fully-activated run:
docs validation, tooling ×3, rust-gate, fmt+clippy, **cargo-deny**,
Rust tests ×3 OS (macos 'Apple Paravirtual device' + windows 'Microsoft
Basic Render Driver' adapters run the real S1 benchmark after the
two-buffer idiom fix they themselves forced), S4 updater (+ live
`/releases/latest` probe → 404 as expected with zero releases), HEIF
probe. `-- --nocapture` added to rust-tests so future runs print the
measurements into logs.

## Active work

None — M0 code+docs complete on the branch; awaiting owner review of
PR #18 (and the two labeled-pending legs: S1 webview/GPU-owner-device,
S2 webview — both tracked in the reports and issue #4).

## Next five atomic actions (in order)

1. Owner reviews draft PR #18 (GPL already confirmed in chat; PR
   merges with the branch as-is).
2. Address PR review comments (small `docs:`/`fix:` commits, same
   branch).
3. On approval+merge: close #2 #3 #5 #6 (leave #4 open — webview leg);
   start M1 per PLAN (editor shell MVP → S1/S2 webview legs ride it).
4. At M1: run the webview double-transform detection (xvfb CI + owner
   devices) and record it in S2_REPORT.md.
5. Keep PLAN §4 provisional lines under review as owner hardware gets
   declared.

## Environment notes for next agent

- Token: `MELOGRAPHY_GH_TOKEN` env or `/home/z/my-project/.secrets/gh_token`
  (600 perms, outside the repo); REST API via curl (`gh` CLI absent).
- Dev sandbox: 2 cores, ~10 GB disk (watch build dirs); no root (a
  sysroot under `scratch/sysroot` provides the webkit stack for
  tauri/linux builds — see `scripts/sysroot_webkit.sh` upstream copy in
  the repo? no: it lives at `/home/z/my-project/scripts/` outside the
  repo; CI uses apt instead).
- `PATH` needs `~/.cargo/bin`; `rust-toolchain.toml` pins 1.99.1;
  cargo-deny 0.20.2 installed locally.
- Spike standalone crates are EXCLUDED from the workspace; their
  `target/` dirs must never be committed (`.gitignore` covers
  `spikes/*/target/` after a 5.3 GB staging incident was amended away
  pre-push).
