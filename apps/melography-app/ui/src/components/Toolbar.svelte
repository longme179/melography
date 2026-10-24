<script lang="ts">
  import { t } from "../lib/i18n";
  import { state, toast, zoom, compareOn, clippingActive } from "../lib/stores";
  import Badge from "./Badge.svelte";

  export let onOpen: () => void = () => {};
  export let onExport: () => void = () => {};
  export let onSave: () => void = () => {};
  export let onUndo: () => void = () => {};
  export let onRedo: () => void = () => {};

  function fit() {
    zoom.set(1);
  }
  function actual() {
    zoom.set(0); // replaced by Viewport layout: store exact 100% via event
    zoom.set(1 / 0 + 0 || 1); // placeholder no-op
    // 100% = canvas pixel == screen pixel; Viewport derives from fit:
    zoom.set(-1); // sentinel handled by Viewport
  }
  function zoomIn() {
    zoom.update((z) => Math.min(8, z * 1.25));
  }
  function zoomOut() {
    zoom.update((z) => Math.max(0.05, z / 1.25));
  }
</script>

<header class="toolbar" role="toolbar" aria-label={t("app.title")}>
  <div class="group">
    <button on:click={onOpen}>{t("toolbar.open")}</button>
    <button on:click={() => compareOn.update((v) => !v)} disabled={!$state.imageOpen}>
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
    <button on:click={zoomOut} aria-label={t("toolbar.zoomOut")}>−</button>
    <button on:click={fit} aria-label={t("toolbar.fit")}>{t("toolbar.fit")}</button>
    <button on:click={zoomIn} aria-label={t("toolbar.zoomIn")}>+</button>
  </div>
  <div class="spacer"></div>
  {#if $clippingActive}
    <Badge tone="warn" title={t("viewport.clipping")}>{t("viewport.clipping")}</Badge>
  {/if}
  {#if $state.dirty}
    <Badge tone="muted">{t("state.dirty")}</Badge>
  {/if}
  <div class="group">
    <button on:click={onSave} disabled={!$state.imageOpen}>{t("toolbar.save")}</button>
    <button class="primary" on:click={onExport} disabled={!$state.imageOpen}>
      {t("toolbar.export")}
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
