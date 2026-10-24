<script lang="ts">
  /** Color panel (UX_SPEC §6, M4 #23): Temperature (2000–12000 K) + Tint
   *  (−100..100) → WB cone gains, synthesized in Rust (`wb_from_cct_tint`
   *  — the webview never computes color, ADR-0001). The stage's own
   *  Kelvin/tint ESTIMATES drive the display when gains came from
   *  auto-lighting. Vibrance/Saturation are honest not-available rows. */
  import { getTone, setTone, wbFromCctTint } from "../lib/ipc";
  import { t } from "../lib/i18n";
  import { tone, toast, wbPicker, type ToneParams } from "../lib/stores";
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

  /** Slider neutral: ~5800 K (the Planckian locus point nearest the AP1
   *  white; color::tone documents why Kelvin-neutral ≠ exact D60). */
  const NEUTRAL_K = 5800;

  $: params = $tone?.params ?? DEFAULT_PARAMS;
  $: currentK = $tone?.cct_estimate_k ?? NEUTRAL_K;
  $: currentTint = $tone?.tint_estimate ?? 0;
  $: hasWb = params.wb !== null;

  async function applyWb(cctK: number, tint: number) {
    try {
      const wb = await wbFromCctTint(cctK, tint);
      const next = { ...params, wb };
      tone.set(await setTone({ params: next, source: null }));
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  /** "As shot" — remove the WB correction entirely. */
  async function neutral() {
    try {
      const next = { ...params, wb: null };
      tone.set(await setTone({ params: next, source: null }));
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  /** Eyedropper (UX_SPEC §6): arm the viewport pick — the next click on
   *  a neutral area sets the WB gains (Esc cancels). */
  function togglePicker() {
    wbPicker.update((v) => !v);
  }

  getTone().then((s) => tone.set(s)).catch(console.error);
</script>

<section class="panel">
  <div class="head">
    <h3>{t("color.title")}</h3>
    <div class="head-actions">
      <button
        class:armed={$wbPicker}
        aria-pressed={$wbPicker}
        title={t("wb.pick.tip")}
        on:click={togglePicker}
      >
        {t("wb.pick")}
      </button>
      {#if hasWb}
        <button on:click={neutral}>{t("color.neutral")}</button>
      {/if}
    </div>
  </div>

  {#if $wbPicker}
    <p class="pick-hint" role="status">{t("wb.picking")}</p>
  {/if}

  {#if hasWb}
    <Slider
      label="color.temp"
      tooltip="color.temp.tip"
      value={Math.round(currentK)}
      min={2000}
      max={12000}
      step={50}
      digits={0}
      deflt={NEUTRAL_K}
      unit="units.kelvin"
      onCommit={(v) => applyWb(v, currentTint)}
    />
    <Slider
      label="color.tint"
      tooltip="color.tint.tip"
      value={currentTint}
      min={-100}
      max={100}
      step={1}
      digits={0}
      deflt={0}
      signed
      onCommit={(v) => applyWb(currentK, v)}
    />
  {:else}
    <button class="wide" on:click={() => applyWb(NEUTRAL_K, 0)}>
      {t("color.temp")} · {t("color.neutral")}
    </button>
    <p class="hint">{t("tone.none")}</p>
  {/if}

  <div class="na-row" title={t("color.vibrance.tip")}>
    <span class="label">{t("color.vibrance")}</span>
    <span class="badge">{t("basic.na")}</span>
  </div>
  <div class="na-row" title={t("color.saturation.tip")}>
    <span class="label">{t("color.saturation")}</span>
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
  .head-actions {
    display: flex;
    gap: var(--space-1);
  }
  .head-actions .armed {
    color: var(--accent-400);
    border-color: var(--accent-400);
  }
  .pick-hint {
    font-size: 12px;
    color: var(--accent-400);
    margin: 0 0 var(--space-1);
  }
  .head h3 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
  }
  .wide {
    width: 100%;
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
