<script lang="ts">
  /** Grain · Halation panel (UX_SPEC §6, M4 #23): amount + seed dice +
   *  halation amount wired to `Adjustments`; size scale / shadow weight /
   *  correlation lock / halation radius / red bias are CALIBRATED PER
   *  STOCK PROFILE (FILM_MODEL §G/§C) — an honest read-only note, not
   *  fake recipe-level sliders. */
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../lib/i18n";
  import { film, toast, type FilmState, type FilmAdjustments } from "../lib/stores";
  import Slider from "./Slider.svelte";

  export let onChange: () => void = () => {};

  $: adj = $film?.adjustments;
  $: hasFilm = !!$film?.stock && !!adj?.enabled;

  async function push(patch: Partial<FilmAdjustments>) {
    try {
      const next = await invoke<FilmState>("set_film", {
        args: { stock: $film?.stock ?? null, adjustments: { ...adj, ...patch } },
      });
      film.set(next);
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }

  /** Seed dice: a new deterministic seed (the grain pattern re-rolls;
   *  zoom-stable within the seed). */
  function rollSeed() {
    push({ grain_seed: (Math.floor(Math.random() * 0xffff_ffff) | 0) >>> 0 });
  }
</script>

<section class="panel">
  <div class="head">
    <h3>{t("grain.title")}</h3>
    {#if adj?.grain_seed != null}
      <button on:click={rollSeed}>🎲 {t("grain.seed.dice")}</button>
    {/if}
  </div>

  {#if hasFilm}
    <Slider
      label="grain.amount"
      tooltip="grain.amount.tip"
      value={Math.round(adj.grain_intensity * 100)}
      min={0}
      max={200}
      step={1}
      deflt={100}
      unit="units.percent"
      onCommit={(v) => push({ grain_intensity: v / 100 })}
    />
    <div class="seed-row" title={t("grain.seed.tip")}>
      <span class="label">{t("grain.seed")}</span>
      <button class="mono" on:click={rollSeed}>
        {adj?.grain_seed != null ? `#${adj.grain_seed.toString(16).padStart(8, "0")}` : t("grain.seed.dice")}
      </button>
    </div>
    <Slider
      label="grain.halation"
      tooltip="grain.halation.tip"
      value={Math.round(adj.halation_intensity * 100)}
      min={0}
      max={100}
      step={1}
      deflt={100}
      unit="units.percent"
      onCommit={(v) => push({ halation_intensity: v / 100 })}
    />
    <p class="profile-note">{t("grain.profileNote")}</p>
  {:else}
    <p class="hint">{t("tone.none")}</p>
  {/if}
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
  .seed-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13px;
    margin: var(--space-1) 0;
  }
  .seed-row .label {
    color: var(--fg-secondary);
  }
  .seed-row button {
    font-size: 11px;
  }
  .profile-note {
    font-size: 11px;
    color: var(--fg-tertiary);
    margin: var(--space-2) 0 0;
    padding: var(--space-2);
    background: var(--bg-inset, rgba(127, 127, 127, 0.08));
    border-radius: 6px;
  }
  .hint {
    font-size: 12px;
    color: var(--fg-tertiary);
    margin: var(--space-1) 0 0;
  }
</style>
