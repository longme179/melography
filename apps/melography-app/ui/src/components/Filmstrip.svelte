<script lang="ts">
  /** Filmstrip (UX_SPEC §4, M4 #23): the ORIGINAL plus this photo through
   *  every stock, live. Clicking a frame selects that stock (one tap =
   *  one look — persona P1's core loop). Renders are Rust-side
   *  (`stock_thumbs`); the webview only binds data URLs. */
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../lib/i18n";
  import { film, stocks, stockThumbs, type StockThumb, type FilmState } from "../lib/stores";
  import { originalFrame } from "../lib/ipc";
  import { putRgba } from "../lib/blit";

  export let visible = false;
  export let onStockPicked: () => void = () => {};

  let originalUrl: string | null = null;

  function rgbaToDataUrl(w: number, h: number, rgba: Uint8ClampedArray<ArrayBuffer>): string {
    const canvas = document.createElement("canvas");
    // Single blit path (session 15): shares the validated RGBA→canvas
    // code with the viewport — a mismatched/truncated frame yields an
    // empty string (honest empty strip) instead of a canvas exception.
    if (!putRgba(canvas, rgba, w, h)) return "";
    return canvas.toDataURL("image/png");
  }

  async function refreshOriginal() {
    try {
      // A1: the preview transport is validated at the command boundary
      // (256..=2560) — the strip's "original" glyph asks for the minimum
      // bound; the browser scales it down for display (crisper on HiDPI).
      const f = await originalFrame(256);
      originalUrl = rgbaToDataUrl(f.width, f.height, f.rgba);
    } catch {
      originalUrl = null; // no session — the strip is empty, honestly
    }
  }

  async function refreshStocks() {
    try {
      const thumbs: StockThumb[] = await invoke("stock_thumbs", { maxDim: 96 });
      stockThumbs.set(thumbs);
    } catch {
      stockThumbs.set([]);
    }
  }

  export async function refresh() {
    await Promise.all([refreshOriginal(), refreshStocks()]);
  }

  async function pick(stockId: string | null) {
    try {
      const next = await invoke<FilmState>("set_film", {
        args: { stock: stockId },
      });
      film.set(next);
      onStockPicked();
    } catch (e) {
      console.error(e);
    }
  }

  $: if (visible) refresh();
</script>

{#if visible}
  <div class="filmstrip" role="listbox" aria-label={t("filmstrip.title")}>
    <span class="hint">{t("filmstrip.title")}</span>
    {#if originalUrl}
      <div
        class="frame"
        class:active={!$film?.stock}
        role="option"
        aria-selected={!$film?.stock}
        tabindex="0"
        title={t("filmstrip.original")}
        on:click={() => pick(null)}
        on:keydown={(e) => e.key === "Enter" && pick(null)}
      >
        <img src={originalUrl} alt={t("filmstrip.original")} />
        <span class="tag">{t("filmstrip.original")}</span>
      </div>
    {/if}
    {#each $stockThumbs as th (th.stock_id)}
      {@const name = $stocks.find((s) => s.id === th.stock_id)?.display_name ?? th.stock_id}
      <div
        class="frame"
        class:active={$film?.stock === th.stock_id}
        role="option"
        aria-selected={$film?.stock === th.stock_id}
        tabindex="0"
        title={name}
        on:click={() => pick(th.stock_id)}
        on:keydown={(e) => e.key === "Enter" && pick(th.stock_id)}
      >
        <img src={th.thumb_data_url} alt={name} />
        <span class="tag">{name}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
</style>
