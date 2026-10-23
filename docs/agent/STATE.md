# MELOGRAPHY — Agent State

**Updated:** 2026-10-24 (end of M0 session 02) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M0 (functional skeleton + risk spikes)** — in progress on branch
`agent/m0-skeleton`; Phase 0 was reviewed and merged (PR #17, commit
`ce5108c`). All four spike harnesses + the HEIF/AVIF probe now exist
with locally-verified evidence; push + draft PR for M0 is the next
gate.

## Branch / PR / HEAD

- Working branch: `agent/m0-skeleton` (local; NOT pushed yet)
- HEAD: see `git log` — last commits: workspace+golden battery
  (`90f247e`), S1+S3 harnesses (`c49da1c`), S4+HEIF probe (`6699d97`),
  CI+deny (`b0fb191`), plus docs/memory commit(s) at session close.
- Phase 0 PR #17 merged into `main` (`ce5108c`).
- Branch protection on `main`: still NOT applied (PAT lacks
  Administration:write, 403) — owner action stands.

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

## Active work

M0 close-out: commit docs/memory → push branch → draft PR (links
issues #2 #3 #5 #6; #4 partially) → observe first real 3-OS CI run of
the activated gates → append CI evidence to spike reports (S3 per-OS
numbers, S1 macos adapter results if any) → request owner review.

## Blockers

- None technical. CI is the first run of the new jobs (cargo-deny,
  spike-s4, spike-heif) — failures are expected to be fix-forward on
  this same branch.

## Assumptions (conservative defaults in effect)

- GPL-3.0-or-later confirmed (owner, PR #17 merged with it).
- Tauri 2 provisional (ADR-0001) — S1/S4 mechanics PASS so far; the
  webview legs decide; egui fallback armed.
- Reference hardware: CI runners + dev sandbox declared in PLAN §4;
  owner's integrated-GPU laptops still to be declared.

## Risks (top — full register PLAN §5)

1. Webview preview transport unmeasured (S1 leg 2) → egui fallback
   designed (ADR-0001); decision at M1 app shell, not later.
2. GPU readback numbers absent until CI macos / owner devices.
3. First CI run of 3 new jobs may need fix-forward iterations.
4. Disk pressure in the dev sandbox (9.9 GB total) — build dirs are
   the hog; keep `spikes/*/target` out of git (done) and clean between
   heavy builds.

## Decisions awaiting owner

- OPEN_QUESTIONS #3–#10 defaults remain in effect; #2 (M0 PR review)
  becomes actionable the moment the draft PR opens.

## Next five atomic actions (in order)

1. Commit docs/memory updates (this file + session log 02 + PLAN
   re-pin + spike reports).
2. Push `agent/m0-skeleton`; open **draft PR** → main (title marks
   M0; body links #2 #3 #5 #6, notes #4 half-done).
3. Watch the first CI run; fix-forward on the same branch; append
   CI-measured evidence to S1/S3 reports.
4. Comment on issue #4 with the webview-leg plan (xvfb at M1 shell,
   owner devices) keeping it open.
5. Hand off to owner review (Vietnamese summary + PR link).

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
