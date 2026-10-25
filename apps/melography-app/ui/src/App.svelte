<script lang="ts">
  // App shell (UX_SPEC §2/§4, M4 #23): S1 Home ⇆ S2 Editor with the full
  // panel rail (Basic / Color / Film Lab / LUT / Lens / Grain·Halation /
  // AI / Export), filmstrip, and the §8 keyboard map:
  //   O open · Ctrl+S save · B hold-compare · E export · [ ] cycle panels
  //   0 fit · 1 100% · 2 200% · +/− zoom · Space compare · Ctrl+Z/Y history
  import { onMount } from "svelte";
  import {
    openImageDialog,
    openImage,
    closeImage,
    getState,
    saveRecipe,
    undo as undoCmd,
    redo as redoCmd,
    listStocks,
    getFilm,
  } from "./lib/ipc";
  import { t, setLang, locale, type Lang } from "./lib/i18n";
  import { applyTheme } from "./lib/theme";
  import {
    state,
    lastError,
    toast,
    compareOn,
    stocks,
    film,
    missingStock,
    settings,
    batchJobs,
    batchPaused,
    wbPicker,
  } from "./lib/stores";
  import { getSettings, listenBatch } from "./lib/ipc";
  import Toolbar from "./components/Toolbar.svelte";
  import Viewport from "./components/Viewport.svelte";
  import Home from "./components/Home.svelte";
  import BatchQueue from "./components/BatchQueue.svelte";
  import BasicPanel from "./components/BasicPanel.svelte";
  import ColorPanel from "./components/ColorPanel.svelte";
  import FilmLab from "./components/FilmLab.svelte";
  import LutPanel from "./components/LutPanel.svelte";
  import LensPanel from "./components/LensPanel.svelte";
  import GrainHalationPanel from "./components/GrainHalationPanel.svelte";
  import TonePanel from "./components/TonePanel.svelte";
  import ModelManager from "./components/ModelManager.svelte";
  import StockBrowser from "./components/StockBrowser.svelte";
  import ExportDialog from "./components/ExportDialog.svelte";
  import ImportLutDialog from "./components/ImportLutDialog.svelte";
  import PresetManager from "./components/PresetManager.svelte";
  import Settings from "./components/Settings.svelte";
  import Filmstrip from "./components/Filmstrip.svelte";
  import Badge from "./components/Badge.svelte";
  import ErrorBanner from "./components/ErrorBanner.svelte";

  /** Panel rail order = UX_SPEC §4 (tab order fixed). */
  const PANELS = [
    "basic",
    "color",
    "film",
    "lut",
    "lens",
    "grain",
    "ai",
  ] as const;
  type PanelId = (typeof PANELS)[number];
  const panelLabel: Record<PanelId, string> = {
    basic: "panel.basic",
    color: "panel.color",
    film: "panel.film",
    lut: "panel.lut.title",
    lens: "panel.lens",
    grain: "panel.grain",
    ai: "panel.ai",
  };

  let activePanel: PanelId = "basic";
  let exportOpen = false;
  let importOpen = false;
  let presetsOpen = false;
  let settingsOpen = false;
  let stockBrowserOpen = false;
  /** S6 screen (session 08): the queue is a VIEW over the worker — the
   *  editor session underneath stays intact and keeps rendering. */
  let showBatch = false;
  let viewport: Viewport;
  let lutPanel: LutPanel;
  let filmstrip: Filmstrip;

  $: showHome = !$state.imageOpen;

  async function refreshState() {
    try {
      const s = await getState();
      state.set(s);
      missingStock.set(s.missingStock);
      if (s.dirty && s.imageOpen) scheduleAutosave();
      else if (!s.dirty) cancelAutosave();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doOpenPath(path: string) {
    showBatch = false; // opening a photo goes to the editor
    try {
      await openImage(path);
      await refreshState();
      await refreshRecipePanels();
      await viewport?.setViewportSize(
        window.innerWidth - 300,
        window.innerHeight - 120,
      );
      filmstrip?.refresh();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doOpen() {
    const path = await openImageDialog();
    if (!path) return;
    await doOpenPath(path);
  }

  /** Every history jump / preset apply / open lands here: panels mirror
   * recipe state — the UI never shows a stale recipe. */
  async function refreshRecipePanels() {
    getFilm().then((f) => film.set(f)).catch(console.error);
    lutPanel?.reload();
  }

  // ── Autosave (M4 #24, UX_SPEC §4 "dot, never modal") ────────────────
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
    cancelAutosave();
    try {
      await saveRecipe();
      toast.set(t("toolbar.saved"));
      await refreshState();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  /** Home (toolbar ◀) — close the session, keep the library. */
  async function goHome() {
    cancelAutosave();
    try {
      await closeImage();
    } catch {
      /* closing without a session is fine */
    }
    await refreshState();
  }

  function onLutChanged() {
    viewport?.refresh();
    refreshState();
  }

  function onToneChanged() {
    viewport?.refresh();
    refreshState();
  }

  /** Panel cycling (UX_SPEC §8 `[` / `]`). */
  function cyclePanel(dir: 1 | -1) {
    const i = PANELS.indexOf(activePanel);
    activePanel = PANELS[(i + dir + PANELS.length) % PANELS.length];
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
    // Typing in an input/textarea must not trigger the map.
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === "Escape") {
      if ($wbPicker) {
        wbPicker.set(false);
      } else if (showBatch) {
        showBatch = false;
      }
      return;
    }
    switch (e.key) {
      case "o":
        doOpen();
        break;
      case "e":
        if ($state.imageOpen) exportOpen = true;
        break;
      case "[":
        cyclePanel(-1);
        break;
      case "]":
        cyclePanel(1);
        break;
      case "0":
        viewport?.zoomPercent(0); // fit
        break;
      case "1":
        viewport?.zoomPercent(1); // 100% pixel-exact
        break;
      case "2":
        viewport?.zoomPercent(2); // 200%
        break;
      case "+":
      case "=":
        viewport?.zoomBy(1.25);
        break;
      case "-":
        viewport?.zoomBy(1 / 1.25);
        break;
      case " ":
        if ($state.imageOpen) {
          e.preventDefault();
          compareOn.update((v) => !v);
        }
        break;
    }
  }

  async function doUndo() {
    try {
      state.set(await undoCmd());
      viewport?.refresh();
      await refreshRecipePanels();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function doRedo() {
    try {
      state.set(await redoCmd());
      viewport?.refresh();
      await refreshRecipePanels();
    } catch (e) {
      lastError.set(e as never);
    }
  }

  /** A preset replaced the whole recipe — refresh everything bound to it. */
  async function onPresetApplied() {
    viewport?.refresh();
    await refreshState();
    await refreshRecipePanels();
  }

  /** Home multi-drop (session 08): queue the handful, jump to S6. The
   * look defaults to the current recipe (or a straight convert when no
   * image is open) — the adder lets the user switch before adding. */
  function onBatchFiles(paths: string[]) {
    batchQueueFromHome = paths;
    showBatch = true;
  }

  let batchQueueFromHome: string[] | null = null;

  onMount(async () => {
    refreshState();
    listStocks().then((s) => stocks.set(s)).catch(console.error);
    getFilm().then((f) => film.set(f)).catch(console.error);
    // Worker events → the queue stores (S6 stays live on any screen).
    listenBatch((snap) => {
      batchJobs.set(snap.jobs);
      batchPaused.set(snap.paused);
    }).then((un) => {
      unlistenBatch = un;
    });
    // Settings drive language + theme at startup (S7).
    try {
      const s = await getSettings();
      settings.set(s);
      setLang(s.language as Lang);
      applyTheme(s.theme);
    } catch {
      /* defaults */
    }
    const h = window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      unlistenBatch?.();
      cancelAutosave(); // never save after unmount
    };
  });
  let unlistenBatch: (() => void) | null = null;
</script>

<div class="app">
  <Toolbar
    onOpen={doOpen}
    onHome={goHome}
    onExport={() => (exportOpen = true)}
    onSave={doSave}
    onUndo={doUndo}
    onRedo={doRedo}
    onPresets={() => (presetsOpen = true)}
    onSettings={() => (settingsOpen = true)}
    onBatch={() => (showBatch = !showBatch)}
    onZoomIn={() => viewport?.zoomBy(1.25)}
    onZoomOut={() => viewport?.zoomBy(1 / 1.25)}
    onActual={() => viewport?.zoomPercent(1)}
  />
  <ErrorBanner />
  {#if $missingStock}
    <div class="warnline" role="status">
      {t("badge.missingStock", { stock: $missingStock })}
    </div>
  {/if}
  <main>
    {#if showBatch}
      <BatchQueue
        pendingFiles={batchQueueFromHome}
        onPendingConsumed={() => (batchQueueFromHome = null)}
        onBack={() => (showBatch = false)}
      />
    {:else if showHome}
      <Home onOpenPath={doOpenPath} onOpenDialog={doOpen} onBatchFiles={onBatchFiles} />
    {:else}
      <div class="editor">
        <Viewport
          bind:this={viewport}
          imageOpen={$state.imageOpen}
          imageKey={$state.sourcePath ?? ""}
          onWbPicked={refreshState}
        />
        <Filmstrip bind:this={filmstrip} visible={$state.imageOpen} onStockPicked={onLutChanged} />
      </div>
      <aside class="rail" aria-label={t(panelLabel[activePanel])}>
        <div class="rail-tabs" role="tablist" aria-label={t("app.title")}>
          {#each PANELS as p (p)}
            <button
              role="tab"
              aria-selected={activePanel === p}
              tabindex={activePanel === p ? 0 : -1}
              on:click={() => (activePanel = p)}
            >
              {t(panelLabel[p])}
            </button>
          {/each}
        </div>
        <div class="rail-body" role="tabpanel" aria-label={t(panelLabel[activePanel])}>
          {#key $locale}
            {#if activePanel === "basic"}
              <BasicPanel onChange={onToneChanged} />
            {:else if activePanel === "color"}
              <ColorPanel onChange={onToneChanged} />
            {:else if activePanel === "film"}
              <div class="film-wrap">
                <div class="rail-head">
                  <h2>{t("panel.film")}</h2>
                  <button class="link" on:click={() => (stockBrowserOpen = true)}>
                    {t("stock.browse")}
                  </button>
                </div>
                <!-- A1: one render path — film mutations (knobs AND stock
                     select) refresh through the App-level viewport, like
                     every other panel; FilmLab never renders by itself. -->
                <FilmLab onChange={onLutChanged} />
              </div>
            {:else if activePanel === "lut"}
              <LutPanel bind:this={lutPanel} onChange={onLutChanged} />
              <div class="rail-actions">
                <button on:click={() => (importOpen = true)}>
                  {t("panel.lut.import")}
                </button>
              </div>
            {:else if activePanel === "lens"}
              <LensPanel onChange={onLutChanged} />
            {:else if activePanel === "grain"}
              <GrainHalationPanel onChange={onLutChanged} />
            {:else if activePanel === "ai"}
              <TonePanel onChange={onToneChanged} />
              <ModelManager />
            {/if}
          {/key}
        </div>
      </aside>
    {/if}
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
      <!-- role+aria-live: SR users hear "Recipe saved"-style confirmations
           (UX_SPEC §8 announcements; #15). -->
      <span class="toast" role="status" aria-live="polite">{$toast}</span>
    {/if}
  </footer>
</div>

<ExportDialog
  bind:open={exportOpen}
  imageName={$state.imageName}
  sourcePath={$state.sourcePath}
/>
<ImportLutDialog
  bind:open={importOpen}
  onDone={() => {
    lutPanel?.reload();
    onLutChanged();
  }}
/>
<PresetManager bind:open={presetsOpen} canApply={$state.imageOpen} onApplied={onPresetApplied} />
<Settings bind:open={settingsOpen} />
{#if !$state.imageOpen}
  <StockBrowser bind:open={stockBrowserOpen} />
{/if}

<style>
  .app {
    display: grid;
    grid-template-rows: auto auto 1fr auto;
    height: 100%;
  }
  main {
    display: grid;
    grid-template-columns: 1fr 300px;
    min-height: 0;
  }
  main:has(.home) {
    grid-template-columns: 1fr;
  }
  .editor {
    display: grid;
    grid-template-rows: 1fr auto;
    min-height: 0;
    min-width: 0;
  }
  .rail {
    background: var(--bg-panel);
    border-left: 1px solid var(--stroke-subtle);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .rail-body {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
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
  .rail-head .link {
    background: none;
    border: none;
    color: var(--info);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  .rail-actions {
    padding: var(--space-3);
    border-top: 1px solid var(--stroke-subtle);
  }
  .warnline {
    background: color-mix(in srgb, var(--warn) 14%, var(--bg-panel));
    color: var(--warn);
    border-bottom: 1px solid var(--warn);
    padding: 4px 12px;
    font-size: 12px;
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
