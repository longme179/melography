<script lang="ts">
  /** Model manager (M3, AI_MODELS §6): registry states, offline install
   *  from file (owner-side evaluation path), delete removes all files
   *  immediately. v1 ships an EMPTY registry — nothing passed the vetting
   *  gate (issue #14) — and the honest empty state says so. */
  import {
    deleteModel,
    installModelFromFile,
    listModels,
    manifestFileDialog,
    weightsFileDialog,
  } from "../lib/ipc";
  import { t, fmtNum } from "../lib/i18n";
  import { models, toast, type ModelState } from "../lib/stores";

  let busy = false;
  let open = false;

  async function refresh() {
    try {
      models.set(await listModels());
    } catch (e) {
      toast.set(String(e));
    }
  }

  async function install() {
    if (busy) return;
    busy = true;
    try {
      const manifest = await manifestFileDialog();
      if (!manifest) return;
      const weights = await weightsFileDialog();
      if (!weights) return;
      models.set(await installModelFromFile(manifest, weights));
      toast.set(t("models.installed"));
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  async function remove(id: string) {
    if (busy) return;
    busy = true;
    try {
      models.set(await deleteModel(id));
    } catch (e) {
      toast.set(String(e));
    } finally {
      busy = false;
    }
  }

  function stateLabel(s: ModelState): string {
    switch (s.state) {
      case "ready":
        return t("models.state.ready", { version: s.version });
      case "not_installed":
        return t("models.state.not_installed");
      case "downloading":
        return t("models.state.downloading");
      case "verifying":
        return t("models.state.verifying");
      case "installing":
        return t("models.state.installing");
      case "corrupt":
        return t("models.state.corrupt");
      case "missing":
        return t("models.state.missing");
    }
  }

  function fmtSize(bytes: number): string {
    // Locale-aware decimal separator (VI = comma) — UX_SPEC §10, #15.
    if (bytes >= 1024 * 1024) return `${fmtNum(bytes / 1024 / 1024, 1)} MB`;
    return `${fmtNum(bytes / 1024, 1)} kB`;
  }

  refresh();
</script>

<section class="panel">
  <div class="head">
    <h2>{t("models.title")}</h2>
    <button class="link" on:click={() => (open = !open)}>
      {open ? t("models.collapse") : t("models.expand")}
    </button>
  </div>

  {#if open}
    {#if $models.length === 0}
      <p class="hint">{t("models.empty")}</p>
    {:else}
      <ul class="list">
        {#each $models as m (m.id)}
          <li>
            <div class="row">
              <span class="id mono">{m.id}</span>
              <span class="badge" class:ok={m.state.state === "ready"}>
                {stateLabel(m.state)}
              </span>
            </div>
            <div class="meta">
              v{m.version} · {fmtSize(m.size_bytes)} · {m.weights_license}
            </div>
            {#if m.known_issues.length}
              <div class="meta warn">{m.known_issues[0]}</div>
            {/if}
            <button disabled={busy} on:click={() => remove(m.id)}>
              {t("models.delete")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    <button disabled={busy} on:click={install}>{t("models.install")}</button>
    <p class="hint">{t("models.note")}</p>
  {/if}
</section>

<style>
  .panel {
    padding: 0 var(--space-3) var(--space-3);
    border-bottom: 1px solid var(--stroke-subtle);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    color: var(--accent, #7aa2f7);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  .hint {
    font-size: 11px;
    color: var(--fg-tertiary);
    margin: var(--space-2) 0 0;
  }
  .list {
    list-style: none;
    margin: var(--space-2) 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .list li {
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
    padding: var(--space-2);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
  }
  .id {
    font-size: 12px;
  }
  .badge {
    font-size: 11px;
    border: 1px solid var(--stroke-subtle);
    border-radius: 999px;
    padding: 1px 8px;
    color: var(--fg-secondary);
  }
  .badge.ok {
    color: var(--ok, #9ece6a);
    border-color: var(--ok, #9ece6a);
  }
  .meta {
    font-size: 11px;
    color: var(--fg-tertiary);
  }
  .meta.warn {
    color: var(--warn, #e0af68);
  }
  button {
    background: transparent;
    border: 1px solid var(--stroke-subtle);
    border-radius: 6px;
    padding: 3px 10px;
    cursor: pointer;
    font-size: 12px;
    color: inherit;
    align-self: flex-start;
  }
</style>
