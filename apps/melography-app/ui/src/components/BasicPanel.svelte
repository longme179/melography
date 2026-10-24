<script lang="ts">
  /** Basic panel (UX_SPEC §6, M4 #23): Exposure / Highlights / Shadows —
   *  the tone stage's user surface. Contrast / Whites / Blacks are honest
   *  "not available yet" rows (no fake sliders, UX_SPEC honesty rules). */
  import { getTone, setTone } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import { tone, toast, type ToneParams } from "../lib/stores";
  import Slider from "./Slider.svelte";

  export let onChange: () => void = () => {};

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
  $: hasStage = !!$tone?.params;

  async function push(patch: Partial<ToneParams>) {
    try {
      const next = { ...params, ...patch };
      tone.set(await setTone({ params: next, source: null }));
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  async function reset() {
    try {
      tone.set(await setTone({ params: null, source: null }));
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  getTone().then((s) => tone.set(s)).catch(console.error);
</script>

<section class="panel">
  <div class="head">
    <h3>{t("panel.basic")}</h3>
    {#if hasStage}
      <button on:click={reset}>{t("tone.reset")}</button>
    {/if}
  </div>

  <Slider
    label="basic.ev"
    tooltip="basic.ev.tip"
    value={params.ev}
    min={-3}
    max={3}
    step={0.05}
    digits={2}
    deflt={0}
    signed
    onCommit={(v) => push({ ev: v })}
  />
  <Slider
    label="basic.highlights"
    tooltip="basic.highlights.tip"
    value={params.highlights}
    min={-100}
    max={100}
    step={1}
    deflt={0}
    signed
    onCommit={(v) => push({ highlights: v })}
  />
  <Slider
    label="basic.shadows"
    tooltip="basic.shadows.tip"
    value={params.shadows}
    min={-100}
    max={100}
    step={1}
    deflt={0}
    signed
    onCommit={(v) => push({ shadows: v })}
  />

  {#if !hasStage}
    <p class="hint">{t("tone.none")}</p>
  {/if}

  <!-- Honest "not available yet" rows (UX_SPEC §9: no dead ends, no fake
       controls) — each names the tracked follow-up. -->
  <div class="na-row" title={t("basic.contrast.tip")}>
    <span class="label">{t("basic.contrast")}</span>
    <span class="badge">{t("basic.na")}</span>
  </div>
  <div class="na-row" title={t("basic.whites.tip")}>
    <span class="label">{t("basic.whites")}</span>
    <span class="badge">{t("basic.na")}</span>
  </div>
  <div class="na-row" title={t("basic.blacks.tip")}>
    <span class="label">{t("basic.blacks")}</span>
    <span class="badge">{t("basic.na")}</span>
  </div>
</section>

<style>
  .panel {
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head h3 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
  }
  .na-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13px;
    opacity: 0.55;
    margin: var(--space-1) 0;
    cursor: help;
  }
  .na-row .label {
    color: var(--fg-secondary);
  }
  .badge {
    font-size: 11px;
    padding: 1px 8px;
    border-radius: 999px;
    border: 1px dashed var(--stroke-strong);
    color: var(--fg-tertiary);
  }
  .hint {
    font-size: 12px;
    color: var(--fg-tertiary);
    margin: var(--space-1) 0 0;
  }
</style>
