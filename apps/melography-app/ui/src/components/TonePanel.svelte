<script lang="ts">
  /** AI panel (M3 issue #13 → M4 #23 restructure): the baseline
   *  auto-lighting suggestion (Accept / Adjust / Dismiss — AI_MODELS §7)
   *  plus model management. The parameter SLIDERS live in Basic / Color;
   *  this panel owns the suggestion conversation and its provenance. */
  import {
    autoLightSuggest,
    getTone,
    setTone,
    suggestionProvenance,
  } from "../lib/ipc";
  import { t, fmtNum } from "../lib/i18n";
  import { suggestion, tone, toast, type ToneParams } from "../lib/stores";

  export let onChange: () => void = () => {};

  let busy = false;

  const DEFAULT_PARAMS: ToneParams = {
    ev: 0,
    highlights: 0,
    shadows: 0,
    wb: null,
    black_source: null,
    black_target: null,
    white_source: null,
    white_target: null,
  };

  $: params = $tone?.params ?? DEFAULT_PARAMS;

  async function push(patch: Partial<ToneParams>, source = null) {
    if (busy) return;
    busy = true;
    try {
      const next = { ...params, ...patch };
      const state = await setTone({ params: next, source });
      tone.set(state);
      onChange();
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  async function reset() {
    if (busy) return;
    busy = true;
    try {
      tone.set(await setTone({ params: null, source: null }));
      suggestion.set(null);
      onChange();
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  async function suggest() {
    if (busy) return;
    busy = true;
    try {
      suggestion.set(await autoLightSuggest());
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  /** Accept: apply + record provenance (AI_MODELS §7). */
  async function accept() {
    const s = $suggestion;
    if (!s) return;
    const source = await suggestionProvenance("baseline", s.engine, s.engine_version);
    await push(s.params, source);
    suggestion.set(null);
  }

  /** Adjust: load into the sliders, no provenance (user owns the values). */
  function adjust() {
    const s = $suggestion;
    if (!s) return;
    push(s.params);
    suggestion.set(null);
  }

  function dismiss() {
    suggestion.set(null);
  }

  function fmt(v: number, digits = 2): string {
    // Locale-aware decimal separator (VI = comma) — UX_SPEC §10, #15.
    return fmtNum(v, digits, true);
  }

  function noteKey(n: string): string {
    return `suggest.notes.${n}`;
  }

  getTone().then((s) => tone.set(s)).catch(console.error);
</script>

<section class="panel">
  <div class="head">
    <h2>{t("panel.ai")}</h2>
    <div class="actions">
      <button class="primary" disabled={busy} on:click={suggest}>
        {t("tone.auto")}
      </button>
      {#if $tone?.params}
        <button disabled={busy} on:click={reset}>{t("tone.reset")}</button>
      {/if}
    </div>
  </div>

  {#if $suggestion}
    <div class="card" role="status">
      <div class="card-title">{t("suggest.title")}</div>
      <div class="mono summary">
        EV {fmt($suggestion.params.ev)}
        {#if $suggestion.params.highlights}
          · {t("tone.highlights")} {fmt($suggestion.params.highlights, 0)}
        {/if}
        {#if $suggestion.params.shadows}
          · {t("tone.shadows")} {fmt($suggestion.params.shadows, 0)}
        {/if}
        {#if $suggestion.params.wb}
          · WB
          {#if $suggestion.wb_cct_estimate_k}
            ≈ {Math.round($suggestion.wb_cct_estimate_k)} K
          {:else}
            {fmt($suggestion.params.wb.r)} / {fmt($suggestion.params.wb.g)} / {fmt($suggestion.params.wb.b)}
          {/if}
        {/if}
      </div>
      {#if $suggestion.notes.length}
        <ul class="notes">
          {#each $suggestion.notes as n (n)}
            <li>{t(noteKey(n))}</li>
          {/each}
        </ul>
      {/if}
      <div class="meta mono">
        {$suggestion.engine} v{$suggestion.engine_version} ·
        {$suggestion.analysis_ms.toFixed(0)} ms · {t("suggest.model_free")}
      </div>
      <div class="actions">
        <button class="primary" on:click={accept}>{t("suggest.accept")}</button>
        <button on:click={adjust}>{t("suggest.adjust")}</button>
        <button on:click={dismiss}>{t("suggest.dismiss")}</button>
      </div>
    </div>
  {/if}

  {#if $tone?.params}
    {#if $tone?.source}
      <div class="meta">
        {t("suggest.accepted_from")}
        {$tone.source.name} v{$tone.source.version}
        ({$tone.source.accepted_utc})
      </div>
    {/if}
  {:else}
    <p class="hint">{t("tone.none")}</p>
  {/if}
</section>

<style>
  .panel {
    padding: 0 var(--space-3) var(--space-3);
    border-bottom: 1px solid var(--stroke-subtle);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .head h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .card {
    background: var(--bg-inset, rgba(127, 127, 127, 0.08));
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
    padding: var(--space-2);
    margin: var(--space-2) 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .card-title {
    font-weight: 600;
  }
  .summary {
    font-size: 12px;
  }
  .notes {
    margin: 0;
    padding-left: 18px;
    font-size: 12px;
    color: var(--fg-secondary);
  }
  .meta {
    font-size: 11px;
    color: var(--fg-tertiary);
  }
  .hint {
    font-size: 12px;
    color: var(--fg-tertiary);
    margin: var(--space-1) 0 0;
  }
  button {
    background: transparent;
    border: 1px solid var(--stroke-subtle);
    border-radius: 6px;
    padding: 4px 10px;
    cursor: pointer;
    font-size: 12px;
    color: inherit;
  }
</style>
