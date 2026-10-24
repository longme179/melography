<script lang="ts">
  // App shell (S2 editor MVP layout, UX_SPEC §4): toolbar / viewport /
  // right panel rail / status bar. Keyboard map: O open · B hold-compare ·
  // Ctrl+S save recipe · E export (partial M4 polish deferred).
  import { onMount } from "svelte";
  import {
    openImageDialog,
    openImage,
    getState,
    saveRecipe,
    listStocks,
    getFilm,
  } from "./lib/ipc";
  import { t, setLang, type Lang } from "./lib/i18n";
  import { state, lastError, toast, compareOn, stocks, film } from "./lib/stores";
  import Toolbar from "./components/Toolbar.svelte";
  import Viewport from "./components/Viewport.svelte";
  import LutPanel from "./components/LutPanel.svelte";
  import FilmLab from "./components/FilmLab.svelte";
  import StockBrowser from "./components/StockBrowser.svelte";
  import ExportDialog from "./components/ExportDialog.svelte";
  import ImportLutDialog from "./components/ImportLutDialog.svelte";
  import ErrorBanner from "./components/ErrorBanner.svelte";

  let exportOpen = false;
  let importOpen = false;
  let viewport: Viewport;
  let lutPanel: LutPanel;
  let stockBrowserOpen = false;
  let lang: Lang = "en";

  async function refreshState() {
    try {
      state.set(await getState());
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doOpen() {
    const path = await openImageDialog();
    if (!path) return;
    try {
      await openImage(path);
      await refreshState();
      await viewport?.setViewportSize(
        window.innerWidth - 300,
        window.innerHeight - 120,
      );
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doSave() {
    try {
      await saveRecipe();
      toast.set(t("toolbar.saved"));
      await refreshState();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  function onLutChanged() {
    viewport?.refresh();
    refreshState();
  }

  function onKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey) {
      if (e.key === "s") {
        e.preventDefault();
        doSave();
      }
      return;
    }
    if (e.key === "o") doOpen();
    else if (e.key === "e") exportOpen = $state.imageOpen;
  }

  onMount(() => {
    refreshState();
    // Film Lab (M2): registry + current film state.
    listStocks().then((s) => stocks.set(s)).catch(console.error);
    getFilm().then((f) => film.set(f)).catch(console.error);
    const h = window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  });
</script>

<div class="app">
  <Toolbar
    onOpen={doOpen}
    onExport={() => (exportOpen = true)}
    onSave={doSave}
  />
  <ErrorBanner />
  <main>
    <Viewport bind:this={viewport} imageOpen={$state.imageOpen} />
    <aside class="rail" aria-label={t("panel.lut")}>
      <div class="rail-head">
        <h2>{t("panel.lut")}</h2>
        <select
          bind:value={lang}
          on:change={() => setLang(lang)}
          aria-label="Language"
        >
          <option value="en">EN</option>
          <option value="vi">VI</option>
        </select>
      </div>
      <LutPanel bind:this={lutPanel} onChange={onLutChanged} />
      <div class="rail-actions">
        <button on:click={() => (importOpen = true)}>
          {t("panel.lut.import")}
        </button>
      </div>
      <div class="rail-head film-head">
        <h2>Film Lab</h2>
        <button class="link" on:click={() => (stockBrowserOpen = true)}>
          Browse stocks…
        </button>
      </div>
      <FilmLab />
      <StockBrowser bind:open={stockBrowserOpen} />
    </aside>
  </main>
  <footer class="statusbar mono">
    {#if $state.imageName}
      <span>{$state.imageName}</span>
      {#if $state.imageDims}
        <span>·</span>
        <span>{$state.imageDims[0]}×{$state.imageDims[1]}</span>
      {/if}
      {#if $state.sidecarPath}
        <span>·</span>
        <span class="muted">{t("state.sidecar")}</span>
      {/if}
    {/if}
    {#if $toast}
      <span class="toast">{$toast}</span>
    {/if}
  </footer>
</div>

<ExportDialog bind:open={exportOpen} imageName={$state.imageName} />
<ImportLutDialog
  bind:open={importOpen}
  onDone={() => {
    lutPanel?.reload();
    onLutChanged();
  }}
/>

<style>
  .app {
    display: grid;
    grid-template-rows: auto auto 1fr auto;
    height: 100%;
  }
  main {
    display: grid;
    grid-template-columns: 1fr 280px;
    min-height: 0;
  }
  .rail {
    background: var(--bg-panel);
    border-left: 1px solid var(--stroke-subtle);
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
  }
  .rail-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-3) 0;
  }
  .rail-head h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }
  .rail-actions {
    padding: var(--space-3);
    border-top: 1px solid var(--stroke-subtle);
    margin-top: auto;
  }
  .statusbar {
    display: flex;
    gap: var(--space-2);
    padding: 4px 12px;
    background: var(--bg-panel);
    border-top: 1px solid var(--stroke-subtle);
    color: var(--fg-secondary);
    font-size: 12px;
  }
  .muted {
    color: var(--fg-tertiary);
  }
  .toast {
    margin-left: auto;
    color: var(--ok);
  }
</style>
