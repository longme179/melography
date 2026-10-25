<script lang="ts">
  /** Film Lab panel (M2, issue #10 UX leg; UX_SPEC P3 depth).
   *  Every knob maps 1:1 onto `Adjustments`; provenance honesty is
   *  prominent (UX_SPEC §5: artist-authored = "no measured calibration").
   *  Session 09 (#15): zero embedded strings — every label/tooltip goes
   *  through the i18n catalogs, numbers are locale-aware, and all knobs
   *  use the shared Slider (double-click reset, arrows, typed values).
   *  A1 (M5-A): renders route through the App-level viewport.refresh()
   *  — the ONE render path, stock select included. No panel invokes
   *  render_preview itself; rapid commits coalesce in the viewport
   *  (last-wins) instead of a busy gate that silently swallows them. */
  import { invoke } from "@tauri-apps/api/core";
  import { film, stocks, toast, state, type Stock, type FilmAdjustments } from "../lib/stores";
  import { t, tEnum } from "../lib/i18n";
  import Slider from "./Slider.svelte";

  export let onChange: () => void = () => {};

  async function push(patch: Partial<FilmAdjustments>) {
    try {
      const adj = { ...$film.adjustments, ...patch };
      const next = await invoke<{ stock: string | null; enabled: boolean; adjustments: FilmAdjustments }>(
        "set_film",
        { args: { stock: $film.stock, adjustments: adj } },
      );
      film.set(next);
      // App routes this to viewport.refresh() — the single render path.
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  async function selectStock(id: string | null) {
    try {
      const next = await invoke<{ stock: string | null; enabled: boolean; adjustments: FilmAdjustments }>(
        "set_film",
        { args: { stock: id, adjustments: $film.adjustments } },
      );
      film.set(next);
      // A1: stock select re-renders (was: no visual change until a
      // slider moved — the reported "Phòng phim" symptom).
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  const selected: Stock | undefined = $stocks.find((s) => s.id === $film?.stock);
</script>

<section class="panel">
  <label class="row">
    <span>{t("film.stock")}</span>
    <select
      aria-label={t("film.stock")}
      value={$film?.stock ?? ""}
      on:change={(e) => selectStock(e.currentTarget.value || null)}
    >
      <option value="">{t("film.none")}</option>
      {#each $stocks as s (s.id)}
        <option value={s.id}>{s.display_name}</option>
      {/each}
    </select>
  </label>

  {#if $stocks.length === 0}
    <!-- Session 11 (UX_SPEC §9 honest failure): a missing registry is a
         diagnosable configuration error — name the path, never silently
         show an empty feature. -->
    <div class="no-stocks" role="status">
      <p>{t("film.noStocks")}</p>
      <p class="mono path">{$state.stocksDir}</p>
    </div>
  {/if}

  {#if selected}
    <div class="provenance" class:warn={selected.provenance === "ArtistAuthored"}>
      {tEnum("stock.provenance", selected.provenance)}
      <span class="meta">
        {tEnum("stock.polarity", selected.polarity)} ·
        {tEnum("stock.layers", selected.layers)} ·
        {tEnum("stock.path", selected.default_path)}
      </span>
    </div>

    {#if $film}
      <Slider
        label="film.ev"
        tooltip="film.ev.tip"
        value={$film.adjustments.ev}
        min={-5}
        max={5}
        step={0.25}
        deflt={0}
        digits={2}
        signed
        onCommit={(v) => push({ ev: v })}
      />
      <Slider
        label="film.pushPull"
        tooltip="film.pushPull.tip"
        value={$film.adjustments.push_pull_stops}
        min={-3}
        max={3}
        step={0.5}
        deflt={0}
        digits={1}
        signed
        onCommit={(v) => push({ push_pull_stops: v })}
      />
      <Slider
        label="film.devContrast"
        tooltip="film.devContrast.tip"
        value={$film.adjustments.dev_contrast}
        min={0.6}
        max={1.6}
        step={0.05}
        deflt={1}
        digits={2}
        onCommit={(v) => push({ dev_contrast: v })}
      />
      <Slider
        label="film.printExposure"
        tooltip="film.printExposure.tip"
        value={$film.adjustments.print_exposure_stops}
        min={-3}
        max={3}
        step={0.25}
        deflt={0}
        digits={2}
        signed
        onCommit={(v) => push({ print_exposure_stops: v })}
      />
      <Slider
        label="grain.amount"
        tooltip="grain.amount.tip"
        value={Math.round($film.adjustments.grain_intensity * 100)}
        min={0}
        max={200}
        step={5}
        deflt={100}
        digits={0}
        unit="units.percent"
        onCommit={(v) => push({ grain_intensity: v / 100 })}
      />
      <Slider
        label="grain.halation"
        tooltip="grain.halation.tip"
        value={Math.round($film.adjustments.halation_intensity * 100)}
        min={0}
        max={100}
        step={5}
        deflt={100}
        digits={0}
        unit="units.percent"
        onCommit={(v) => push({ halation_intensity: v / 100 })}
      />

      <label class="row seed" title={t("grain.seed.tip")}>
        <span>{t("grain.seed")}</span>
        <input
          class="mono"
          type="number"
          min="0"
          step="1"
          aria-label={t("grain.seed")}
          value={$film.adjustments.grain_seed ?? 0}
          on:change={(e) => push({ grain_seed: parseInt(e.currentTarget.value) || 0 })}
        />
      </label>

      {#if $film.adjustments.grain_intensity > 0}
        <p class="hint">{t("film.grainFast")}</p>
      {/if}

      <label class="row checkbox">
        <input
          type="checkbox"
          checked={$film.adjustments.enabled}
          on:change={(e) => push({ enabled: e.currentTarget.checked })}
        />
        <span>{t("film.chainEnabled")}</span>
      </label>
    {/if}
  {:else}
    <p class="hint">{t("film.hint")}</p>
  {/if}
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    font-size: 0.8rem;
  }
  .row {
    display: grid;
    grid-template-columns: 6.5rem 1fr 3rem;
    align-items: center;
    gap: 0.4rem;
  }
  .row.seed {
    grid-template-columns: 6.5rem 1fr;
  }
  .row.checkbox {
    grid-template-columns: auto 1fr;
  }
  .provenance {
    padding: 0.35rem 0.5rem;
    border-radius: 4px;
    background: rgba(127, 127, 127, 0.12);
    font-size: 0.72rem;
  }
  .provenance.warn {
    background: rgba(200, 150, 40, 0.15);
  }
  .meta {
    display: block;
    opacity: 0.65;
    margin-top: 2px;
  }
  .hint {
    opacity: 0.7;
    line-height: 1.4;
  }
  .no-stocks {
    background: color-mix(in srgb, var(--warn) 10%, var(--bg-panel));
    border: 1px solid var(--warn);
    border-radius: var(--radius);
    color: var(--fg-secondary);
    font-size: 12px;
    line-height: 1.45;
    padding: var(--space-2) var(--space-3);
  }
  .no-stocks .path {
    color: var(--fg-tertiary);
    font-size: 11px;
    margin: var(--space-1) 0 0;
    overflow-wrap: anywhere;
  }
</style>
