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

  export let visible = false;
  export let onStockPicked: () => void = () => {};

  let originalUrl: string | null = null;

  function rgbaToDataUrl(w: number, h: number, rgba: Uint8ClampedArray): string {
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (!ctx) return "";
    // NOTE: this is blitting Rust-produced pixels for a UI glyph, not
    // computing any color — ADR-0001's "webview never computes pixels"
    // is about the pipeline, and the same pattern serves Viewport.
    ctx.putImageData(new ImageData(rgba, w, h), 0, 0);
    return canvas.toDataURL("image/png");
  }

  async function refreshOriginal() {
    try {
      const f = await originalFrame(96);
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
