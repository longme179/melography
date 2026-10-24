# MELOGRAPHY — Agent State

**Updated:** 2026-10-24T05:35Z (end of M1 session 03) · **Maintainer:** the acting agent · **Rule:** ≤250 lines, no stale claims — every claim must be checkable against the repository/remote right now.

## Current milestone

**M2 (film lab core)** — complete on branch `agent/m2-film-core`,
draft PR open (issues #10/#11/#12). §L gates green (heavy gates run in
release via a CI step); 5 artist-authored stocks ship with schema
validation; fitting harness live (LM vs CMA-ES recorded); GPU pixel
kernel + parity gate shipped (spatial-stage kernels = documented M3/M4
follow-up); preview shortfall honestly documented with a measured cause
+ fast-path plan (docs/quality/M2_FILM_QUALITY_REPORT.md).
PR #19 (M1) merged by owner at 78c0249 (2026-10-24T06:16Z).

## Branch / PR / HEAD

- Working branch: `agent/m1-lut-workflow` @ `1509b0f` (pushed)
- **Draft PR #19:** https://github.com/longme179/melography/pull/19
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

**GREEN 13/13** on `1509b0f` (run 40068658895) — incl. the new
`app-build` job (first run), oracle-freshness + architecture-guard
steps, and the lut3d GPU parity gate on macos Metal (1.19e-7) and
windows WARP (bit-exact). Getting there took 3 fix iterations:
missing cargo-deny install in the app job → wrong flag position →
deny.toml needed MPL-2.0 allowlist + the tauri/wry global ban replaced
by the per-graph architecture guard (a lesson: an earlier CI edit had
silently no-op'd — the oracle step never landed until
assertion-verified re-insertion; all future workflow edits must assert
content).

## Active work

M1 branch complete; session 03 closed. Next session: watch CI on the
pushed branch, fix any 3-OS fallout, address owner review.

## Next five atomic actions (in order)

1. Owner reviews draft PR #19 (evidence-mapped body; 13/13 CI green).
2. Address review comments (small `fix:`/`docs:` commits, same branch).
3. On approval+merge: close #7/#8/#9 with evidence comments (leave #4
   open — webview legs); start M2 per PLAN (film chain stages A–I,
   tests-first on the CPU oracle — issues #10/#11/#12).
4. Owner-side standing items: reference-device measurements (PLAN §4
   provisional lines); S1 webview-IPC + S2 double-transform legs (issue
   #4) need a live GUI run (xvfb harness or owner devices).
5. Keep the sandbox sysroot alive (`~/melography-sysroot`) for M2 app
   work; watch disk (the tauri tree filled it once this session).

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
