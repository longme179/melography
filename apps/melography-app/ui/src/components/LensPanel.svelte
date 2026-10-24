<script lang="ts">
  /** Lens panel (UX_SPEC §6, M4 #23): Vignette + Veiling/diffusion wired
   *  to the optics stage (FILM_MODEL §B); CA and Softness are honest
   *  not-available rows (CA is declared-rejected by the film crate — it
   *  refuses to silently no-op). Lens state rides the film recipe; no
   *  stock selected = the stage has nothing to act on (honest hint). */
  import { invoke } from "@tauri-apps/api/core";
  import { t } from "../lib/i18n";
  import { film, toast, type FilmState, type LensParams } from "../lib/stores";
  import Slider from "./Slider.svelte";

  export let onChange: () => void = () => {};

  const DEFAULT_LENS: LensParams = {
    vignette_amount: 0,
    lens_fov_deg: 54,
    veiling_amount: 0,
    veiling_sigma_mm: 1.5,
    chromatic_aberration: 0,
    distortion: 0,
  };

  $: lens = $film?.lens ?? DEFAULT_LENS;
  $: hasFilm = !!$film?.stock && $film.enabled;

  async function pushLens(patch: Partial<LensParams>) {
    try {
      const next = await invoke<FilmState>("set_film", {
        args: { stock: $film?.stock ?? null, lens: { ...lens, ...patch } },
      });
      film.set(next);
      onChange();
    } catch (e) {
      toast.set(String(e));
    }
  }
</script>

<section class="panel">
  <div class="head">
    <h3>{t("lens.title")}</h3>
  </div>

  {#if hasFilm}
    <Slider
      label="lens.vignette"
      tooltip="lens.vignette.tip"
      value={Math.round(lens.vignette_amount * 100)}
      min={0}
      max={100}
      step={1}
      deflt={0}
      unit="units.percent"
      onCommit={(v) => pushLens({ vignette_amount: v / 100 })}
    />
    <Slider
      label="lens.veiling"
      tooltip="lens.veiling.tip"
      value={Math.round(lens.veiling_amount * 100)}
      min={0}
      max={100}
      step={1}
      deflt={0}
      unit="units.percent"
      onCommit={(v) => pushLens({ veiling_amount: v / 100 })}
    />
  {:else}
    <p class="hint">{t("tone.none")}</p>
  {/if}

  <div class="na-row" title={t("lens.ca.tip")}>
    <span class="label">{t("lens.ca")}</span>
    <span class="badge">[ART off] · {t("basic.na")}</span>
  </div>
  <div class="na-row" title={t("lens.softness.tip")}>
    <span class="label">{t("lens.softness")}</span>
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
