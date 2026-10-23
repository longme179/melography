# MELOGRAPHY

Offline-first desktop image editor that makes ordinary (non-RAW) photographs convincingly resemble analog film photographs — built on a **Rust image-processing core**, running locally on **Linux, Windows, and macOS**.

> **Status: Phase 0 — research, architecture, and planning.**
> This repository currently contains planning artifacts only. No product features are implemented yet, and no release has been published. Review work-in-progress documentation on the [draft pull requests](https://github.com/longme179/melography/pulls).

## Principles

- **Offline-first, privacy by default** — no accounts, no telemetry, no uploads; the only optional network activity is explicit update checks / model downloads with user consent.
- **Non-destructive** — original image files are never modified; edits live in versioned sidecar recipes.
- **Rust core** — all pixel processing happens in Rust (GPU via wgpu/WGSL where available, deterministic CPU fallback everywhere).
- **Local-only AI** — optional small ONNX models for lighting suggestions and masks; CPU always supported.
- **English + Vietnamese UI** from the first beta; accessibility and high-DPI are first-class.

## V1 non-goals

RAW development, video, mobile apps, accounts, cloud editing/sync, generative scene alteration, subscription infrastructure, plugin marketplace, full DAM/catalog, printer driver support.

## Repository layout

Planning documentation (architecture, color pipeline, film model, ADRs, roadmap) lands via the `agent/phase-0-foundations` branch under `docs/`, `skills/`, `references/`, and `.github/`. See [`AGENTS.md`](AGENTS.md) for the working conventions map.

## License

**Undecided** — tracked in `docs/adr/0003-licensing.md` (Phase-0 branch). Until the owner decides: original clean-room implementations only; no third-party source code is copied into this repository.
