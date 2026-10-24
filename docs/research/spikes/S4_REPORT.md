# Spike S4 — Tauri v2 updater dummy-channel roundtrip (issue #6, ADR-0005)

**Status:** complete for check→download→verify mechanics with negative
paths; `install()` is **deliberately not exercised** (M5, real bundles,
gated release workflow, issue #16). Nothing unmeasured is claimed.
**Date:** 2026-10-24 · **Harness:** `spikes/s4-updater` (standalone
crate, excluded from the workspace) running the **real
`tauri-plugin-updater` 2.14.0** on a **mock runtime**
(`tauri` 2.12.5, `default-features = false, features = ["test"]` —
no webview/GTK at the plugin layer; the tauri *crate* still needs the
webkit stack at build time on Linux, covered in CI by apt).

## Wire formats (verified against the plugin's own test fixtures, not
memory)

- `pubkey` parameter: **base64 of the full `.pub` box** (comment line +
  key line). The plugin base64-decodes it before
  `minisign_verify::PublicKey::decode`.
- manifest `signature` field: **base64 of the full SignatureBox**
  (untrusted comment + sig base64 + trusted comment + global-sig
  base64).
- Static manifest shape: `{"version", "pub_date", "platforms":
  {"<target>-<arch>": {"signature", "url"}}}` with
  `target() == "linux-x86_64"`-style keys (portable across CI OSes —
  asserted by a dedicated test).

## Evidence (5/5 tests, `tests/roundtrip.rs`)

| Path | Result |
|---|---|
| positive roundtrip: check → download → verify | **PASS** — manifest fetched exactly once, bundle fetched exactly once, returned bytes == bundle (minisign verified **by the plugin during download**) |
| tampered bundle (valid signature over different bytes) | **rejected** at download |
| garbage signature field | **rejected** ("Invalid symbol 32, offset 4") |
| older version (0.0.9 < current 0.1.0) | **not announced**; zero bundle fetches (default comparator = anti-downgrade, ADR-0005 §3) |
| platform key == `target()` on this OS | PASS |

`main.rs` report (this sandbox, 2 cores, 128 KiB bundle):

```
check 27.1 ms · download+verify 28.1 ms · 1 manifest fetch · 1 bundle fetch
```

## Live `/releases/latest` semantics (ADR-0005 §1) — probed in CI

CI step `Live /releases/latest semantics probe` records the status
while the repo has **zero releases** (404 expected). The
prerelease-exclusion claim ("latest excludes prereleases") is
**re-verified at M5** with a real release and recorded then — this
document says so rather than asserting it now.

## Honest scope

- **`install()`** not exercised (needs real platform bundles; M5).
- **Channel-ID refusal** (ADR-0005 §1) is enforced by our *manifest
  generator* at M5 (the plugin has no channel concept) — covered by
  the M5 release workflow issue, not here.
- **`require_signed_version`** stays false in the test config; the M5
  manifest generator will turn it on and pin versions into the trusted
  comment — noted for the release checklist.
- Local build required a hand-rolled sysroot (no root in the dev
  sandbox); CI uses apt with the standard tauri linux dependency set.

## Reproduce

```
cd spikes/s4-updater && cargo test     # all evidence
cargo run                              # JSON summary of one roundtrip
```
