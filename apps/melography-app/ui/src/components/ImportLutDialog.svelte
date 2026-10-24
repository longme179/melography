<script lang="ts">
  // D1 Import LUT dialog (UX_SPEC §2 D1): file + encoding declaration.
  // Only implemented encodings are selectable; agx-log/custom shaper are
  // visible but disabled ("coming soon") — the fixed vocabulary is part
  // of the contract (luts.md §3), never guessed.
  import { lutFileDialog, importLut } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import { lastError } from "../lib/stores";

  export let onDone: () => void = () => {};
  export let open = false;

  const implementedEncodings = [
    "srgb-display",
    "rec709-display-linear",
    "rec709-gamma2.4",
    "display-p3-display",
    "acescct",
    "acescc",
  ];
  const futureEncodings = ["agx-log", "custom-1d-shaper"];

  let path: string | null = null;
  let encIn = "srgb-display";
  let encOut = "srgb-display";
  let sourceUrl = "";
  let licenseNote = "";
  let busy = false;

  async function pick() {
    path = await lutFileDialog();
  }

  async function doImport() {
    if (!path || busy) return;
    busy = true;
    try {
      await importLut({
        path,
        declaredInput: encIn,
        declaredOutput: encOut,
        sourceUrl: sourceUrl || undefined,
        licenseNote: licenseNote || undefined,
      });
      onDone();
      open = false;
    } catch (e) {
      lastError.set(e as never);
    } finally {
      busy = false;
    }
  }

  // Esc closes the dialog (#15 a11y; never while an import is in flight).
  function onWindowKey(e: KeyboardEvent) {
    if (open && !busy && e.key === "Escape") open = false;
  }
</script>

<svelte:window on:keydown={onWindowKey} />

{#if open}
  <div class="overlay" role="dialog" aria-modal="true" aria-label={t("import.title")}>
    <div class="dialog">
      <h2>{t("import.title")}</h2>

      <label class="field">
        {t("import.file")}
        <button on:click={pick}>{path ?? t("import.pick")}</button>
      </label>

      <label class="field">
        {t("encoding.label.in")}
        <select bind:value={encIn}>
          {#each implementedEncodings as e}
            <option value={e}>{t(`encoding.${e}`)}</option>
          {/each}
          {#each futureEncodings as e}
            <option disabled value={e}>{t(`encoding.${e}`)}</option>
          {/each}
        </select>
      </label>

      <label class="field">
        {t("encoding.label.out")}
        <select bind:value={encOut}>
          {#each implementedEncodings as e}
            <option value={e}>{t(`encoding.${e}`)}</option>
          {/each}
          {#each futureEncodings as e}
            <option disabled value={e}>{t(`encoding.${e}`)}</option>
          {/each}
        </select>
      </label>

      <fieldset>
        <legend>{t("import.provenance")}</legend>
        <label class="field">
          {t("import.sourceUrl")}
          <input type="text" bind:value={sourceUrl} placeholder="https://…" />
        </label>
        <label class="field">
          {t("import.license")}
          <input type="text" bind:value={licenseNote} />
        </label>
      </fieldset>

      <div class="actions">
        <button on:click={() => (open = false)}>{t("export.cancel")}</button>
        <button class="primary" disabled={!path || busy} on:click={doImport}>
          {t("import.doImport")}
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
    width: min(440px, 90vw);
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
  .field input[type="text"] {
    flex: 1;
  }
  fieldset {
    border: 1px solid var(--stroke-subtle);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
