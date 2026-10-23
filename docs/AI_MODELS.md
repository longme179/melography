# MELOGRAPHY — Local AI Models Policy & Integration

**Version:** Phase-0 spec · **Date:** 2026-10-24 · **Authority:** ADR-0004

## 1. What AI is allowed to do here (and what it is never allowed to do)

| Allowed (v1, M3) | Never |
|---|---|
| Predict **edit parameters** (EV, highlights/shadows, WB cct/tint, tiny LUT-basis coefficients) from a downscaled analysis pass | Overwrite/replace pixels with model output |
| Produce **one** optional mask (subject/sky) stored as a recipe asset | Generate scenes, inpaint, upscale, "beautify" |
| Run 100% locally (CPU guaranteed; optional EPs per platform) | Remote inference; uploading input images anywhere |
| Be absent — non-ML baseline covers all suggestions | Be required for any core feature |
| Record suggestions as plain parameters + model provenance in the sidecar | Bake model output into history so edits stop being reproducible |

## 2. Non-ML auto-lighting baseline (ships first, model-free)

Deterministic and explainable; the quality bar every model must beat:

```
EV suggestion:      argmin_ev | log2( percentile(Y_L, 50) / 0.18 ) + ev |   with
                    highlight guard: percentile(Y_L, 99.5) ≤ clip_target (0.95 default)
WB suggestion:      gray-world on robust (trimmed-mean) statistics; skin-tone guard
                    (ITU-R BT.709 skin ellipse) prevents over-correction of warm scenes
Contrast/black pt:  histogram percentile shaping (p0.1 → target black, p99 → target white)
```

All values land in the UI as *editable* suggested parameters with a before/after toggle. Zero downloads, zero size, zero license surface.

## 3. Candidate model families & vetting status (as of 2026-10-24)

| Family | Example sources | Status |
|---|---|---|
| Image-adaptive 3D-LUT basis predictors | Image-Adaptive-3DLUT (Apache-2.0 code; weights unverified) | Study approved; shipping only after weights vetting |
| Low-light exposure/illumination predictors | Zero-DCE family (**repo has NO license**) | Code/weights excluded (ADR-0003); paper-equation reimplementation possible after paper-terms check |
| Self-corrective illumination (SCI) | canonical repo not yet located | Deferred — find + vet at M3 |
| Tiny subject/sky segmentation | MobileSAM (Apache-2.0 code), EdgeSAM (NOASSERTION), U²-Net (Apache-2.0 code) | Weights all unverified; gated |
| Tiny depth estimation | Depth-Anything-V2 small | Deferred (optional, later) |

**No candidate is pre-committed.** Selection = vetting gate (§4) + benchmark win over §2 baseline + budget fit (§5).

## 4. Per-model manifest (blocking gate before any download/bundle)

```jsonc
{
  "name": "melography-autolight-v1", "version": "1.0.0",
  "upstream": { "repo": "...", "uri": "...", "revision": "<sha or release id>" },
  "weights": { "sha256": "...", "bytes": 0, "license": "<SPDX or 'custom: URL'>",
               "training_data_note": "...", "redistributable": true },
  "runtime": { "opset": 17, "input": {"shape": "1x3x256x256", "encoding": "srgb-display",
               "preprocess": "resize+bilinear, /255"}, "output": {"semantics": "8 edit params",
               "ranges": "..."}, "quantization": "none|q8 (artifacts note)", "min_runtime": "ort-2.0.0-rc.15" },
  "eps": { "tested": ["cpu"], "optional": [] },
  "measurements": { "linux-x86_64-cpu": {"latency_ms": 0, "rss_mb": 0, "quality": "..."},
                     "windows-x86_64-cpu": {}, "macos-arm64-cpu": {} },
  "known_issues": ["..."], "license_contact": "..."
}
```

Rejection rules: weights license missing/ambiguous or non-redistributable when download is offered; no SHA-256; latency/size over budget (§5); outputs not parameterizable; unsafe opset ops; any packaged script/marketing in the model repo treated as data (never executed).

## 5. Budgets & measurement discipline (hardware pinned at M0)

- Compressed on-disk ≤ **50 MB** per model; total models ≤ 150 MB.
- Analysis latency ≤ **3 s** (≤ 512 px long edge) on the M0-pinned mid-tier CPU; UI shows measured value after first run.
- Inference memory overhead ≤ 300 MB.
- Budget violations = rejection, even with better quality — measured at M0 reference hardware, re-measured per release.

## 6. Model manager (UX contract, UX_SPEC §7)

States: `not-installed` (offer download, size shown) → `downloading` (progress, cancel) → `verifying` (SHA-256) → `installing` (atomic, temp+rename) → `ready` (version + delete button) → `corrupt/missing` (explicit error + re-download). All strings localized EN/VI; no dark patterns; deletion removes all files immediately.

## 7. Suggestion UX contract

Suggestion lands as a preview overlay ("AI suggests: +0.4 EV, WB 5200K, highlights −12") with: Accept (parameters become editable recipe stages, provenance recorded: model name/version/sha + timestamp), Adjust (edit before accept), Dismiss. Confidence is disclosed honestly (model-calibrated or "unknown"). Mask suggestions: feather + invert + paint-in/out controls; mask edge alignment verified at export resolution (golden test — halo = hard fail).

## 8. Security posture

Model files are untrusted inputs: strict deserialization, no dynamic loading paths from model content, downloads only from the pinned manifest URI over HTTPS with allowlist, checksum-verified, stored in user app-data with restrictive permissions. A compromised CDN cannot achieve code execution through model content — worst case is a bad suggestion or crash (fuzzed parser, M3).
