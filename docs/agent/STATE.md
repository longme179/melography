# MELOGRAPHY — Agent State

**Updated:** 2026-10-25 (M4 session 10, #25 delivered pending review — M4 scope complete) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M4 (UX polish & hardening)** — **scope complete on branch
`agent/m4-ux-hardening`, all parts merged through PR #29 (#23 ✅ #24
core ✅ #15 ✅) except session 10 (#25) which awaits owner review.**
Sessions 06–09 delivered the session model, screens/panels, batch
queue, i18n/a11y (evidence: the M4 quality reports). **Session 10
(#25, this branch) — hardening: memory budget met (1265 MiB peak vs
2048 MiB @ 49.8 MP, exact VmHWM; two real full-res-buffer lifetime
bugs found & fixed); kill-during-export battery (anchored-kills SIGKILL,
mutation-validated 4/4, all encoders atomic); fuzz battery (4 targets,
`fuzz/` crate, CI bursts, 0 crashes, 156k decode execs locally); secret
scan + provenance gate (16 patterns × 335 files; 14 binary assets
sha-verified; 2 unprovenanced icons found); error-state inventory (38
codes × EN/VI × matrix, test-enforced; 2 untranslated codes found);
cache budgets enforced + hit-rate in the S2 status bar; cross-platform
build scripts (Ubuntu/Debian/Fedora/Arch/Windows/macOS + BUILDING.md).
Evidence: `docs/quality/M4_HARDENING_REPORT.md`. Next: M5 (owner-gated
— packaging matrix + gated release workflow).**

## Branch / PR / HEAD

- Working branch: `agent/m4-ux-hardening` (recreated from main @ 7df83ea)
- M4 sessions 06–09: merged (PRs #26, #27, #28, #29); #23 closed by owner
- M4 session 10 (#25, hardening + build scripts): delivered on this
  branch — PR to open (owner review required; closes #25 when merged)
- Issue permissions: PAT has them; branch protection on `main` =
  owner-side (unchanged).

## Session 10 delivered (#25 — details in session log 10 + M4_HARDENING_REPORT)

1. ✅ Memory budget: **1265 MiB peak vs 2048 MiB @ 49.8 MP** (exact
   VmHWM; release gate `crates/cli/tests/memory_budget.rs`, CI
   `memory-budget` 3 OSes) — two real full-res-buffer lifetime bugs
   found & fixed in `cmd_apply`/`cmd_film_apply` (pre-tone buffer
   survived its replacement; decoded/ap1/working/output-slab held to
   end-of-report)
2. ✅ Kill-during-export: all 4 stage-S5 encoders publish through
   `atomic_write_durable[_with]`; battery `crates/io/tests/export_kill.rs`
   (progress-pipe-anchored SIGKILL, 9 rounds × 3 formats, retry
   contract) — **mutation-validated 4/4** after the naive version
   failed sensitivity testing
3. ✅ Fuzz battery: `fuzz/` crate (4 targets — decode/cube/sidecar/
   manifest; nightly + libFuzzer; seeds committed + provenance-checked)
   + `scripts/gen_fuzz_corpus.py`; CI `fuzz-burst` 30 s/target; local
   validation 0 crashes (decode 156,751 execs / 46 s)
4. ✅ Secret scan + provenance: `scripts/secret_scan.py` (16 patterns,
   .gitignore policy executable, allowlist-with-reasons) +
   `scripts/provenance_check.py` (sibling + directory records,
   sha256-verified; 2 unprovenanced icons found & recorded)
5. ✅ Error-state inventory: `docs/quality/ERROR_STATE_INVENTORY.md`
   (38 codes × taxonomy × EN/VI × surface × next action) +
   `scripts/error_inventory.py` (3-way enforced; **2 untranslated codes
   found & fixed** — io.encode, lut.encoding_mismatch)
6. ✅ Cache budgets: `Settings.cache_budget_mb` enforced (preview slot
   full share, LUT LRU quarter share) + `cache_stats` command + S2
   status-bar hit-rate (EN/VI) + live budget updates; 7 new unit tests
7. ✅ Build scripts: `scripts/build/{build-linux.sh,build-windows.ps1,
   build-macos.sh}` (Ubuntu/Debian/Fedora/Arch detect; universal macOS;
   unsigned-by-design) + `docs/BUILDING.md` + README §3 rewrite +
   `bundle.active: true` (local bundling; no CI job runs tauri build)

## Local verification (this machine)

- Rust: core+io 19 tests ✓ (incl. both kill batteries) · cli 4 E2E ✓ +
  release budget run 1265/2048 MiB ✓ · workspace fmt/clippy ✓ · app
  headless 47+3 tests ✓ clippy 0 warnings ✓ (sandbox: no webkit —
  pkg-config shim for check-only, documented in session log)
- UI: `tsc` ✓ · `vite build` ✓ · `build:pseudo` ✓ · i18n audit PASS
  (327 keys, 28→38-code inventory consistent) · contrast audit PASS
- Gates: validate_docs ✓ · secret_scan ✓ · provenance ✓ ·
  error_inventory ✓ · fuzz bursts ×4 ✓ (0 crashes)
- Sandbox: fresh env (reset); Rust 1.99.1 + nightly reinstalled;
  node_modules via npm ci; disk 9.3G free
## Environment (reproducibility)

Sandbox resets between sessions — restore pattern: PAT →
`/home/z/my-project/.secrets/gh_token` (chmod 600); clone; `npm ci` in
`apps/melography-app/ui`. Toolchain notes from sessions 06–08 (lld
RUSTFLAGS etc.) apply only to Rust GUI builds — none needed this session.

## Next five atomic actions

1. Push the branch, open the session-10 PR (#25 — owner review
   required); after merge, close #25 with the report link
2. M5 (owner-gated): packaging matrix, updater beta roundtrip with
   owner-held keys, gated release workflow — the build scripts from
   session 10 are the local-build half of that story
3. Owner-device follow-ups queued for beta: live SR pass (NVDA/Orca/
   VoiceOver), EN/VI screenshots, GPU budget legs (OPEN_Q #8),
   first real run of build-windows.ps1 / build-macos.sh
4. Watch the first CI runs of the two new jobs (memory-budget,
   fuzz-burst) — Windows/macOS numbers land in CI logs, not the sandbox
5. Keep PLAN.md M4 progress + STATE.md re-pinned after each slice
