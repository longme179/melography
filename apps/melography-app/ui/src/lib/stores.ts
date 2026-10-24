// Shared reactive state (Svelte stores).

import { writable, derived } from "svelte/store";
import type { AppState, AppErrorPayload } from "./ipc";

export const state = writable<AppState>({
  imageOpen: false,
  imageName: null,
  imageDims: null,
  lutStack: [],
  dirty: false,
  sidecarPath: null,
  lastClampedPixels: 0,
  libraryCount: 0,
});

/** Latest error surfaced to the banner (UX_SPEC §9: message + cause + next action). */
export const lastError = writable<AppErrorPayload | null>(null);

/** Viewport zoom as a scale factor (1 = fit). */
export const zoom = writable<number>(1);

/** Compare mode: 0 = off, otherwise the split position 0..1. */
export const compareSplit = writable<number>(0.5);
export const compareOn = writable<boolean>(false);

/** Transient toast (e.g. "Recipe saved"). */
export const toast = writable<string | null>(null);

export const clippingActive = derived(
  state,
  ($s) => $s.lastClampedPixels > 0,
);
