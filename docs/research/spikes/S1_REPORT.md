# Spike S1 — preview transport budget (issue #3, ADR-0001)

**Status:** completed for the headlessly-measurable legs; GPU-readback and
webview-IPC legs are **explicitly pending** (CI macos Metal / owner
devices / M1 app shell) — nothing unmeasured is claimed as proven.
**Date:** 2026-10-24 · **Harness:** `spikes/s1-transport` (workspace
member) + `crates/gpu` `wgpu` feature (wgpu 30.0.1, pollster).

## What was measured, where, and how

| Leg | Environment | Result |
|---|---|---|
| adapter enumeration | this sandbox (linux x86_64, 2 cores, no GPU) | **0 adapters — honest skip** (the spike reports it, never fabricates; CI assertions enforce exactly that semantics) |
| staging readback (compute → copy → map) | **CI runners with real adapters** — first CI run exercised it and **caught a real wgpu usage bug** (see below) | **runs and validates green on macos ('Apple Paravirtual device', Metal) and windows ('Microsoft Basic Render Driver', WARP)**; ubuntu runners and the sandbox have no adapter (honest skip) |
| CPU frame encode: raw / PNG / WebP-lossless @1080p/1440p | same | measured (below) |
| payload sizes | same | measured (below) |
| webview IPC hop + canvas blit | — | **not measurable headless** — deferred to the M1 app shell; the budget math below uses only measured legs |

### The bug the CI adapters caught (evidence, not luck)

The first CI run of the activated gates rejected our original buffer
usage (`MAP_READ | COPY_SRC | STORAGE` on one buffer) with wgpu's
validation: *"MAP usage can only be combined with the opposite COPY"*.
No-adapter environments (sandbox, ubuntu runners) can never catch that
class. The harness now uses the standard **two-buffer idiom** — compute
writes a `STORAGE|COPY_SRC` buffer, the encoder copies it into a
`COPY_DST|MAP_READ` buffer, and only the latter is mapped
(`crates/gpu/src/readback.rs`, commit `1668e76`). The passing CI run
on macos/windows is the validation; per-run numeric readback
throughput is printed into CI logs via `-- --nocapture` from the next
run onward.

## Measured numbers (deterministic synthetic RGBA8 gradient; median of runs)

| Frame | raw memcpy | PNG encode | WebP-lossless encode | raw bytes | PNG bytes | WebP bytes |
|---|---|---|---|---|---|---|
| 1920×1080 | 2.04 ms | 12.25 ms | 6.42 ms | 8,294,400 | 2,336,011 | 15,848 |
| 2560×1440 | 2.57 ms | 21.11 ms | 10.06 ms | 14,745,600 | 4,152,361 | 27,948 |

**Payload-size caveat (important):** the synthetic pattern compresses
extraordinarily well; a real photo preview at 1080p is typically
0.8–3 MB as WebP-lossless. The **encode cost** column is the meaningful
result; the payload column is the floor, not a prediction for photos.

## Budget math (measured legs only)

p50 interaction budget (PLAN §4) = 50 ms. Even the worst measured CPU
leg (PNG 1440p, 21 ms) + raw transport leaves > 25 ms of headroom for
the render itself at 1440p; at 1080p the CPU side totals ≤ 19 ms
worst case, ~8.5 ms typical (WebP). Bandwidth feasibility is not the
question (a 0.5 GiB/s path moves > 3 raw 1080p frames per 50 ms —
asserted in `tests/adapter_probe.rs`, which caught and fixed its own
GiB/MiB unit bug during development).

## What still gates ADR-0001 (honest)

1. **GPU→CPU readback throughput on target-class hardware** (integrated
   GPUs). CI macos-latest runs the real benchmark via
   `tests/adapter_probe.rs`; linux/windows runners may expose software
   adapters — either way the harness reports what actually happened.
2. **Webview IPC + blit cost** — needs the M1 app shell (S4 harness
   note in S4_REPORT.md); thresholds to check: composed preview p50
   ≤ 50 ms / p95 ≤ 120 ms (PLAN §4) with colorimetry intact (S2 webview
   leg).
3. ADR-0001 stays **provisional** until both legs are in; the egui
   fallback design remains ready (ADR-0001 §fallback).

## Reproduce

```
cargo run -p s1-transport-spike --release   # JSON report
cargo test -p s1-transport-spike            # CI-enforced form
```
