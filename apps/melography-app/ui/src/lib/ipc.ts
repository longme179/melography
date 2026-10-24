// Typed IPC wrappers — the ONLY bridge to Rust (ARCHITECTURE §1: params
// in, preview frames out; the webview never computes pixels).

import { invoke } from "@tauri-apps/api/core";
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
  rgba: Uint8ClampedArray;
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
}

export interface AppErrorPayload {
  code: string;
  message: string;
  context: string | null;
}

function parseFrame(buf: ArrayBuffer): PreviewFrame {
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
  // tauri v2: a command returning ipc::Response yields ArrayBuffer.
  const buf = await invoke<ArrayBuffer>("render_preview", { maxDim });
  return parseFrame(buf);
}

export async function originalFrame(maxDim: number): Promise<PreviewFrame> {
  const buf = await invoke<ArrayBuffer>("original_frame", { maxDim });
  return parseFrame(buf);
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
