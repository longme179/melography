<script lang="ts">
  /** Stock browser overlay (M2, UX_SPEC §5 S3): the shipped registry with
   *  provenance/confidence labels — the honesty rule is the point. */
  import { stocks, film } from "../lib/stores";
  import { invoke } from "@tauri-apps/api/core";

  export let open = false;

  async function pick(id: string) {
    try {
      const next = await invoke<{ stock: string | null; enabled: boolean; adjustments: unknown }>(
        "set_film",
        { args: { stock: id, adjustments: null } },
      );
      film.set(next as never);
    } catch (e) {
      console.error(e);
    }
    open = false;
  }
</script>

{#if open}
  <div class="overlay" role="dialog" aria-label="Film stock browser">
    <div class="sheet">
      <header>
        <h2>Film stocks</h2>
        <button on:click={() => (open = false)}>Close</button>
      </header>
      <ul>
        {#each $stocks as s (s.id)}
          <li class:active={s.id === $film?.stock}>
            <button class="stock" on:click={() => pick(s.id)}>
              <span class="name">{s.display_name}</span>
              <span class="labels">
                <span class="badge" class:warn={s.provenance === "ArtistAuthored"}>
                  {s.provenance === "ArtistAuthored"
                    ? "artist-authored · no measured calibration"
                    : `${s.provenance} · ${s.confidence}`}
                </span>
                <span class="badge">{s.polarity === "Reversal" ? "reversal" : "negative"}</span>
                <span class="badge">{s.layers === "Monochrome" ? "mono" : "color"}</span>
                <span class="badge">{s.default_path.toLowerCase()}</span>
              </span>
              <span class="text">{s.provenance_text}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if $stocks.length === 0}
        <p class="empty">
          No stocks found (registry directory missing or empty — see
          MELOGRAPHY_PROFILES). The film chain stays disabled rather than
          pretending.
        </p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 40;
  }
  .sheet {
    background: #20242b;
    color: #e6e9ef;
    border-radius: 8px;
    width: min(640px, 92vw);
    max-height: 80vh;
    overflow: auto;
    padding: 1rem 1.25rem;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  ul {
    list-style: none;
    margin: 0.75rem 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .stock {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    width: 100%;
    text-align: left;
    padding: 0.6rem 0.75rem;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    color: inherit;
    cursor: pointer;
  }
  .stock:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  li.active .stock {
    border-color: #7aa2f7;
  }
  .name {
    font-weight: 600;
  }
  .labels {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
  }
  .badge {
    font-size: 0.68rem;
    padding: 1px 6px;
    border-radius: 999px;
    background: rgba(127, 127, 127, 0.2);
  }
  .badge.warn {
    background: rgba(200, 150, 40, 0.25);
  }
  .text {
    font-size: 0.72rem;
    opacity: 0.7;
    line-height: 1.35;
  }
  .empty {
    opacity: 0.7;
    font-size: 0.8rem;
    line-height: 1.4;
  }
</style>
