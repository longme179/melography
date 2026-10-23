# ADR-0004: Local AI & Model Policy

**Status:** Accepted · **Date:** 2026-10-24
**Related:** `docs/AI_MODELS.md` (full policy + manifest schema), ADR-0001 (S3 spike), `docs/research/ai.md`

## Context

The master prompt allows *small, optional, local* AI models for lighting suggestions and masks, but demands: local inference only, ONNX where suitable, CPU always supported, editability preserved (no pixel-baking suggestions), opt-in downloads with pinned checksums, strict model vetting (license of **weights** distinct from code), and a non-ML baseline first.

## Decision

**v1 AI scope = parameter predictors + optional single mask. Pixel-generating AI is excluded.**

1. **Order of arrival (M3):** (a) deterministic non-ML auto-lighting baseline (no model, no download); (b) one ONNX *parameter predictor* (exposure/highlights/shadows/WB vectors, optionally small LUT-basis coefficients) — if and only if it measurably beats (a) on the internal benchmark; (c) optional tiny subject/sky segmentation mask — if and only if latency, license, and mask-edge quality gates pass. Everything else deferred.
2. **Runtime:** ONNX Runtime via `ort` (MIT/Apache; 2.0.0-rc.15 pinned at M0 — RC status is a tracked risk; CPU EP mandatory on all platforms; CoreML/DirectML/CUDA EPs opt-in per-platform after S3 verification, never required).
3. **Budgets (measurement-defined, hardware pinned at M0):** model ≤ 50 MB compressed; reduced-resolution (≤ 512 px) analysis ≤ 3 s on reference mid-tier CPU; memory overhead ≤ 300 MB during inference. Exceeding budget = rejection, regardless of quality.
4. **Vetting gate (per model, before any download/bundle):** manifest with — exact name/version; canonical upstream + weights URI + revision; **weights license** (distinct from code license; "research-only" weights are rejected); redistribution/commercial terms; filesize + SHA-256; opset, tensor shapes, preprocessing, output semantics; quantization notes; minimum runtime; tested EPs; measured latency/memory/quality per OS; known biases/failures. **Ambiguous weights = rejected.** Downloaded weights are untrusted file inputs (parsed, never executed; no scripts from model repos run).
5. **Distribution:** never bundled with installers by default. Opt-in download, SHA-256-pinned, atomic install into user app-data, versioned manifest, deletable from Settings. Offline mode fully functional without any model.
6. **UX contract:** progress + cancellation; model-not-installed state; before/after comparison of *our pipeline* applying suggested parameters; confidence disclosure; suggestions recorded in the sidecar as plain parameters (model name/version/hash recorded) — reproducible forever after, even if the model is deleted; mask assets regenerated deterministically at export resolution with edge-alignment golden tests (halo = hard fail).
7. **Privacy:** input images never leave the machine; inference is local; no telemetry; model download is the only AI-related network operation and requires explicit user action.

## Consequences

- The app's core value (film pipeline) has zero AI dependency; AI is a removable enhancement — protecting the offline-first promise and the performance budget.
- The vetting burden is real: the M3 candidate list (`docs/research/ai.md`) is deliberately *not* pre-committed; Zero-DCE-style repos without licenses are already excluded from shipping.
- If no model passes the gates in M3, v1 ships with the non-ML baseline only — acceptable outcome, not a failure.
