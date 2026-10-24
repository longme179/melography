<script lang="ts">
  /** Film Lab panel (M2, issue #10 UX leg; UX_SPEC P3 depth).
   *  Every knob maps 1:1 onto `Adjustments`; provenance honesty is
   *  prominent (UX_SPEC §5: artist-authored = "no measured calibration"). */
  import { invoke } from "@tauri-apps/api/core";
  import { film, stocks, type Stock, type FilmAdjustments } from "../lib/stores";
  import { toast } from "../lib/stores";

  let busy = false;

  async function push(patch: Partial<FilmAdjustments>) {
    if (busy) return;
    busy = true;
    try {
      const adj = { ...$film.adjustments, ...patch };
      const next = await invoke<{ stock: string | null; enabled: boolean; adjustments: FilmAdjustments }>(
        "set_film",
        { args: { stock: $film.stock, adjustments: adj } },
      );
      film.set(next);
      await invoke("render_preview", { maxDim: 0 }).catch(() => {});
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  async function selectStock(id: string | null) {
    if (busy) return;
    busy = true;
    try {
      const next = await invoke<{ stock: string | null; enabled: boolean; adjustments: FilmAdjustments }>(
        "set_film",
        { args: { stock: id, adjustments: $film.adjustments } },
      );
      film.set(next);
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  const selected = $stocks.find((s) => s.id === $film?.stock);
</script>

<section class="panel">
  <h2>Film Lab</h2>

  <label class="row">
    <span>Stock</span>
    <select
      value={$film?.stock ?? ""}
      on:change={(e) => selectStock(e.currentTarget.value || null)}
    >
      <option value="">— none (identity) —</option>
      {#each $stocks as s (s.id)}
        <option value={s.id}>{s.display_name}</option>
      {/each}
    </select>
  </label>

  {#if selected}
    <div class="provenance" class:warn={selected.provenance === "ArtistAuthored"}>
      {selected.provenance === "ArtistAuthored"
        ? "artist-authored — no measured calibration"
        : `${selected.provenance} · ${selected.confidence}`}
      <span class="meta">
        {selected.polarity === "Reversal" ? "reversal" : "negative"} ·
        {selected.layers === "Monochrome" ? "mono" : "color"} ·
        {selected.default_path.toLowerCase()}
      </span>
    </div>

    {#if $film}
      <label class="row">
        <span>EV</span>
        <input
          type="range" min="-5" max="5" step="0.25"
          value={$film.adjustments.ev}
          on:input={(e) => push({ ev: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.ev).toFixed(2)}</b>
      </label>

      <label class="row">
        <span>Push/pull</span>
        <input
          type="range" min="-3" max="3" step="0.5"
          value={$film.adjustments.push_pull_stops}
          on:input={(e) => push({ push_pull_stops: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.push_pull_stops).toFixed(1)}</b>
      </label>

      <label class="row">
        <span>Dev contrast</span>
        <input
          type="range" min="0.6" max="1.6" step="0.05"
          value={$film.adjustments.dev_contrast}
          on:input={(e) => push({ dev_contrast: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.dev_contrast).toFixed(2)}</b>
      </label>

      <label class="row">
        <span>Print exposure</span>
        <input
          type="range" min="-3" max="3" step="0.25"
          value={$film.adjustments.print_exposure_stops}
          on:input={(e) => push({ print_exposure_stops: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.print_exposure_stops).toFixed(2)}</b>
      </label>

      <label class="row">
        <span>Grain</span>
        <input
          type="range" min="0" max="2" step="0.05"
          value={$film.adjustments.grain_intensity}
          on:input={(e) => push({ grain_intensity: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.grain_intensity).toFixed(2)}</b>
      </label>

      <label class="row">
        <span>Halation</span>
        <input
          type="range" min="0" max="1" step="0.05"
          value={$film.adjustments.halation_intensity}
          on:input={(e) => push({ halation_intensity: parseFloat(e.currentTarget.value) })}
        />
        <b>{($film.adjustments.halation_intensity).toFixed(2)}</b>
      </label>

      <label class="row">
        <span>Grain seed</span>
        <input
          type="number" min="0" step="1"
          value={$film.adjustments.grain_seed ?? 0}
          on:change={(e) => push({ grain_seed: parseInt(e.currentTarget.value) || 0 })}
        />
      </label>

      <label class="row checkbox">
        <input
          type="checkbox"
          checked={$film.adjustments.enabled}
          on:change={(e) => push({ enabled: e.currentTarget.checked })}
        />
        <span>Film chain enabled</span>
      </label>
    {/if}
  {:else}
    <p class="hint">
      Select a stock to enable the film chain. Export and sidecar record the
      stock by id (data, not code); artist-authored stocks carry no measured
      calibration — the honesty line follows the look everywhere.
    </p>
  {/if}
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    font-size: 0.8rem;
  }
  h2 {
    margin: 0 0 0.25rem;
    font-size: 0.9rem;
  }
  .row {
    display: grid;
    grid-template-columns: 6.5rem 1fr 3rem;
    align-items: center;
    gap: 0.4rem;
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
  input[type="range"] {
    width: 100%;
  }
</style>
