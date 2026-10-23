---
name: lut-engine
description: Use when implementing or changing LUT parsing, validation, interpolation, library storage, or the LUT UI states. Enforces format strictness, encoding contracts, and the CPU/GPU oracle discipline.
---

# LUT Engine

## Prerequisites
- Read `docs/research/luts.md` (format pitfalls + encoding contract) and `docs/ARCHITECTURE.md` §7 (library/sidecar references by hash).

## Workflow
1. **Parser strictness first:** token-stream parsing (not line-based) for `.cube`; exact entry-count enforcement; DOMAIN sanity; NaN/Inf rejection; size caps (N ≤ 256 for 3D, ≤ 65536 for 1D; file ≤ 64 MB default). Malformed = structured `LutError` with line number, never best-effort.
2. **Interpolation:** tetrahedral CPU in `melography-lut`; WGSL mirror in `melography-gpu`; both validated against the *independent* oracle in `tools/` (write the oracle from the math in luts.md, not by transcribing the CPU code).
3. **Identity invariant** property test: identity-sampled LUT → input within 1e-6.
4. **Encoding contract:** the LUT stage wraps declared input/output encodings at its insertion point; unknown/undeclared = amber UI warning, never silent application.
5. **Library:** content-addressed storage (SHA-256 filename), provenance metadata (original name, URL, license note, declared encodings), checksum re-verified at load; missing file = explicit error state + locate flow.
6. **Fuzzing:** every parser change re-runs the fuzz corpus (malformed, truncated, oversized, adversarial domain values) — add discovered crashes as regression cases.
7. **Parity & tiling:** CPU↔GPU ≤ 1e-6 f32 after identical rounding; tiled ≡ monolithic.

## Verification checklist
- [ ] Fuzz corpus green; new malformed samples added for any fixed bug
- [ ] Identity + neutral-ramp + domain-transform tests green
- [ ] Encoding declaration flows through to UI states (badges/warnings)
- [ ] Sidecar references by hash; missing-LUT error path exercised in a UI test
- [ ] No `.3dl`/Hald support merged before their dialect/validation specs are implemented (M1+)

## Common pitfalls
- Line-based `.cube` parsing (tables wrap across lines) — stream tokens.
- Silent clamping at domain edges without surfacing "domain clipping" to UI.
- Guessing a LUT's color space from its filename ("looks like a log LUT") — never.
- Storing LUTs by user filename (breaks portability); always by content hash.

## Links
`docs/research/luts.md` · `docs/ARCHITECTURE.md` §7 · skill `color-management`

## Expected outputs
Parser/interpolator changes + oracle parity numbers + fuzz corpus growth + updated library state machine tests.
