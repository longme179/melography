// Typed IPC wrappers — the ONLY bridge to Rust (ARCHITECTURE §1: params
// in, preview frames out; the webview never computes pixels).

import { invoke } from "@tauri-apps/api/core";
import type { FilmState, Stock } from "./stores";
import { open, save } from "@tauri-apps/plugin-dialog";

/** Preview wire format (render.rs): LE header + RGBA8.
 * magic u32 | version u32 | width u32 | height u32 | render_ms f64 |
 * clamped u64 | rgba bytes */
const WIRE_MAGIC = 0x4d475046;
const WIRE_HEADER = 32;

export interface PreviewFrame {
  width: number;
  height: number;
  renderMs: number;
  clamped: number;
  // TS 5.7+ typed-array generics: parseFrame builds this from a real
  // ArrayBuffer, and the precise generic is what lib.dom's ImageData
  // constructor accepts (blit.ts) — a plain `Uint8ClampedArray` default
  // (ArrayBufferLike) no longer typechecks through that call.
  rgba: Uint8ClampedArray<ArrayBuffer>;
}

export interface ImageInfo {
  name: string;
  width: number;
  height: number;
  sidecarLoaded: boolean;
  fingerprintSha256: string;
}

export interface LutRecord {
  contentHash: string;
  originalFilename: string;
  title: string | null;
  kind: string;
  size: number;
  domainMin: [number, number, number];
  domainMax: [number, number, number];
  declaredInput: string;
  declaredOutput: string;
  sourceUrl: string | null;
  licenseNote: string | null;
  importedUtc: string;
  warnings: { kind: string }[];
  implemented: boolean;
}

export interface LutStackEntry {
  lutHash: string;
  insertion: string;
  intensity: number;
  declaredInput: string;
  declaredOutput: string;
  enabled: boolean;
}

export interface AppState {
  imageOpen: boolean;
  imageName: string | null;
  imageDims: [number, number] | null;
  lutStack: LutStackEntry[];
  dirty: boolean;
  sidecarPath: string | null;
  lastClampedPixels: number;
  libraryCount: number;
  /** Undo availability (M4 #24 — drives toolbar + keyboard). */
  canUndo: boolean;
  canRedo: boolean;
  /** Current film stock id (null = film off) — panel refresh (M4 #23). */
  filmStock: string | null;
  /** Recipe referenced a stock missing from the registry (amber badge). */
  missingStock: string | null;
  /** Absolute path of the open original (S6 add-to-batch). */
  sourcePath: string | null;
  /** Where the stock registry was resolved from + how many stocks it
   * yielded (session 11) — the Film Lab empty state names the path it
   * looked in (UX_SPEC §9 honest failure: never a silent "no stocks"). */
  stocksDir: string;
  stocksCount: number;
}

export interface AppErrorPayload {
  code: string;
  message: string;
  context: string | null;
}

/** Coerce every transport shape of a raw preview frame into an ArrayBuffer.
 *
 * Tauri resolves a command returning `ipc::Response` (raw bytes) with a
 * real ArrayBuffer ONLY on the custom-protocol IPC path
 * (`fetch ipc://localhost` — requires the CSP `connect-src` to allow
 * `ipc:` / `http://ipc.localhost`). When the webview blocks that
 * protocol, tauri silently falls back to the postMessage interface,
 * where the SAME command arrives as:
 *  - a plain JSON array of numbers (tauri's `format_result` serializes
 *    `Vec<u8>` as JSON on that path), or
 *  - `{ message: ArrayBuffer, index }` (channel-transport wrapper).
 * Feeding either to `new DataView(...)` throws on JavaScriptCore
 * (WebKitGTK): "Expected ArrayBuffer for the first argument." — the
 * exact error the owner's first built-app run hit (session 12; every
 * JSON command kept working, so only the viewport died). Normalizing
 * here keeps previews functional on every transport, platform and
 * webview policy — the CSP fix restores the fast path, this keeps the
 * fallback honest. */
function frameBuffer(value: unknown): ArrayBuffer {
  if (value instanceof ArrayBuffer) return value;
  if (value instanceof Uint8Array) {
    // A view over its own exact buffer passes through; a subarray
    // (byteOffset ≠ 0 / partial) is copied into a standalone buffer.
    // (`TypedArray#buffer` is typed ArrayBufferLike — a view built by
    // `Uint8Array.from`/`slice` is always a plain ArrayBuffer.)
    return value.byteOffset === 0 && value.byteLength === value.buffer.byteLength
      ? (value.buffer as ArrayBuffer)
      : (value.slice().buffer as ArrayBuffer);
  }
  if (typeof value === "object" && value !== null && "message" in value) {
    return frameBuffer((value as { message: unknown }).message);
  }
  if (Array.isArray(value)) {
    return Uint8Array.from(value as number[]).buffer as ArrayBuffer;
  }
  throw new Error(
    `preview frame arrived in an unsupported transport shape (${typeof value})`,
  );
}

function parseFrame(payload: unknown): PreviewFrame {
  const buf = frameBuffer(payload);
  const dv = new DataView(buf);
  if (dv.byteLength < WIRE_HEADER) throw new Error("frame too short");
  const magic = dv.getUint32(0, true);
  if (magic !== WIRE_MAGIC) throw new Error(`bad frame magic 0x${magic.toString(16)}`);
  const version = dv.getUint32(4, true);
  if (version !== 1) throw new Error(`unsupported frame version ${version}`);
  const width = dv.getUint32(8, true);
  const height = dv.getUint32(12, true);
  const renderMs = dv.getFloat64(16, true);
  const clamped = dv.getBigUint64(24, true);
  const rgba = new Uint8ClampedArray(buf, WIRE_HEADER, width * height * 4);
  return { width, height, renderMs, clamped: Number(clamped), rgba };
}

export async function openImageDialog(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [
      {
        name: "Images",
        extensions: ["jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "avif"],
      },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

export async function openImage(path: string): Promise<ImageInfo> {
  return invoke<ImageInfo>("open_image", { path });
}

export async function renderPreview(maxDim: number): Promise<PreviewFrame> {
  // tauri v2: a command returning ipc::Response yields ArrayBuffer on the
  // custom-protocol path — parseFrame normalizes the fallback shapes too.
  const payload = await invoke<unknown>("render_preview", { maxDim });
  return parseFrame(payload);
}

export async function originalFrame(maxDim: number): Promise<PreviewFrame> {
  const payload = await invoke<unknown>("original_frame", { maxDim });
  return parseFrame(payload);
}

/** Cache stats (#25): hit-rates + budgets for the S2 status bar. */
export interface CacheStats {
  preview: {
    hits: number;
    misses: number;
    hit_rate_pct: number;
    budget_mb: number;
    cached_mb: number;
    cached: boolean;
  };
  luts: {
    hits: number;
    misses: number;
    hit_rate_pct: number;
    budget_mb: number;
    resident_mb: number;
    entries: number;
  };
}

export async function cacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>("cache_stats");
}

export async function importLut(args: {
  path: string;
  declaredInput: string;
  declaredOutput: string;
  sourceUrl?: string;
  licenseNote?: string;
}): Promise<LutRecord> {
  return invoke<LutRecord>("import_lut", { args });
}

export async function listLuts(): Promise<LutRecord[]> {
  return invoke<LutRecord[]>("list_luts");
}

export async function removeLut(hash: string): Promise<void> {
  return invoke("remove_lut", { hash });
}

export async function setLutStack(stack: LutStackEntry[]): Promise<void> {
  return invoke("set_lut_stack", { stack });
}

export async function getState(): Promise<AppState> {
  return invoke<AppState>("get_state");
}

/** Undo the last recipe mutation (M4 #24). The backend returns the
 * refreshed state — same shape as getState. */
export async function undo(): Promise<AppState> {
  return invoke<AppState>("undo");
}

/** Redo the last undone mutation (M4 #24). */
export async function redo(): Promise<AppState> {
  return invoke<AppState>("redo");
}

export async function saveRecipe(): Promise<void> {
  return invoke("save_recipe");
}

export async function exportImage(
  args: { format: string; depth: number; quality: number },
  dest: string,
): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("export_image", { args, dest });
}

export async function exportDialog(
  defaultName: string,
  format: string,
): Promise<string | null> {
  const ext = format === "jpeg" ? "jpg" : format;
  const picked = await save({
    defaultPath: `${defaultName}.${ext}`,
    filters: [{ name: format.toUpperCase(), extensions: [ext] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function lutFileDialog(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "Cube LUT", extensions: ["cube"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function listStocks(): Promise<Stock[]> {
  return invoke<Stock[]>("list_stocks");
}

export async function getFilm(): Promise<FilmState> {
  return invoke<FilmState>("get_film");
}

// ── M3: tone + auto-lighting + models ──────────────────────────────────────

import type {
  ModelInfo,
  Suggestion,
  ToneParams,
  ToneState,
  SuggestionSource,
} from "./stores";

export async function getTone(): Promise<ToneState> {
  return invoke<ToneState>("get_tone");
}

export async function setTone(args: {
  params: ToneParams | null;
  source?: SuggestionSource | null;
}): Promise<ToneState> {
  return invoke<ToneState>("set_tone", { args });
}

export async function autoLightSuggest(): Promise<Suggestion> {
  return invoke<Suggestion>("auto_light_suggest");
}

export async function suggestionProvenance(
  kind: string,
  name: string,
  version: string,
): Promise<SuggestionSource> {
  return invoke<SuggestionSource>("suggestion_provenance", {
    kind,
    name,
    version,
  });
}

export async function listModels(): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("list_models");
}

export async function installModelFromFile(
  manifestPath: string,
  weightsPath: string,
): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("install_model_from_file", {
    manifestPath,
    weightsPath,
  });
}

export async function deleteModel(id: string): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>("delete_model", { id });
}

export async function manifestFileDialog(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "Model manifest", extensions: ["json"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function weightsFileDialog(): Promise<string | null> {
  const picked = await open({
    multiple: false,
    filters: [{ name: "ONNX weights", extensions: ["onnx"] }],
  });
  return typeof picked === "string" ? picked : null;
}

// ── M4 #23: settings · recents (S1) · presets (S5) · color panel ───────────

import type { AppSettings, RecentItem, UserPreset, StockThumb, WbGains } from "./stores";

export async function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>("get_settings");
}

export async function setSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke<AppSettings>("set_settings", { settings });
}

export async function listRecents(): Promise<RecentItem[]> {
  return invoke<RecentItem[]>("list_recents");
}

export async function purgeRecents(): Promise<number> {
  return invoke<number>("purge_recents");
}

export async function listPresets(): Promise<UserPreset[]> {
  return invoke<UserPreset[]>("list_presets");
}

export async function savePreset(name: string): Promise<UserPreset> {
  return invoke<UserPreset>("save_preset", { name });
}

export async function renamePreset(id: string, name: string): Promise<UserPreset[]> {
  return invoke<UserPreset[]>("rename_preset", { id, name });
}

export async function deletePreset(id: string): Promise<UserPreset[]> {
  return invoke<UserPreset[]>("delete_preset", { id });
}

export async function applyPreset(id: string): Promise<AppState> {
  return invoke<AppState>("apply_preset", { id });
}

export async function stockThumbsCmd(maxDim: number): Promise<StockThumb[]> {
  return invoke<StockThumb[]>("stock_thumbs", { maxDim });
}

/** Color panel: (Temperature K, Tint −100..100) → WB cone gains (Rust-side
 * color math — the webview never computes color, ADR-0001). */
export async function wbFromCctTint(cctK: number, tint: number): Promise<WbGains> {
  return invoke<WbGains>("wb_from_cct_tint", { cctK, tint });
}

/** Close the current session (S1 ⇆ S2 navigation) — the original is
 * never touched; the sidecar stays next to it. */
export async function closeImage(): Promise<void> {
  return invoke("close_image");
}

// ── M4 #23 part 2 (session 08): S6 batch queue + WB eyedropper ─────────────

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BatchSnapshot } from "./stores";

/** Every queue mutation returns the fresh snapshot; the worker also
 * emits it as `batch://update` — whichever arrives first wins. */
export async function batchAdd(
  paths: string[],
  presetId: string | null,
  spec: { format: string; depth: number; quality: number },
  destDir: string,
): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_add", { paths, presetId, spec, destDir });
}

export async function batchList(): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_list");
}

/** Row thumbnail (lazy, cached Rust-side): PNG data URL. */
export async function batchThumb(id: number): Promise<string> {
  return invoke<string>("batch_thumb", { id });
}

export async function batchPause(): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_pause");
}

export async function batchResume(): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_resume");
}

export async function batchCancel(id: number): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_cancel", { id });
}

export async function batchCancelAll(): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_cancel_all");
}

/** Retry keeps the job's settings; `newDestDir` is the disk-full flow. */
export async function batchRetry(
  id: number,
  newDestDir: string | null,
): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_retry", { id, newDestDir });
}

export async function batchRemove(id: number): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_remove", { id });
}

export async function batchClear(): Promise<BatchSnapshot> {
  return invoke<BatchSnapshot>("batch_clear");
}

/** Worker events → stores. Returns the unlisten fn (App unmount). */
export async function listenBatch(
  onSnapshot: (snap: BatchSnapshot) => void,
): Promise<UnlistenFn> {
  return listen<BatchSnapshot>("batch://update", (event) => onSnapshot(event.payload));
}

/** WB eyedropper: normalized (0..1, top-left) viewport coords → gains. */
export async function wbPick(nx: number, ny: number): Promise<ToneState> {
  return invoke<ToneState>("wb_pick", { nx, ny });
}

/** Output-folder chooser (S6 + disk-full retry flow). */
export async function directoryDialog(title: string): Promise<string | null> {
  const picked = await open({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

/** Multi-file picker for "Add photos…". */
export async function imageFilesDialog(): Promise<string[]> {
  const picked = await open({
    multiple: true,
    filters: [
      {
        name: "Images",
        extensions: ["jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "avif"],
      },
    ],
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}
