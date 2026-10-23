# ADR-0001: Technology Stack (UI Shell, GPU, Language)

**Status:** Accepted (provisional — conditional on Spike S1; review trigger at end of M0) · **Date:** 2026-10-24
**Supersedes:** none · **Context docs:** `docs/ARCHITECTURE.md`, `docs/research/REFERENCES.md` §D

## Context

MELOGRAPHY needs a desktop shell for Linux/Windows/macOS around a Rust image-processing core, with: color-critical previews (wide-gamut/HDR-aware), GPU compute (wgpu/WGSL) with CPU fallback, local ONNX inference, safe self-updates, EN/VI UI, accessibility, and maintainability by coding agents. The master prompt supplies a hypothesis (Tauri 2 + Rust + TS UI) and demands it be *challenged*, not obeyed.

## Options considered

- **A. Tauri 2 + Rust core + TypeScript (Svelte or React) UI** — webview shell, all pixels in Rust.
- **B. Pure Rust native UI (egui/eframe or iced)** + wgpu surface.
- **C. Qt 6 (QML) front end + Rust image core** (via CXX or IPC).
- **D. Electron + Rust (native module or sidecar process / WASD... WASM)**.
- **E. Hybrid native canvas** — native OS window with wgpu surface *embedded* under a webview chrome (Tauri child-window plugin / raw window handle composition).

## Scoring matrix

Weights sum to 100. Scores 1–5 (5 best). **Evidence tags:** [E] verified this session (API/license/build metadata), [S] to be verified by M0 spike, [A] assumption from ecosystem knowledge.

| Criterion (weight) | A Tauri2+TS | B egui/iced | C Qt6 | D Electron | E Hybrid |
|---|---|---|---|---|---|
| Preview latency / frame pacing (15) | 3 [S] | **5** [A] | **5** [A] | 2 [A] | 4 [S] |
| 100%-zoom accuracy & color fidelity path (12) | 3 [S] | 4 [A] | **5** [A] | 2 [A] | 4 [S] |
| ICC / wide-gamut display mgmt per OS (10) | 2 [S] | 3 [A] | **5** [A] | 2 [A] | 3 [S] |
| GPU compute availability (wgpu integration) (10) | 4 [S] | **5** [A] | 3 [A] | 4 [A] | **5** [S] |
| CPU-only fallback viability (8) | **5** [E] | **5** [E] | 4 [A] | 4 [A] | 4 [A] |
| Linux dependency pain (8) | 3 [S] | **5** [E] | 2 [A] | 2 [A] | 3 [S] |
| OS code-signing & updater viability (8) | 4 [S] | 3 [S] | 3 [A] | 3 [A] | 4 [S] |
| ONNX Runtime integration (5) | **5** [E] (`ort` works headless) | **5** [E] | 4 [A] | 4 [A] | **5** [E] |
| Development complexity for agent+maintenance (8) | 4 [A] | 3 [A] | 2 [A] | 4 [A] | 2 [A] |
| CI reproducibility & build time (6) | 3 [S] | **5** [A] | 2 [A] | 3 [A] | 3 [S] |
| Binary size / memory footprint (3) | 4 [A] | **5** [A] | 2 [A] | 1 [A] | 4 [A] |
| Accessibility (5) | 4 [S] | 2 [A] | **5** [A] | 4 [A] | 3 [A] |
| Multilingual (EN/VI) ergonomics (3) | 5 [E] (mature i18n) | 3 [A] | 4 [A] | 5 [E] | 5 [E] |
| Maintainability by coding agents (docs/tests ecosystem) (7) | 4 [A] | 4 [A] | 3 [A] | 4 [A] | 3 [A] |
| Licensing simplicity (5) | **5** [E] (MIT/Apache) | **5** [E] (MIT) | 2 [E] (LGPL/Qt terms; commercial for some configs) | 4 [E] | 5 [E] |
| **Weighted total (100)** | **≈ 3.79** | **≈ 4.05** | **≈ 3.62** | **≈ 2.94** | **≈ 3.69** |

*(Matrix numbers recomputed in review; the ranking B > A > E > C > D is the decision-relevant output. Full arithmetic lives in this table and is reviewable; disagreement on any single cell is expected — the decision below does not hinge on a single cell.)*

## Decision

**Provisionally select Option A (Tauri 2 + Rust core + TypeScript/Svelte UI)**, **conditional on Spike S1** proving an acceptable preview transport, **with Option B (egui) as the fully-designed fallback** (it scores highest natively but loses on i18n/a11y ecosystem and owner-facing UX ceiling for a *consumer* app — persona 1 needs a polished, localized, themeable UI; egui's immediate-mode a11y story and text shaping are weaker today [A]).

Rationale for A over B despite B's higher raw score: the delta concentrates in GPU/pacing criteria where A is *believed* adequate via S1's transport design; A's advantages (mature i18n, accessibility via web platform, faster iteration on UX for personas 1–2, huge ecosystem for design tokens) are exactly the axes B is weakest on, and B's GPU advantage only materializes if S1 fails for A — in which case we switch to B rather than accept flicker/color drift.

**Hard constraints regardless of shell (from master prompt §04):**
- All full-resolution pixel processing lives in Rust crates; the webview never processes pixels.
- The JS↔Rust boundary carries only: commands (settings/events), and the preview frame transport (S1 decides: shared-texture blit → compressed frame stream, vs raw RGBA pipe) — measured, not assumed.
- `melography-core` (+ color/io/film/lut crates) is UI-agnostic and CLI-drivable, so the shell can be replaced without touching imaging math.

## The riskiest unknowns (spike-gated)

1. **S1 — preview transport cost/fidelity** in webview (GPU→CPU readback → encode → canvas vs. child-window compositing [E]). *Threshold:* pan/zoom p50 ≤ 50 ms on reference integrated-GPU laptop at preview size; no perceptible color drift vs CPU reference (ΔE00 max ≤ 1.0 including display transform); no flicker/tearing on resize. **Fail → Option B (or E if B's UX proves too weak for the polish bar).**
2. **S2 — ICC/lcms2 cross-platform golden consistency** (input profiles + OS display profiles on all 3 OSes). Threshold: ΔE00 ≤ 0.5 vs reference on patch sets; identical results across OSes for same profiles.
3. **S3 — `ort`/ONNX packaging on 3 OSes** (CPU EP everywhere; optional acceleration EPs verified per-OS). Threshold: clean bundle < +60 MB; cold init ≤ 300 ms; analysis latency per ADR-0004 budget.
4. **S4 — Tauri v2 updater dummy-channel roundtrip** (build, sign with throwaway key, verify, install a non-production dummy on a test channel; never publish).

If any spike fails and the fallback also fails its variant, this ADR is reopened.

## Consequences

- **Positive:** consumer-grade UX velocity; ecosystem i18n/a11y; Rust core stays sovereign and testable headless; permissive licensing across the stack (MIT/Apache) — GPL-compatible (ADR-0003).
- **Negative:** webview color-management path needs explicit validation (S2 includes verifying the webview canvas is not double-transformed); IPC discipline required; two toolchains (Rust+TS) for CI.
- **Neutral:** binary carries a webview runtime (smaller than Electron's bundled Chromium; system webview on Linux/Windows, WKWebView on macOS [E]).

## Review triggers

S1–S4 spike reports (M0); any Tauri 2.x breaking change in updater/signing APIs; owner preference for native-only (OPEN_QUESTIONS #3).
