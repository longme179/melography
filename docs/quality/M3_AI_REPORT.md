# M3 AI Quality Report — auto-lighting baseline + model manager + ONNX vetting gate

**Milestone:** M3 (issues #13, #14) · **Date:** 2026-10-24 (session 05) ·
**Reproduce:** `cargo test -p melography-ai` (fast) ·
`cargo test -p melography-ai --features onnx -- --nocapture` (the benchmark) ·
`cargo test --release -p melography-ai --features onnx --test onnx_eval -- --ignored` (budgets) ·
`melography ai suggest --input <photo>` (live) · CI job `ai-onnx` re-runs all of it on every push.

## 1. What shipped

| Piece | Where | Notes |
|---|---|---|
| Tone stage (the landing zone) | `crates/color/src/tone.rs` | WB cone gains (Bradford, round-trip exact), EV, highlights/shadows (masked ±0.75 stops + floor lift), percentile black/white shaping (post-EV knots, mid-gray pinned). 17 tests. |
| Sidecar integration | `crates/presets` `StageEntry::Tone` + `SuggestionSource` | Additive variant; provenance survives save/load; schema stays v1. |
| Non-ML auto-lighting baseline | `crates/ai/src/auto_light.rs` | AI_MODELS §2 exactly: percentile EV + p99.5 guard, robust gray-world WB + skin-tone guard, post-EV shaping knots. Deterministic single-pass histograms. |
| Model manager | `crates/ai/src/manager.rs` | §6 state machine, SHA-256 verify, atomic install (tmp→sync→rename, manifest.json commit marker), crash-litter cleanup, delete-removes-all. |
| Manifest v2 + gate | `crates/ai/src/lib.rs` | Full ADR-0004 §4 shape; rejection rules accumulate (budget 50 MiB, ambiguous weights, HTTPS allowlist, opset ≤ 17, CPU EP, measurements, v1 = parameter predictors). |
| ONNX runner + eval harness | `crates/ai/src/onnx.rs` + `tests/onnx_eval.rs` (feature `onnx`) | gate → sha → session-from-bytes; output decodes to clamped editable `ToneParams`. Fixture: project-generated naive gray-world predictor (921 B, CC0). |
| CLI | `melography ai suggest / vet / install / models / delete` + `--tone-*` flags on `apply`/`film apply` | Headless, CI-verbatim paths. |
| App UX | TonePanel (Accept/Adjust/Dismiss + provenance), ModelManager (states, offline install, delete), EN/VI | AI_MODELS §7 contract. |

## 2. The benchmark (issue #14's bar)

Synthetic corpus with ground truth (EV shifts, green casts, clipped, crushed,
mixed); metrics after applying each suggestion as `ToneParams`:
`ev_error` = |log2(patch luma/0.18)|, `wb_error` = patch channel spread/0.18,
`clip_rate` = fraction pushed above 1.0. Aggregate = `ev + 5·wb + clip`
(lower is better).

```text
scene           baseline(ev/wb/clip)         dummy(ev/wb/boosted clip)
ev-2            0.000 / 0.000 / 0.000        1.267 / 0.000 / 0.000
ev-1            0.000 / 0.000 / 0.000        0.754 / 0.000 / 0.000
ev-0.5          0.000 / 0.000 / 0.000        0.489 / 0.000 / 0.000
ev+0.5          0.000 / 0.000 / 0.000        0.068 / 0.000 / 0.000
ev+1            0.000 / 0.000 / 0.000        0.393 / 0.000 / 0.079
ev+2            0.000 / 0.000 / 0.000        1.058 / 0.000 / 0.199
cast0.15        0.105 / 0.000 / 0.000        0.091 / 0.139 / 0.000
cast0.3         0.265 / 0.102 / 0.000        0.037 / 0.295 / 0.003
cast0.5         0.458 / 0.093 / 0.000        0.219 / 0.533 / 0.023
clipped         0.000 / 0.000 / 0.000        0.709 / 0.000 / 0.149
crushed         0.562 / 0.000 / 0.000        0.963 / 0.000 / 0.000
mixed           0.089 / 0.000 / 0.000        0.367 / 0.104 / 0.000
aggregate (ev + 5·wb + clip): baseline 0.2046 vs dummy 1.0194
```

Reading it honestly:

* The baseline wins the aggregate **5.0×** and never clips a pixel on any
  scene (the dummy overshoots into 19.9 % clipping at ev+2 — it has no
  guard); on every pure-EV scene the baseline lands the neutral patch
  exactly (0.000).
* The dummy is *closer on raw `ev_error` for cast scenes* — because it
  "corrects" WB less (0.13–0.53 wb residual vs the baseline's 0.09–0.10)
  and the EV metric rewards not touching the patch. The aggregate weights
  that trade honestly: neutralization quality dominates.
* The baseline's worst line is `crushed` (0.562): the suggested shadow
  lift deliberately raises the midtone-shadows — the metric counts it,
  the photographer asked for it. Documented, not hidden.

**Verdict:** no real candidate was evaluated this milestone because none
passed the manifest gate (§3 below) — the naive dummy is the plumbing
proof. Per ADR-0004 §7: *"If no model passes the gates in M3, v1 ships
with the non-ML baseline only — acceptable outcome, not a failure."*
Any future model must beat **0.2046** on this job (CI-asserted) **and**
pass the gate **and** fit the budgets.

## 3. Candidate vetting status (gate outcomes, 2026-10-24)

| Family | Weights license | Gate verdict |
|---|---|---|
| Image-Adaptive-3DLUT | Apache-2.0 **code**; weights **unverified** | **Blocked** — weights license not verified (rule: ambiguous = rejected) |
| Zero-DCE family | repo has **no license** | **Rejected** (ADR-0003 exclusion) |
| SCI | canonical repo not located | **Deferred** |
| MobileSAM / U²-Net | Apache-2.0 code; weights **unverified** | **Blocked** (also: v1 gate = parameter predictors only) |
| Depth-Anything-V2 small | weights **unverified** | **Deferred** |

The gate itself is demonstrably strict: 12 rejection tests including
accumulated-rejection output, research-only weights, oversize (50 MiB),
non-HTTPS and off-allowlist sources, opset 21, missing measurements.
`melography ai vet --manifest <file>` exposes it as a workflow.

## 4. Budgets (ADR-0004 §5) — measured

| Line | Budget | Measured (dev sandbox, linux x86_64, release) | Status |
|---|---|---|---|
| Analysis latency (12 MP → 512 px proxy) | ≤ 3 000 ms | **60.6 ms** | 49× margin |
| Predictor inference (12 MP → 64 px, ort CPU) | ≤ 3 000 ms | **49.3 ms** | 61× margin |
| Model size (dummy fixture) | ≤ 50 MiB | 921 B | trivial |
| Inference memory overhead | ≤ 300 MB | not measured this session (S3 evidence: RSS ≈ 12 MB at these sizes) | carried from S3 |

## 5. Bugs the harness caught during development (kept for the record)

* **Shaping-knot domain mismatch:** knots were measured pre-EV but the
  operator applies the curve post-EV — on the crushed scene the white
  stretch lifted midtones by +1.6 stops. Fixed by measuring knot sources
  post-EV (exact for the monotone EV scale); benchmark aggregate improved
  0.29 → 0.20.
* **NaN survives `f32::clamp`:** a model outputting NaN EV produced NaN
  params. `ToneParams::sanitized` now defaults non-finite knobs first.
* **WB gains carried a luminance factor** (analysis against unit-luma
  white) which tripped the ±3-stop chromatic clamp on valid casts —
  normalized by geometric mean (luminance is EV's job).
* **Warm flat fields are skin-colored:** the skin guard correctly damps
  their gray-world correction; the test suite pins that behavior instead
  of fighting it.

## 6. Honest gaps

* GPU mirror of the tone stage is not implemented (CPU single-pass is
  ~19 ms/MP in debug; the spatial-kernel GPU follow-up from M2 covers it
  if profiling ever demands).
* The network downloader ships **with the first gate-passing model** —
  v1 installs from local files (owner-side path) only. An HTTPS client
  with an empty registry would be unreachable attack surface.
* WB is stored as cone gains, not Kelvin (round-trip exact; a CCT
  parameterization needs a Planckian-locus table — documented follow-up
  in `tone.rs`).
* Mask suggestions (ADR-0004 §1c) deferred: nothing passed the gate, so
  the mask-edge golden tests have no subject yet.
* The benchmark corpus is synthetic; real-photo evaluation happens on
  owner reference machines (OPEN_QUESTIONS #8) with the same harness.
