<script lang="ts">
  /** The one slider control (UX_SPEC §6): double-click resets, arrow keys
   *  nudge, typed exact values, localized numbers, physical tooltips.
   *  Every panel parameter surface uses THIS — no ad-hoc range inputs. */
  import { t, locale } from "../lib/i18n";
  export let label: string;
  export let value: number;
  export let min: number;
  export let max: number;
  export let step = 1;
  /** The reset value (defaults to `min` when it is the neutral point). */
  export let deflt: number;
  /** decimals for display */
  export let digits = 0;
  /** unit suffix key (e.g. "units.percent") — appended localized */
  export let unit: string | null = null;
  /** physical tooltip key (UX_SPEC: explain the physical meaning) */
  export let tooltip: string | null = null;
  /** disabled (honest "not available" rows) */
  export let disabled = false;
  /** show a signed value (+/−) */
  export let signed = false;
  export let onCommit: (v: number) => void = () => {};

  let typing = false;
  let typed = "";

  $: isDefault = Math.abs(value - deflt) < 1e-9;

  function clamp(v: number): number {
    return Math.min(max, Math.max(min, v));
  }

  function fmt(v: number): string {
    const loc = $locale === "vi" ? "vi-VN" : "en-US";
    const n = v.toLocaleString(loc, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
    return signed && v > 0 ? `+${n}` : n;
  }

  function commit(v: number) {
    const c = clamp(Math.round(v / step) * step);
    if (Math.abs(c - value) > 1e-9) onCommit(c);
  }

  function onKey(e: KeyboardEvent) {
    if (typing) return;
    const fine = e.shiftKey ? 10 : 1;
    if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
      e.preventDefault();
      commit(value - step * fine);
    } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
      e.preventDefault();
      commit(value + step * fine);
    }
  }

  function onDoubleClick() {
    // Double-click = reset to default (UX_SPEC §6 "every slider").
    if (!disabled) commit(deflt);
  }

  function startTyping() {
    typing = true;
    typed = String(value);
  }

  function commitTyping() {
    typing = false;
    const v = parseFloat(typed.replace(",", "."));
    if (Number.isFinite(v)) commit(v);
  }

  /** focus + select the just-swapped typed input */
  function initFocus(node: HTMLInputElement) {
    node.focus();
    node.select();
    return {
      destroy() {},
    };
  }
</script>

<div class="slider-row" class:disabled>
  <span class="label" title={tooltip ? t(tooltip) : undefined}>{t(label)}</span>
  <input
    type="range"
    {min}
    {max}
    {step}
    {disabled}
    {value}
    aria-label={t(label)}
    on:keydown={onKey}
    on:dblclick={onDoubleClick}
    on:change={(e) => commit(Number(e.currentTarget.value))}
  />
  {#if typing}
    <input
      class="typed mono"
      type="text"
      bind:value={typed}
      on:blur={commitTyping}
      on:keydown={(e) => {
        if (e.key === "Enter") commitTyping();
        if (e.key === "Escape") (typing = false);
      }}
      use:initFocus
    />
  {:else}
    <button
      class="val mono"
      class:neutral={isDefault}
      {disabled}
      title={t("slider.typeHint")}
      on:click={startTyping}
    >
      {fmt(value)}{unit ? t(unit) : ""}
    </button>
  {/if}
</div>

<style>
  .slider-row {
    display: grid;
    grid-template-columns: 84px 1fr 58px;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
    margin: var(--space-1) 0;
  }
  .slider-row.disabled {
    opacity: 0.45;
  }
  .label {
    color: var(--fg-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: help;
  }
  .slider-row:not(.disabled) .label:hover {
    color: var(--fg-primary);
  }
  input[type="range"] {
    width: 100%;
  }
  .val {
    text-align: right;
    font-size: 11px;
    background: transparent;
    border: none;
    padding: 2px 4px;
    color: var(--fg-primary);
    cursor: text;
    /* Exempt from the global 32px touch-target floor: this is the value
       readout (click-to-type); the slider row itself is the target. */
    min-height: 0;
  }
  .val.neutral {
    color: var(--fg-tertiary);
  }
  .typed {
    width: 100%;
    font-size: 11px;
    padding: 1px 4px;
    text-align: right;
  }
</style>
