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
  canUndo: false,
  canRedo: false,
  filmStock: null,
  missingStock: null,
  sourcePath: null,
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

export interface LensParams {
  vignette_amount: number;
  lens_fov_deg: number;
  veiling_amount: number;
  veiling_sigma_mm: number;
  chromatic_aberration: number;
  distortion: number;
}

export interface FilmState {
  stock: string | null;
  enabled: boolean;
  adjustments: FilmAdjustments;
  /** M4 #23: the Lens panel surface. */
  lens: LensParams;
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
  /** Display estimates for the Color panel (Kelvin is a UI projection
   * of the cone gains; `color::tone` owns the round-trip tolerances). */
  cct_estimate_k: number | null;
  tint_estimate: number | null;
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

// ─── M4 #23: settings · recents · presets · filmstrip ──────────────────────

export interface AppSettings {
  language: string;
  theme: string;
  preview_max_dim: number;
  cache_budget_mb: number;
  gpu_enabled: boolean;
  update_channel: string;
  offline_mode: boolean;
  log_level: string;
}

export interface RecentItem {
  path: string;
  name: string;
  last_opened_utc: string;
  width: number;
  height: number;
  has_recipe: boolean;
  missing: boolean;
  thumb_data_url: string | null;
}

export interface UserPreset {
  id: string;
  name: string;
  created_utc: string;
  modified_utc: string;
  source: string;
  summary: string;
}

export interface StockThumb {
  stock_id: string;
  thumb_data_url: string;
}

export const settings = writable<AppSettings | null>(null);
export const recents = writable<RecentItem[]>([]);
export const presets = writable<UserPreset[]>([]);
export const stockThumbs = writable<StockThumb[]>([]);

export const missingStock = writable<string | null>(null);

// ─── M4 #23 part 2 (session 08): S6 batch queue + WB eyedropper ────────────

export interface BatchJobError {
  code: string;
  message: string;
  /** i18n key for the actionable next step ("hint.*" — localized in the
   * row, e.g. disk full → folder chooser + retry). */
  hint: string | null;
}

export type BatchJobStatus = "queued" | "rendering" | "done" | "error" | "cancelled";

export interface BatchJob {
  id: number;
  name: string;
  source: string;
  preset_id: string | null;
  preset_name: string | null;
  format: string;
  depth: number;
  quality: number;
  dest_dir: string;
  dest: string | null;
  status: BatchJobStatus;
  error: BatchJobError | null;
  warnings: string[];
  dims: [number, number] | null;
  bytes: number | null;
  total_ms: number | null;
  added_at: string;
  finished_at: string | null;
  has_thumb: boolean;
}

/** Wire snapshot from Rust — jobs + the pause flag ride together so the
 * UI never keeps a second opinion (single source: the queue itself). */
export interface BatchSnapshot {
  jobs: BatchJob[];
  paused: boolean;
}

/** The queue, kept fresh by `batch://update` events + command returns. */
export const batchJobs = writable<BatchJob[]>([]);
export const batchPaused = writable<boolean>(false);

/** Rows that are queued or rendering (toolbar badge + summary). */
export const batchActiveCount = derived(batchJobs, (jobs) =>
  jobs.filter((j) => j.status === "queued" || j.status === "rendering").length,
);

/** WB eyedropper mode (Color panel ⇄ Viewport): true = the next viewport
 * click picks a neutral reference; Esc cancels. */
export const wbPicker = writable<boolean>(false);
