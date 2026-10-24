<script lang="ts">
  // Export dialog (UX_SPEC §6 Export panel slice): per-format factual
  // capability matrix — impossible combos disabled, never converted.
  // "Add to batch queue" (S6, session 08): same spec, rendered by the
  // queue worker — the editor stays free.
  import { exportImage, exportDialog, batchAdd, directoryDialog } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import { lastError, toast } from "../lib/stores";

  export let open = false;
  export let imageName: string | null = null;
  /** Absolute source path (S6 add-to-batch needs the file, not the session). */
  export let sourcePath: string | null = null;
  export let onDone: () => void = () => {};

  // Esc closes the dialog (#15 a11y — every dialog is keyboard-dismissable;
  // never while an export is in flight).
  function onWindowKey(e: KeyboardEvent) {
    if (open && !busy && e.key === "Escape") open = false;
  }

  let format = "png";
  let depth = 16;
  let quality = 92;
  let busy = false;

  $: depths =
    format === "png" ? [8, 16] : format === "jpeg" ? [8] : [16];

  $: if (format === "jpeg") depth = 8;
  else if (format === "tiff") depth = 16;
  else if (!depths.includes(depth)) depth = 16;

  async function doExport() {
    if (busy) return;
    busy = true;
    try {
      const dest = await exportDialog(
        (imageName ?? "export").replace(/\.[^.]+$/, "") + "-melography",
        format,
      );
      if (!dest) return;
      const report = await exportImage({ format, depth, quality }, dest);
      if (Array.isArray((report as { warnings?: string[] }).warnings)) {
        // Honest state (UX_SPEC §9): e.g. LUT skipped by the film chain.
        toast.set(t("batch.warn.lutSkipped"));
      } else {
        toast.set(t("export.done"));
      }
      open = false;
      onDone();
    } catch (e) {
      lastError.set(e as never);
    } finally {
      busy = false;
    }
  }

  /** S6 flow: pick a folder, the queue worker renders it — this image
   * or any other keeps editing meanwhile. */
  async function addToBatch() {
    if (busy || !sourcePath) return;
    busy = true;
    try {
      const dir = await directoryDialog(t("batch.outFolder"));
      if (!dir) return;
      await batchAdd([sourcePath], null, { format, depth, quality }, dir);
      toast.set(t("export.queued"));
      open = false;
      onDone();
    } catch (e) {
      lastError.set(e as never);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window on:keydown={onWindowKey} />

{#if open}
  <div class="overlay" role="dialog" aria-modal="true" aria-label={t("export.title")}>
    <div class="dialog">
      <h2>{t("export.title")}</h2>

      <label class="field">
        {t("export.format")}
        <select bind:value={format}>
          <option value="png">PNG</option>
          <option value="jpeg">JPEG</option>
          <option value="tiff">TIFF</option>
        </select>
      </label>

      <label class="field">
        {t("export.depth")}
        <select bind:value={depth}>
          {#each depths as d}
            <option value={d}>{d}-bit</option>
          {/each}
        </select>
      </label>

      {#if format === "jpeg"}
        <label class="field">
          {t("export.quality")}
          <input type="range" min="1" max="100" bind:value={quality} />
          <span class="mono">{quality}</span>
        </label>
      {/if}

      <div class="actions">
        <button on:click={() => (open = false)}>{t("export.cancel")}</button>
        <button disabled={busy || !sourcePath} on:click={addToBatch}>
          {t("export.toQueue")}
        </button>
        <button class="primary" disabled={busy} on:click={doExport}>
          {t("export.save")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(8, 9, 10, 0.6);
    display: grid;
    place-items: center;
    z-index: 10;
  }
  .dialog {
    background: var(--bg-elevated);
    border: 1px solid var(--stroke-strong);
    border-radius: 8px;
    padding: var(--space-4);
    width: min(380px, 90vw);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
    font-size: 16px;
  }
  .field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
