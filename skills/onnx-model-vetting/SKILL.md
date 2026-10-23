---
name: onnx-model-vetting
description: Use before adding, updating, or shipping ANY AI model, and when changing the model manager. Runs the license/security/budget gate from ADR-0004. A model without a passing manifest never ships or downloads.
---

# ONNX Model Vetting

## Prerequisites
- Read ADR-0004 + `docs/AI_MODELS.md` (manifest schema is normative).
- Check current candidate status in `docs/research/ai.md` BEFORE research: several families are already excluded (e.g., Zero-DCE repo has no license).

## Workflow
1. **Locate canonical upstream** (repo + exact weights file + revision). Note forks are not sources.
2. **License split:** code license ≠ weights license ≠ training-data terms. Record all three. Missing/ambiguous weights license → REJECT (record why; re-checkable later).
3. **Static inspection:** opset version; op list (flag dynamic-control/exotic ops); input/output shapes + dtypes; preprocessing contract; quantization artifacts. Weights are untrusted data — parse, never execute anything packaged alongside.
4. **Integrity:** record SHA-256 + byte size; downloads later must match (allowlist domain, HTTPS).
5. **Measure** on reference hardware per OS (CI or owner devices): latency (≤ 3 s @ ≤512 px analysis), memory overhead (≤ 300 MB), disk (≤ 50 MB compressed). Over budget → reject.
6. **Quality benchmark:** must measurably beat the non-ML baseline (AI_MODELS §2) on the internal fixed set; record the comparison in the manifest.
7. **Fill the full manifest** (AI_MODELS §4 schema) — any empty mandatory field blocks merge.
8. Output contract: parameters/masks only; never pixels (ADR-0004 §1). Verify output decoding is clamped/validated before touching the pipeline.

## Verification checklist
- [ ] Manifest complete: upstream, sha256, licenses (all three kinds), budgets measured, quality vs baseline
- [ ] No executable content from the model repo ran or shipped
- [ ] Downloader: allowlist + checksum + atomic install + delete path tested
- [ ] Sidecar provenance recorded on Accept (name/version/sha/timestamp)
- [ ] Offline mode unaffected (app fully functional with zero models)

## Common pitfalls
- Trusting a repo's code license to cover weights — it usually doesn't.
- "It's only 40 MB" then shipping a 300 MB EP requirement.
- Benchmarking against nothing (no baseline numbers) — the model must earn its size.
- Letting model output bypass recipe editability (baked pixels) — architecturally forbidden.

## Links
ADR-0004 · `docs/AI_MODELS.md` · `docs/research/ai.md` · skill `license-and-asset-vetting`

## Expected outputs
Complete manifest JSON + measurement table + benchmark comparison + accept/reject decision recorded in the PR/issue.
