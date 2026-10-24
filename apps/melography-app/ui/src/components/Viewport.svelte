<script lang="ts">
  // The viewport: canvas blit of Rust-produced RGBA frames, pan/zoom,
  // split compare (drag the divider), hold-B original (UX_SPEC §4).
  // No pixel math here — blit only (ADR-0001).
  import { onMount, onDestroy } from "svelte";
  import {
    renderPreview,
    originalFrame,
    wbPick,
    type PreviewFrame,
  } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import {
    compareOn,
    compareSplit,
    zoom,
    tone,
    toast,
    wbPicker,
    type ToneState,
  } from "../lib/stores";

  export let maxDim = 1600;
  export let imageOpen = false;
  /** Fired after a WB pick so the shell refreshes state (dirty badge). */
  export let onWbPicked: () => void = () => {};

  let canvas: HTMLCanvasElement;
  let originalCanvas: HTMLCanvasElement;
  let container: HTMLElement;

  let edited: PreviewFrame | null = null;
  let original: PreviewFrame | null = null;
  let renderMs = 0;
  let showOriginal = false; // hold-B
  let panX = 0;
  let panY = 0;
  let dragging = false;
  let draggingDivider = false;
  let splitX = 0.5;
  let scale = 1;
  let lastFit = 1; // fit-scale at the current window size (100% = 1/lastFit)
  let loupeZoom = false; // hold-Z: temporary 100% (UX_SPEC §8)
  let zoomBeforeLoupe = 1;

  $: if (imageOpen) {
    refresh();
  }

  export function setViewportSize(w: number, h: number) {
    maxDim = Math.max(256, Math.min(2560, Math.ceil(Math.max(w, h))));
    refresh();
  }

  export async function refresh() {
    if (!imageOpen) return;
    try {
      edited = await renderPreview(maxDim);
      renderMs = edited.renderMs;
      original = await originalFrame(maxDim);
      blit();
    } catch (e) {
      console.error("preview render failed", e);
    }
  }

  function blit() {
    if (!edited || !canvas) return;
    const active = showOriginal && original ? original : edited;
    canvas.width = active.width;
    canvas.height = active.height;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.putImageData(new ImageData(active.rgba, active.width, active.height), 0, 0);

    if (original && edited) {
      originalCanvas.width = original.width;
      originalCanvas.height = original.height;
      const octx = originalCanvas.getContext("2d");
      if (octx) {
        octx.putImageData(
          new ImageData(original.rgba, original.width, original.height),
          0,
          0,
        );
      }
    }
    layout();
  }

  function layout() {
    if (!container || !edited) return;
    const cw = container.clientWidth;
    const ch = container.clientHeight;
    const fit = Math.min(cw / edited.width, ch / edited.height);
    lastFit = fit;
    const activeZoom = loupeZoom ? 1 / lastFit : $zoom;
    scale = fit * activeZoom;
    const w = edited.width * scale;
    const h = edited.height * scale;
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;
    if (originalCanvas) {
      originalCanvas.style.width = `${w}px`;
      originalCanvas.style.height = `${h}px`;
    }
    const holder = canvas.parentElement as HTMLElement;
    holder.style.transform = `translate(${panX}px, ${panY}px)`;
    // divider position within image bounds
    const divider = holder.querySelector(".divider") as HTMLElement | null;
    if (divider) divider.style.left = `${w * splitX}px`;
    const clip = originalCanvas?.parentElement;
    if (clip) clip.style.width = `${w * splitX}px`;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
    zoom.update((z) => Math.min(8, Math.max(0.05, z * factor)));
    layout();
  }

  // ── Zoom API (M4 #23 keyboard map + toolbar; exact, no sentinels) ───
  /** Absolute zoom as a scale factor (1 = fit). */
  export function setZoomScale(s: number) {
    zoom.set(s);
    layout();
  }

  /** Relative zoom (× factor, clamped). */
  export function zoomBy(f: number) {
    zoom.update((z) => Math.min(8, Math.max(0.05, z * f)));
    layout();
  }

  /** Pixel-exact percentage: 1 = 100% (one canvas px == one screen px). */
  export function zoomPercent(p: number) {
    if (p <= 0) {
      zoom.set(1); // fit
    } else {
      zoom.set(p / Math.max(lastFit, 1e-6));
    }
    layout();
  }

  function onPointerDown(e: PointerEvent) {
    if ($wbPicker && imageOpen && canvas) {
      // WB eyedropper (UX_SPEC §6): normalized coords — Rust maps them
      // onto the full-resolution working image; the webview never
      // touches pixels (ADR-0001).
      const rect = canvas.getBoundingClientRect();
      const nx = (e.clientX - rect.left) / Math.max(rect.width, 1);
      const ny = (e.clientY - rect.top) / Math.max(rect.height, 1);
      pickWb(nx, ny);
      return;
    }
    if ((e.target as HTMLElement).classList.contains("divider")) {
      draggingDivider = true;
    } else {
      dragging = true;
      panX += 0; // anchor
    }
    container.setPointerCapture(e.pointerId);
  }

  async function pickWb(nx: number, ny: number) {
    wbPicker.set(false);
    try {
      const s: ToneState = await wbPick(nx, ny);
      tone.set(s);
      toast.set(t("wb.picked"));
      await refresh(); // the new gains are live in the preview
      onWbPicked();
    } catch (e) {
      console.error("wb pick failed", e);
    }
  }

  function onPointerMove(e: PointerEvent) {
    if (draggingDivider) {
      const rect = (
        canvas.parentElement as HTMLElement
      ).getBoundingClientRect();
      splitX = Math.min(
        1,
        Math.max(0, (e.clientX - rect.left) / rect.width),
      );
      compareSplit.set(splitX);
      layout();
    } else if (dragging) {
      panX += e.movementX;
      panY += e.movementY;
      layout();
    }
  }

  function onPointerUp() {
    dragging = false;
    draggingDivider = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && $wbPicker) {
      wbPicker.set(false);
      return;
    }
    if (e.key === "b" || e.key === "B") {
      showOriginal = true;
      blit();
    } else if (e.key === "z" || e.key === "Z") {
      // Hold-Z 100% loupe (UX_SPEC §8): temporary pixel-exact view.
      if (!loupeZoom) {
        loupeZoom = true;
        zoomBeforeLoupe = $zoom;
        layout();
      }
    }
  }
  function onKeyUp(e: KeyboardEvent) {
    if (e.key === "b" || e.key === "B") {
      showOriginal = false;
      blit();
    } else if (e.key === "z" || e.key === "Z") {
      loupeZoom = false;
      zoom.set(zoomBeforeLoupe);
      layout();
    }
  }

  onMount(() => {
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    const ro = new ResizeObserver(() => {
      if (container) setViewportSize(container.clientWidth, container.clientHeight);
    });
    if (container) ro.observe(container);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
      ro.disconnect();
    };
  });
</script>

<svelte:window on:resize={layout} />

<div
  class="viewport"
  class:picker={$wbPicker}
  bind:this={container}
  on:wheel|preventDefault={onWheel}
  on:pointerdown={onPointerDown}
  on:pointermove={onPointerMove}
  on:pointerup={onPointerUp}
  role="img"
  aria-label={imageOpen ? t("viewport.edited") : t("viewport.dropHere")}
  tabindex="0"
>
  {#if !imageOpen}
    <div class="empty">
      <p>{t("viewport.dropHere")}</p>
    </div>
  {:else}
    {#if $wbPicker}
      <div class="pick-banner" role="status">{t("wb.picking")}</div>
    {/if}
    <div class="stack" style="position:relative">
      <canvas bind:this={canvas}></canvas>
      {#if $compareOn && original}
        <div class="split-clip" style="position:absolute;inset:0;overflow:hidden">
          <div style="position:relative">
            <canvas bind:this={originalCanvas}></canvas>
            <div class="divider" style="position:absolute;top:0;bottom:0;width:2px;background:var(--accent-400);cursor:col-resize"></div>
          </div>
        </div>
        <span class="label left">{t("viewport.original")}</span>
        <span class="label right">{t("viewport.edited")}</span>
      {/if}
    </div>
    <div class="status mono">
      <span>{renderMs.toFixed(1)} ms {t("viewport.renderMs")}</span>
      <span>·</span>
      <span>{Math.round($zoom * 100)}%{loupeZoom ? " · " + t("viewport.loupe") : ""}</span>
      <span aria-hidden="true">·</span>
      <span class="hint">{t("viewport.holdB")} · Z 100%</span>
    </div>
  {/if}
</div>

<style>
  .viewport {
    position: relative;
    height: 100%;
    background: var(--bg-viewport);
    display: grid;
    place-items: center;
    overflow: hidden;
    cursor: grab;
  }
  .viewport:active {
    cursor: grabbing;
  }
  .viewport.picker {
    cursor: crosshair;
  }
  .pick-banner {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    font-size: 12px;
    color: var(--accent-400);
    background: rgba(14, 15, 17, 0.85);
    border: 1px solid var(--accent-400);
    padding: 2px 12px;
    border-radius: 999px;
    z-index: 5;
    pointer-events: none;
  }
  .empty p {
    color: var(--fg-tertiary);
    font-size: 15px;
  }
  .stack canvas {
    display: block;
    image-rendering: auto;
    box-shadow: 0 0 0 1px var(--stroke-subtle);
  }
  .status {
    position: absolute;
    bottom: 8px;
    left: 12px;
    display: flex;
    gap: 8px;
    color: var(--fg-tertiary);
    font-size: 12px;
    background: color-mix(in srgb, var(--bg-viewport) 80%, transparent);
    padding: 2px 8px;
    border-radius: var(--radius);
  }
  .label {
    position: absolute;
    top: 8px;
    font-size: 11px;
    color: var(--fg-secondary);
    background: rgba(14, 15, 17, 0.8);
    padding: 1px 8px;
    border-radius: 999px;
  }
  .label.left { left: 8px; }
  .label.right { right: 8px; }
  .hint { color: var(--fg-disabled); }
</style>
