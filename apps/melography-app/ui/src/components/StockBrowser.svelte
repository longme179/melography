<script lang="ts">
  /** Stock browser overlay (M2, UX_SPEC §5 S3): the shipped registry with
   *  provenance/confidence labels — the honesty rule is the point.
   *  Session 09 (#15): zero embedded strings (enum values localized via
   *  tEnum), theme-token colors (light/contrast themes are correct), and
   *  dialog a11y (aria-modal, Esc, initial focus on the close button). */
  import { stocks, film } from "../lib/stores";
  import { t, tEnum } from "../lib/i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy } from "svelte";

  export let open = false;

  let closeBtn: HTMLButtonElement | null = null;

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

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }

  $: if (open && closeBtn) closeBtn.focus();
  $: {
    if (open) window.addEventListener("keydown", onKey);
    else window.removeEventListener("keydown", onKey);
  }
  onDestroy(() => window.removeEventListener("keydown", onKey));
</script>

{#if open}
  <div class="overlay" role="dialog" aria-modal="true" aria-label={t("stock.browser")}>
    <div class="sheet">
      <header>
        <h2>{t("stock.title")}</h2>
        <button bind:this={closeBtn} on:click={() => (open = false)}>{t("common.close")}</button>
      </header>
      <ul role="list">
        {#each $stocks as s (s.id)}
          <li class:active={s.id === $film?.stock}>
            <button class="stock" on:click={() => pick(s.id)}>
              <span class="name">{s.display_name}</span>
              <span class="labels">
                <span class="badge" class:warn={s.provenance === "ArtistAuthored"}>
                  {tEnum("stock.provenance", s.provenance)} · {tEnum("stock.confidence", s.confidence)}
                </span>
                <span class="badge">{tEnum("stock.polarity", s.polarity)}</span>
                <span class="badge">{tEnum("stock.layers", s.layers)}</span>
                <span class="badge">{tEnum("stock.path", s.default_path)}</span>
              </span>
              <span class="text">{s.provenance_text}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if $stocks.length === 0}
        <p class="empty">{t("stock.empty")}</p>
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
    background: var(--bg-elevated);
    color: var(--fg-primary);
    border: 1px solid var(--stroke-strong);
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
    border: 1px solid var(--stroke-subtle);
    background: var(--bg-panel);
    color: inherit;
    cursor: pointer;
  }
  .stock:hover {
    background: var(--bg-hover);
  }
  li.active .stock {
    border-color: var(--accent-400);
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
    color: var(--fg-secondary);
  }
  .badge.warn {
    background: rgba(200, 150, 40, 0.25);
    color: var(--warn);
  }
  .text {
    font-size: 0.72rem;
    color: var(--fg-secondary);
    line-height: 1.35;
  }
  .empty {
    color: var(--fg-secondary);
    font-size: 0.8rem;
    line-height: 1.4;
  }
</style>
