<script lang="ts">
  /** S5 Preset manager (UX_SPEC §2, M4 #23): save the current recipe as a
   *  named preset, rename/delete/apply with provenance. User presets are
   *  clearly distinguished from shipped stock profiles (which live in the
   *  Film Lab browser — data with calibration provenance, not recipes). */
  import { onMount } from "svelte";
  import { t } from "../lib/i18n";
  import { presets, toast, type UserPreset } from "../lib/stores";
  import {
    listPresets,
    savePreset,
    renamePreset,
    deletePreset,
    applyPreset,
  } from "../lib/ipc";

  export let open = false;
  export let canApply = false;
  export let onApplied: () => void = () => {};

  let newName = "";
  let renaming: UserPreset | null = null;
  let renameValue = "";
  let busy = false;

  async function refresh() {
    try {
      presets.set(await listPresets());
    } catch (e) {
      toast.set(String(e));
    }
  }

  async function save() {
    const name = newName.trim();
    if (!name || busy) return;
    busy = true;
    try {
      await savePreset(name);
      newName = "";
      toast.set(t("presets.title") + " ✓");
      await refresh();
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  async function apply(p: UserPreset) {
    if (busy) return;
    busy = true;
    try {
      await applyPreset(p.id);
      toast.set(t("presets.applied"));
      onApplied();
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  function startRename(p: UserPreset) {
    renaming = p;
    renameValue = p.name;
  }

  async function commitRename() {
    const p = renaming;
    const name = renameValue.trim();
    renaming = null;
    if (!p || !name || name === p.name) return;
    try {
      presets.set(await renamePreset(p.id, name));
    } catch (e) {
      toast.set(String(e));
    }
  }

  async function remove(p: UserPreset) {
    try {
      presets.set(await deletePreset(p.id));
    } catch (e) {
      toast.set(String(e));
    }
  }

  onMount(refresh);
  $: if (open) refresh();

  function focus(node: HTMLInputElement) {
    node.focus();
    node.select();
    return { destroy() {} };
  }
</script>

{#if open}
  <div class="overlay" role="dialog" aria-modal="true" aria-label={t("presets.title")}
    on:click|self={() => (open = false)}
    on:keydown={(e) => e.key === "Escape" && (open = false)}
  >
    <div class="sheet" tabindex="-1">
      <div class="head">
        <h2>{t("presets.title")}</h2>
        <button class="close" on:click={() => (open = false)} aria-label="✕">✕</button>
      </div>

      <div class="save-row">
        <input
          type="text"
          placeholder={t("presets.namePrompt")}
          bind:value={newName}
          on:keydown={(e) => e.key === "Enter" && save()}
        />
        <button class="primary" disabled={!newName.trim() || busy} on:click={save}>
          {t("presets.saveBtn")}
        </button>
      </div>

      <div class="groups">
        <h3>{t("presets.userPresets")}</h3>
        {#if $presets.length}
          <ul role="list">
            {#each $presets as p (p.id)}
              <li class="preset">
                {#if renaming?.id === p.id}
                  <input
                    class="rename"
                    type="text"
                    bind:value={renameValue}
                    on:blur={commitRename}
                    on:keydown={(e) => {
                      if (e.key === "Enter") commitRename();
                      if (e.key === "Escape") (renaming = null);
                    }}
                    use:focus
                  />
                {:else}
                  <div class="meta">
                    <span class="name">{p.name}</span>
                    <span class="summary mono">{p.summary}</span>
                    <span class="line">{t("presets.provenance")} · {p.modified_utc.slice(0, 10)}</span>
                  </div>
                  <div class="actions">
                    <button disabled={!canApply || busy} on:click={() => apply(p)}>
                      {t("presets.apply")}
                    </button>
                    <button on:click={() => startRename(p)}>{t("presets.rename")}</button>
                    <button class="danger" on:click={() => remove(p)}>{t("presets.delete")}</button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {:else}
          <p class="empty">{t("presets.empty")}</p>
        {/if}

        <h3 class="muted">{t("presets.stockProfiles")}</h3>
        <p class="empty">
          <button class="link" on:click={() => (open = false)}>
            {t("panel.film")} →
          </button>
        </p>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: grid;
    place-items: center;
    z-index: 40;
  }
  .sheet {
    width: min(560px, 92vw);
    max-height: 80vh;
    overflow-y: auto;
    background: var(--bg-panel);
    border: 1px solid var(--stroke-strong);
    border-radius: 12px;
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .head h2 {
    margin: 0;
    font-size: 17px;
  }
  .close {
    background: none;
    border: none;
    font-size: 15px;
    cursor: pointer;
    color: var(--fg-secondary);
  }
  .save-row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--space-2);
  }
  .save-row input {
    width: 100%;
  }
  .groups h3 {
    font-size: 13px;
    color: var(--fg-secondary);
    margin: var(--space-2) 0 var(--space-1);
  }
  .groups h3.muted {
    opacity: 0.7;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .preset {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
  }
  .meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
  }
  .summary {
    font-size: 11px;
    color: var(--fg-tertiary);
  }
  .line {
    font-size: 11px;
    color: var(--fg-tertiary);
  }
  .actions {
    display: flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .actions button {
    font-size: 11px;
    padding: 3px 8px;
  }
  .danger {
    color: var(--danger);
  }
  .rename {
    width: 100%;
  }
  .empty {
    font-size: 12px;
    color: var(--fg-tertiary);
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    color: var(--info);
    cursor: pointer;
    padding: 0;
    font-size: 12px;
  }
</style>
