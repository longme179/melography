<script lang="ts">
  /** Toolbar (UX_SPEC §4): Home / open / compare / undo·redo / zoom group /
   *  clipping + dirty badges / save / export + the S5/S7 entries
   *  (Preset manager, Settings). Zoom goes through the Viewport's exact
   *  scale API — no sentinel hacks. */
  import { t } from "../lib/i18n";
  import {
    state,
    toast,
    zoom,
    compareOn,
    clippingActive,
    batchActiveCount,
  } from "../lib/stores";
  import Badge from "./Badge.svelte";

  export let onOpen: () => void = () => {};
  export let onHome: () => void = () => {};
  export let onExport: () => void = () => {};
  export let onSave: () => void = () => {};
  export let onUndo: () => void = () => {};
  export let onRedo: () => void = () => {};
  export let onPresets: () => void = () => {};
  export let onSettings: () => void = () => {};
  export let onBatch: () => void = () => {};

  export let onZoomIn: () => void = () => {};
  export let onZoomOut: () => void = () => {};
  export let onFit: () => void = () => {};
  export let onActual: () => void = () => {};

  function fit() {
    zoom.set(1);
    onFit();
  }
</script>

<header class="toolbar" role="toolbar" aria-label={t("app.title")}>
  <div class="group">
    <button on:click={onHome} title={t("home.title")} aria-label={t("home.title")}>
      ◀
    </button>
    <button on:click={onOpen}>{t("toolbar.open")}</button>
    <button
      on:click={() => compareOn.update((v) => !v)}
      disabled={!$state.imageOpen}
    >
      {t("toolbar.compare")}
    </button>
  </div>
  <div class="group">
    <button
      on:click={onUndo}
      disabled={!$state.canUndo}
      aria-label={t("toolbar.undo")}
      title={t("toolbar.undo") + " (Ctrl+Z)"}
    >
      ↩
    </button>
    <button
      on:click={onRedo}
      disabled={!$state.canRedo}
      aria-label={t("toolbar.redo")}
      title={t("toolbar.redo") + " (Ctrl+Shift+Z)"}
    >
      ↪
    </button>
  </div>
  <div class="group">
    <button on:click={onZoomOut} aria-label={t("toolbar.zoomOut")}>−</button>
    <button on:click={fit} aria-label={t("toolbar.fit")}>{t("toolbar.fit")}</button>
    <button on:click={onActual} aria-label={t("toolbar.actual")}>{t("toolbar.actual")}</button>
    <button on:click={onZoomIn} aria-label={t("toolbar.zoomIn")}>+</button>
  </div>
  <div class="spacer"></div>
  {#if $clippingActive}
    <Badge tone="warn" title={t("viewport.clipping")}>{t("viewport.clipping")}</Badge>
  {/if}
  {#if $state.dirty}
    <Badge tone="muted">{t("state.dirty")}</Badge>
  {/if}
  <div class="group">
    <button on:click={onPresets} disabled={!$state.imageOpen}>
      {t("presets.open")}
    </button>
    <button on:click={onSettings}>{t("settings.open")}</button>
    <button on:click={onSave} disabled={!$state.imageOpen}>{t("toolbar.save")}</button>
    <button class="primary" on:click={onExport} disabled={!$state.imageOpen}>
      {t("toolbar.export")}
    </button>
    <button on:click={onBatch}>
      {t("batch.open")}
      {#if $batchActiveCount > 0}
        <Badge tone="info">{$batchActiveCount}</Badge>
      {/if}
    </button>
  </div>
</header>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--bg-panel);
    border-bottom: 1px solid var(--stroke-subtle);
  }
  .group {
    display: flex;
    gap: var(--space-1);
  }
  .spacer {
    flex: 1;
  }
</style>
