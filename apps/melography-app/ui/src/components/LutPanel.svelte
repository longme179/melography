<script lang="ts">
  // LUT panel (UX_SPEC §6 LUT row): library grid with provenance badges,
  // single active LUT slot with intensity + insertion point + declared
  // encodings (read-only after import — declarations are import-time data,
  // re-declaring happens through re-import, never silent reinterpretation).
  import { onMount } from "svelte";
  import {
    listLuts,
    setLutStack,
    type LutRecord,
    type LutStackEntry,
  } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import { state, lastError } from "../lib/stores";
  import Badge from "./Badge.svelte";

  export let onChange: () => void = () => {};

  let library: LutRecord[] = [];
  let activeHash: string | null = null;
  let intensity = 1.0;
  let insertion = "finishing";

  const insertions = ["pre-film", "film-input", "print-output", "finishing"];

  export async function reload() {
    try {
      library = await listLuts();
      const st = $state;
      const first = st.lutStack[0];
      if (first) {
        activeHash = first.lutHash;
        intensity = first.intensity;
        insertion = first.insertion;
      }
    } catch (e) {
      lastError.set(e as never);
    }
  }

  async function push() {
    if (!activeHash) {
      await setLutStack([]);
      onChange();
      return;
    }
    const rec = library.find((l) => l.contentHash === activeHash);
    if (!rec) return;
    const entry: LutStackEntry = {
      lutHash: rec.contentHash,
      insertion,
      intensity,
      declaredInput: rec.declaredInput,
      declaredOutput: rec.declaredOutput,
      enabled: true,
    };
    await setLutStack([entry]);
    onChange();
  }

  function shortHash(h: string): string {
    return h.slice(0, 8);
  }

  onMount(reload);
</script>

<section class="panel" aria-label={t("panel.lut")}>
  <h2>{t("panel.lut")}</h2>

  <div class="library">
    <h3>{t("panel.lut.library")}</h3>
    {#if library.length === 0}
      <p class="empty">{t("panel.lut.empty")}</p>
    {:else}
      <ul>
        {#each library as rec (rec.contentHash)}
          <li>
            <label class="row">
              <input
                type="radio"
                name="active-lut"
                value={rec.contentHash}
                bind:group={activeHash}
                on:change={push}
                disabled={!rec.implemented}
              />
              <span class="name" title={rec.originalFilename}>
                {rec.title ?? rec.originalFilename}
              </span>
              {#if !rec.implemented}
                <Badge tone="warn">{t("encoding.agx-log")}</Badge>
              {/if}
              {#each rec.warnings as w}
                <Badge tone="warn" title={w.kind}>
                  ⚠
                </Badge>
              {/each}
              <span class="meta mono">{rec.size}³</span>
            </label>
          </li>
        {/each}
      </ul>
    {/if}
  </div>

  {#if activeHash}
    <div class="controls">
      <label class="field">
        {t("panel.lut.intensity")}
        <input
          type="range"
          min="0"
          max="1"
          step="0.01"
          bind:value={intensity}
          on:input={push}
        />
        <span class="mono">{Math.round(intensity * 100)}{t("units.percent")}</span>
      </label>
      <label class="field">
        {t("panel.lut.insertion")}
        <select bind:value={insertion} on:change={push}>
          {#each insertions as ins}
            <option value={ins}>{t(`insertion.${ins}`)}</option>
          {/each}
        </select>
      </label>
      {#if library.find((l) => l.contentHash === activeHash)?.declaredInput}
        <p class="encodings">
          {t("encoding.label.in")}:
          {t(
            `encoding.${
              library.find((l) => l.contentHash === activeHash)?.declaredInput
            }`,
          )}
          →
          {t("encoding.label.out")}:
          {t(
            `encoding.${
              library.find((l) => l.contentHash === activeHash)?.declaredOutput
            }`,
          )}
        </p>
      {/if}
    </div>
  {:else if $state.imageOpen}
    <p class="empty">{t("panel.lut.none")}</p>
  {/if}
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3);
  }
  h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }
  h3 {
    font-size: 12px;
    font-weight: 600;
    margin: 0 0 var(--space-2);
    color: var(--fg-secondary);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .library ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius);
    cursor: pointer;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    color: var(--fg-tertiary);
    font-size: 11px;
  }
  .empty {
    color: var(--fg-tertiary);
    margin: var(--space-2) 0;
  }
  .controls {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    border-top: 1px solid var(--stroke-subtle);
    padding-top: var(--space-3);
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .field input[type="range"] {
    flex: 1;
  }
  .encodings {
    color: var(--fg-tertiary);
    font-size: 12px;
    margin: 0;
  }
</style>
