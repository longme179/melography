# MELOGRAPHY — Agent State

**Updated:** 2026-10-24T05:35Z (end of M1 session 03) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M1 (LUT workflow MVP)** — feature-complete on branch
`agent/m1-lut-workflow` (8 commits, local suites green). Issues #7/#8/#9
implemented; PR #18 (M0) merged by owner 2026-10-24T04:22Z. Next gate:
push + CI on 3 OSes + owner review.

## Branch / PR / HEAD

- Working branch: `agent/m1-lut-workflow` @ `606cdfa` (not yet pushed)
- M0 PR #18 merged (`9154d30` on main).
- Issue permissions: PAT now has them (verified — closed #3/#5/#6 with
  evidence comments; commented #7). Branch protection on `main`: still
  owner-side (PAT lacks Administration:write — unchanged).

## M1 delivered (all on the branch; details in session 03)

1. ✅ `.cube` parser (strict, line-attributed) + tetrahedral CPU + 1D
   shaper — commit `775df51`; 4 real bugs caught by its own test suite
2. ✅ Encoding contracts (fixed vocabulary; ACES verified) + intensity
   mix + insertion points + tiled≡monolithic
3. ✅ Independent Python oracle — 656 samples, worst |Δ| 1.19e-7 —
   `6fa5ab3`; CI regenerates + diffs the fixture
4. ✅ WGSL mirror + CPU↔GPU parity gate ≤1e-6 (honest-skip locally;
   CI macos/windows run it) — `99772e1`
5. ✅ Sidecar v1 + fingerprint policy + migration chain + byte pin —
   `baf494c`
6. ✅ LUT library (content-addressed, provenance, verify-at-load,
   explicit error states)
7. ✅ CLI `lut parse` / `apply` / `sidecar` + JPEG APP2 ICC — `54303c1`;
   ICC dateTime determinism bug found+fixed (lcms2 stamps "now")
8. ✅ Tauri 2 shell + Svelte UI (S2 MVP slice, EN/VI, error taxonomy →
   dot-codes) — `472d0db` + `606cdfa`; `docs/CODEC_MATRIX.md` created
   (M0 dangling refs); CI app-build job added

## Local verification (this machine)

- `cargo test --workspace`: **104/104** · clippy `-D warnings`: clean ·
  fmt: clean (both trees)
- app (gui, via rebuilt sysroot): check ✓ · tests **7/7** · clippy ✓
- UI: npm ci + tsc + vite build clean
- GPU parity: honest skip (no adapter) — CI runs the real gate

## CI status

Previous full run on main: GREEN 12/12 (M0, commit `1668e76`). The M1
push adds: oracle-freshness step (docs-validation) + `app-build` job
(ubuntu: webkit apt + npm + tsc + vite + cargo fmt/clippy/test +
cargo-deny on the app lockfile). **First run of the branch is the
authoritative 3-OS verification** — watch app-build (new) and the
macos/windows lut3d parity lines in logs.

## Active work

M1 branch complete; session 03 closed. Next session: watch CI on the
pushed branch, fix any 3-OS fallout, address owner review.

## Next five atomic actions (in order)

1. Push `agent/m1-lut-workflow`; open the M1 draft PR (body maps issues
   #7/#8/#9 → commits → verification evidence).
2. Verify the first CI run end-to-end (esp. app-build + oracle step +
   GPU parity on macos/windows); fix fallout with small `fix:` commits.
3. Owner reviews the PR; on approval+merge close #7/#8/#9 with evidence
   comments (leave #4 open — webview legs, see below).
4. M2 kickoff per PLAN (film chain stages A–I, tests-first on the CPU
   oracle — issues #10/#11/#12).
5. Owner-side standing items: reference-device measurements (PLAN §4
   provisional lines), S1 webview-IPC + S2 double-transform legs (issue
   #4) — need a live GUI run (xvfb harness or owner devices).

## Environment notes for next agent

- Token: `/home/z/my-project/.secrets/gh_token` (600; now WITH issue
  read/write — verified). REST via curl; `gh` CLI still absent.
- Sysroot: `~/melography-sysroot` (source `env.sh` for app cargo work);
  rebuilt this session (682 debs). **Background processes die between
  tool calls** — run long jobs in foreground chunks with resume.
- Disk is tight (9.9 GB): the tauri tree + workspace targets hit 100%
  once this session; clean `*/target`, incremental caches and deb
  archives aggressively; re-verify after every cleanup.
- PATH needs `~/.cargo/bin`; toolchain pinned 1.99.1; node/npm v24
  preinstalled (bun also present — npm is the CI source of truth).
