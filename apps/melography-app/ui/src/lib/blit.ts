// MELOGRAPHY — RGBA→canvas blit: the single path every canvas fill in the
// shell goes through (ADR-0001: the webview transports Rust-produced
// pixels, it never computes them).
//
// Session 15 rationale — the editor's most frequent runtime error was
//   "Something needs attention; undefined is not an object
//    (evaluating 'p.width=v.width')"
// i.e. the minified `originalCanvas.width = original.width` in
// Viewport.blit(): the compare canvas is bound by `bind:this` only while
// its `{#if}` block is mounted, but blit() touched it whenever an
// `original` frame existed — so EVERY render pass with compare mode off
// threw a TypeError into the error banner (the preview itself had already
// drawn, which is why the app "worked" while the banner spammed).
//
// This module makes "the canvas is not there (yet/anymore)" a NORMAL,
// typed, handled case: parameters are `| null | undefined` by signature
// (not by convention at each call site), and a missing canvas, a missing
// frame, degenerate dimensions or a truncated buffer are no-ops that
// leave the caller's previous pixels untouched. `tsc --noEmit` (the CI
// `check` gate) enforces the guards inside this file; the CI smoke gate
// `scripts/ui_blit_smoke.mjs` exercises them and enforces that this
// remains the ONLY module assigning canvas `.width`/`.height`.

import type { PreviewFrame } from "./ipc";

/**
 * Blit a raw RGBA8 buffer into a canvas.
 *
 * No-op (returns `false`, canvas untouched) when:
 * - the canvas is not bound (compare off, element not mounted yet,
 *   or the image was closed mid-render and Svelte unbound it),
 * - the buffer is missing,
 * - dimensions are not finite positive integers,
 * - the buffer length is not exactly `width * height * 4` (a truncated
 *   or mismatched transport frame must never half-draw).
 *
 * On success resizes the backing store and puts the pixels; returns
 * `true`. A 2d context is acquired BEFORE any resize so a null context
 * also leaves the canvas untouched.
 */
export function putRgba(
  canvas: HTMLCanvasElement | null | undefined,
  rgba: Uint8ClampedArray<ArrayBuffer> | null | undefined,
  width: number,
  height: number,
): boolean {
  if (!canvas || !rgba) return false;
  if (!Number.isFinite(width) || !Number.isFinite(height)) return false;
  if (width <= 0 || height <= 0) return false;
  if (!Number.isInteger(width) || !Number.isInteger(height)) return false;
  if (rgba.length !== width * height * 4) return false;
  const ctx = canvas.getContext("2d");
  if (!ctx) return false;
  canvas.width = width;
  canvas.height = height;
  ctx.putImageData(new ImageData(rgba, width, height), 0, 0);
  return true;
}

/**
 * Blit a whole Rust-produced preview frame (the `PreviewFrame` wire
 * payload) into a canvas. Same no-op contract as {@link putRgba}.
 */
export function blitFrame(
  canvas: HTMLCanvasElement | null | undefined,
  frame: PreviewFrame | null | undefined,
): boolean {
  if (!frame) return false;
  return putRgba(canvas, frame.rgba, frame.width, frame.height);
}
