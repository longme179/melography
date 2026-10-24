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

// ─── Film Lab (M2) ─────────────────────────────────────────────────────────

export interface Stock {
  id: string;
  display_name: string;
  provenance: string;
  confidence: string;
  polarity: string;
  layers: string;
  default_path: string;
  provenance_text: string;
}

export interface FilmAdjustments {
  enabled: boolean;
  ev: number;
  push_pull_stops: number;
  dev_contrast: number;
  print_exposure_stops: number;
  inversion_path: string | null;
  grain_intensity: number;
  halation_intensity: number;
  grain_seed: number | null;
  cross_process: string;
  bleach_bypass: number;
  pre_desaturate: number;
}

export interface FilmState {
  stock: string | null;
  enabled: boolean;
  adjustments: FilmAdjustments;
}

export const stocks = writable<Stock[]>([]);

export const film = writable<FilmState | null>(null);

export const filmActive = derived(film, ($f) => !!$f?.stock && $f?.enabled);

// ── M3: tone stage + auto-lighting + model manager ─────────────────────────

export interface WbGains {
  r: number;
  g: number;
  b: number;
}

export interface ToneParams {
  ev: number;
  highlights: number;
  shadows: number;
  wb: WbGains | null;
  black_source: number | null;
  black_target: number | null;
  white_source: number | null;
  white_target: number | null;
}

export interface SuggestionSource {
  kind: "baseline" | "model";
  name: string;
  version: string;
  weights_sha256: string | null;
  accepted_utc: string;
}

export interface ToneState {
  params: ToneParams | null;
  source: SuggestionSource | null;
}

export interface Suggestion {
  params: ToneParams;
  notes: string[];
  wb_cct_estimate_k: number | null;
  engine: string;
  engine_version: string;
  analysis_ms: number;
}

export type ModelState =
  | { state: "not_installed" }
  | { state: "downloading"; bytes_done: number; total_bytes: number }
  | { state: "verifying" }
  | { state: "installing" }
  | { state: "ready"; version: string }
  | { state: "corrupt"; reason: string }
  | { state: "missing"; reason: string };

export interface ModelInfo {
  id: string;
  version: string;
  task: string;
  output_kind: string;
  size_bytes: number;
  weights_license: string;
  sha256: string;
  state: ModelState;
  known_issues: string[];
}

export const tone = writable<ToneState | null>(null);
export const suggestion = writable<Suggestion | null>(null);
export const models = writable<ModelInfo[]>([]);
