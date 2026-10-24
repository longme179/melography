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
    undo as undoCmd,
    redo as redoCmd,
    listStocks,
    getFilm,
  } from "./lib/ipc";
  import { t, setLang, type Lang } from "./lib/i18n";
  import { state, lastError, toast, compareOn, stocks, film } from "./lib/stores";
  import Toolbar from "./components/Toolbar.svelte";
  import Viewport from "./components/Viewport.svelte";
  import LutPanel from "./components/LutPanel.svelte";
  import FilmLab from "./components/FilmLab.svelte";
  import TonePanel from "./components/TonePanel.svelte";
  import ModelManager from "./components/ModelManager.svelte";
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
      const s = await getState();
      state.set(s);
      // Parameter settled with unsaved changes → schedule the debounced
      // autosave (M4 #24). Every mutation path ends in refreshState, so
      // this is the single settle point.
      if (s.dirty && s.imageOpen) scheduleAutosave();
      else if (!s.dirty) cancelAutosave();
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

  // ── Autosave (M4 #24, UX_SPEC §4: "autosave indicator (dot, never
  // modal)") ────────────────────────────────────────────────────────
  // Recipe mutations settle → debounced save_recipe; the toolbar's dirty
  // badge IS the indicator (appears while dirty, clears when saved). No
  // toasts, no modals. Explicit Ctrl+S cancels the pending timer (already
  // saved). Crash-safety: the save path is the shared durable primitive
  // (kill-battery-tested), so a kill mid-autosave leaves the previous
  // complete sidecar.
  let autosaveTimer: ReturnType<typeof setTimeout> | null = null;
  const AUTOSAVE_DELAY_MS = 900;

  function scheduleAutosave() {
    if (autosaveTimer !== null) clearTimeout(autosaveTimer);
    autosaveTimer = setTimeout(async () => {
      autosaveTimer = null;
      try {
        await saveRecipe();
        await refreshState(); // dirty badge clears — that's the whole UI
      } catch (e) {
        // Never a modal: the dirty badge stays; the error banner shows
        // the code + guidance (UX_SPEC §9).
        lastError.set(e as never);
      }
    }, AUTOSAVE_DELAY_MS);
  }

  function cancelAutosave() {
    if (autosaveTimer !== null) {
      clearTimeout(autosaveTimer);
      autosaveTimer = null;
    }
  }

  async function doSave() {
    cancelAutosave(); // explicit save supersedes the pending timer
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

  function onToneChanged() {
    viewport?.refresh();
    refreshState();
  }

  function onKey(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey) {
      if (e.key === "s") {
        e.preventDefault();
        doSave();
      } else if (e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        doUndo();
      } else if ((e.key === "z" || e.key === "Z") && e.shiftKey) {
        e.preventDefault();
        doRedo();
      } else if (e.key === "y" || e.key === "Y") {
        e.preventDefault();
        doRedo();
      }
      return;
    }
    if (e.key === "o") doOpen();
    else if (e.key === "e") exportOpen = $state.imageOpen;
  }

  /** Undo/redo (M4 #24, UX_SPEC §8 keyboard map): apply, refresh every
   * panel bound to recipe state, re-render the viewport. */
  async function doUndo() {
    try {
      state.set(await undoCmd());
      viewport?.refresh();
      // Panels mirror recipe state (film/tone/LUT) — re-fetch so the UI
      // never shows a stale recipe after a history jump.
      getFilm().then((f) => film.set(f)).catch(console.error);
      lutPanel?.reload();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doRedo() {
    try {
      state.set(await redoCmd());
      viewport?.refresh();
      getFilm().then((f) => film.set(f)).catch(console.error);
      lutPanel?.reload();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  onMount(() => {
    refreshState();
    // Film Lab (M2): registry + current film state.
    listStocks().then((s) => stocks.set(s)).catch(console.error);
    getFilm().then((f) => film.set(f)).catch(console.error);
    const h = window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      cancelAutosave(); // never save after unmount
    };
  });
</script>

<div class="app">
  <Toolbar
    onOpen={doOpen}
    onExport={() => (exportOpen = true)}
    onSave={doSave}
    onUndo={doUndo}
    onRedo={doRedo}
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
      <TonePanel onChange={onToneChanged} />
      <ModelManager />
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
