# MELOGRAPHY — M4 hardening report (issue #25)

**Session:** 10 · **Date:** 2026-10-25 · **Scope:** memory budget,
fuzz battery, secret scan, provenance tooling, error-state inventory,
kill-during-export, cross-platform build scripts · **Everything below
is a real measurement or a real test run from this session — no
projected numbers.**

## 1. Memory budget — ≤ 2 GiB @ 50 MP (PLAN §4)

**Gate:** `crates/cli/tests/memory_budget.rs` (`#[ignore]`d, release;
CI job `memory-budget` on ubuntu/windows/macos). It drives the exact
`cmd_film_apply` path the export dialog and S6 batch queue use:
synthetic **8640×5760 (49.8 MP)** PNG8 → decode → AP1 → tone stage ON →
full film chain (registry stock, tiled strips 512 — the app's shape) →
16-bit PNG + sidecar, all in one process (generation cost counts).

**Measured (dev sandbox: linux x86_64, 2 cores, Intel Xeon, release):**

| Line | Evidence | Status |
|---|---|---|
| Peak RSS end-to-end | **1265 MiB** vs 2048 MiB budget (VmHWM, exact) | **green — 783 MiB headroom (38%)** |
| e2e wall time | 36.3 s (decode+S1 2.8 s · film chain 22.3 s · encode 10.1 s) | informational (2-core worst case) |
| Per-phase RSS trace (probe run) | decode 731 MB → +AP1 1169 → tone 1169 (old buffer freed) → film 1295 peak → encode 885 | the drops below are what holds the peak |

**Two real bugs found by the budget work** (the gate exists to catch
exactly these; both fixed in `crates/cli/src/lib.rs`):

1. **A full-res f32 buffer survived its own replacement.** The tone leg
   matched on `&opts.tone` and conditionally moved `ap1` — in practice
   the pre-tone buffer stayed allocated through render+encode
   (+600 MB @ 50 MP). Phase probes showed RSS 1170 MB after tone where
   ~600 was expected. Fix: move `ap1` INTO the match so each arm owns
   it. Peak dropped 1835 → 1265 MiB.
2. **Everything stayed alive for one fingerprint.** `decoded` (a second
   full-res image), `file_bytes`, `ap1` and `working` were held until
   the report/sidecar at the end. Fix: extract the fingerprint,
   encoding and dims immediately after decode; drop `decoded` before
   tone; drop `ap1`/`working` as soon as their successor exists; hash
   the output file in 1 MiB chunks instead of slurping it (a 50 MP
   PNG16 is 100–300 MB of avoidable peak).

Honest per-OS notes: Linux asserts the exact `VmHWM`; Windows asserts
`PeakWorkingSetSize` (exact, via psapi FFI — no extra crate); macOS has
no portable high-water mark, so the gate samples `ps -o rss=` and
**prints that it is a lower bound** — never presented as a peak.

## 2. Kill-during-export battery (PLAN M4 exit criterion)

**Gate:** `crates/io/tests/export_kill.rs` — real SIGKILL
(TerminateProcess on Windows) battery on all 3 OSes (the `rust-tests`
matrix). All four stage-S5 encoders (PNG8/16, JPEG±ICC, TIFF16) now
publish through `atomic_write_durable[_with]` (temp sibling → fsync →
rename); a kill at ANY point leaves the destination **absent or
complete — never torn** — litter is cleaned by `clean_stale_tmp`, and a
retry to the same path succeeds.

**Method note (why this battery is shaped the way it is):** the naive
spawn+sleep+kill version was **rejected by mutation testing** — the
encoders' write phases are a few ms of a ~100 ms loop, random kills
missed them, and a direct-to-dest writer (the torn-file bug this exists
to catch) survived 5/5 runs. The battery anchors each kill to the write
phase with a progress pipe (the child emits one `W` byte immediately
before each encode; the parent kills 0.2–3 ms later). Validation:

| Check | Result |
|---|---|
| Atomic implementation, 6 consecutive runs | 6/6 pass (~13.5 s each, debug) |
| Mutation: TIFF encoder reverted to direct write | **4/4 runs catch the torn file** (round 1: "TORN export survived at the final path … ImageWidth not found") |

## 3. Fuzz battery (ARCHITECTURE §9)

**Crate:** `fuzz/` (standalone, nightly + libFuzzer, excluded from the
workspace). Four targets, CI job `fuzz-burst` (30 s each, crash
artifacts uploaded on failure, seeds copied to a scratch corpus so the
committed tree stays pristine): `decode_image` (the same S0
sanitization as `decode_file` — proven by a new unit test),
`cube_parser`, `sidecar_json` (parse + migrate + re-serialize), and
`manifest_json` (parse + vetting gate).

**Local validation bursts (this sandbox):** zero crashes on all four;
`decode_image` alone ran **156,751 executions in 46 s**. The decode
harness clips inputs claiming >8 MP (documented in-target: tiny deflate
payloads can claim gigapixel dimensions; the production bomb guard
>300 MP is still exercised for real, and the 8–300 MP band is covered
by the memory gate at 49.8 MP instead of the fuzzer).

## 4. Secret scan + provenance (the .gitignore policy executable)

* **`scripts/secret_scan.py`** — 16 high-signal credential patterns ×
  every tracked file + the mandatory `.gitignore` policy lines + "nothing
  tracked that the policy ignores" (`git ls-files --ignored --cached`).
  Allowlist entries carry reasons. **PASS: 335 files.**
* **`scripts/provenance_check.py`** — every tracked binary asset needs a
  sibling or directory provenance record whose `sha256` matches the
  committed bytes. Found and fixed real gaps: the two placeholder icons
  had no records (written, with honest M0/M1 origins); fuzz corpus seeds
  carry records generated by `scripts/gen_fuzz_corpus.py`.
  **PASS: 14 binary assets, all sha-verified.**

## 5. Error-state inventory (UX_SPEC §9 × ARCHITECTURE §8)

`docs/quality/ERROR_STATE_INVENTORY.md` — 38 codes × taxonomy class ×
EN/VI guidance × surface × next action. **Enforced** by
`scripts/error_inventory.py` (Rust `AppError` codes ⇄ both i18n catalogs
⇄ matrix rows, 3-way, CI): the gate **found 2 codes (`io.encode`,
`lut.encoding_mismatch`) that could reach the UI without localization** —
fixed (catalogs now 327 keys). Raw-string leakage is separately enforced
by the existing `ui_i18n_audit.mjs` embedded-string scan.

## 6. Cache budgets (configurable + enforced + surfaced)

`Settings.cache_budget_mb` went from advisory to enforced
(`apps/melography-app/src/cache.rs`): the full-res preview slot takes the
whole budget (an image larger than the budget renders but is **not
retained** — honest degradation, shown as "cache off" in the viewport),
the LUT resolution cache is now an LRU with a quarter share (floor
64 MiB; a single oversize LUT is served but not retained). Hit-rates are
counted and surfaced: `cache_stats` command + the S2 status bar shows
the preview hit-rate next to render-ms (`cache 87%`, localized EN/VI).
Budget changes apply live (Settings → both caches, evicting as needed).
7 new unit tests (LRU eviction/recency/oversize/budget-change/remove/
clear + preview-budget enforcement); app suite 47+3 green.

## 7. Cross-platform build scripts

`scripts/build/` + `docs/BUILDING.md` (README §3 rewritten to point at
them): `build-linux.sh` (Ubuntu/Debian apt, Fedora dnf, Arch pacman —
auto-detected; deps optional via `--no-install`; deb+rpm+AppImage or
`--bin-only`), `build-windows.ps1` (VS/MSVC + WebView2 probes; NSIS+MSI),
`build-macos.sh` (native/aarch64/x64/universal; .app+.dmg). All produce
**unsigned local builds with checksums** — signing/notarization/publish
stay owner-gated at M5 (RELEASING.md); `bundle.active` enabled in
`tauri.conf.json` for local bundling (CI jobs unaffected — none run
`tauri build`). `bash -n` clean; the scripts join the CI shell-syntax
gate.

## 8. Verification summary (this session, this machine)

| Gate | Result |
|---|---|
| `cargo test -p melography-core -p melography-io` | 19 tests green (incl. both kill batteries) |
| `cargo test -p melography-cli` (+ release budget run) | 4 E2E green · budget 1265/2048 MiB |
| `cargo test` app crate (headless) + clippy + fmt | 47+3 green, 0 warnings |
| UI: tsc + vite build + pseudo build + i18n audit + contrast audit | all PASS (327 keys, 0 embedded strings) |
| Fuzz bursts ×4 | 0 crashes (decode 156k execs) |
| secret_scan / provenance_check / error_inventory / validate_docs | all PASS |
| Mutation tests (the gates prove they catch the bugs) | export battery 4/4; budget gate caught the tone-leg buffer bug in situ |

## 9. Honest gaps (known, documented, not hidden)

* The 8–300 MP decode band is covered by the budget gate, not the
  fuzzer (rationale in `fuzz/fuzz_targets/decode_image.rs`).
* macOS memory measurement is a sampled lower bound (no portable
  VmHWM); printed as such.
* `ExportError::UnsupportedCombination` remains structurally prevented
  at the UI layer rather than a code (inventory §"Honest gaps").
* Fuzz bursts are 30 s/target in CI — regression-catching, not
  deep-fuzzing; longer campaigns remain a manual `cargo fuzz run`.
* Build scripts are verified for syntax and logic on Linux
  (this sandbox); the Windows/macOS script bodies will get their first
  real exercise on owner machines or the M5 release workflow — noted in
  BUILDING.md.
