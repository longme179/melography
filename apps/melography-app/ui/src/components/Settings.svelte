<script lang="ts">
  /** S7 Settings (UX_SPEC §7, M4 #23): General / Color / Performance /
   *  Models / Updates / Privacy / Advanced. Every section says exactly
   *  what it does — the offline explainer and the GPU honesty note are
   *  requirements, not decoration. Persisted via the durable settings
   *  store; language + theme apply live. */
  import { onMount } from "svelte";
  import { t, setLang, locale, type Lang } from "../lib/i18n";
  import { settings, toast, type AppSettings } from "../lib/stores";
  import { getSettings, setSettings } from "../lib/ipc";
  import { applyTheme } from "../lib/theme";

  export let open = false;

  let draft: AppSettings | null = null;

  async function refresh() {
    try {
      const s = await getSettings();
      settings.set(s);
      draft = { ...s };
    } catch (e) {
      console.error(e);
    }
  }

  async function save() {
    if (!draft) return;
    try {
      const s = await setSettings(draft);
      settings.set(s);
      draft = { ...s };
      // live-apply language + theme
      setLang(s.language as Lang);
      applyTheme(s.theme);
      toast.set(t("settings.saved"));
    } catch (e) {
      toast.set(String(e));
    }
  }

  function field<K extends keyof AppSettings>(key: K): AppSettings[K] {
    return draft ? draft[key] : (null as unknown as AppSettings[K]);
  }

  function setField<K extends keyof AppSettings>(key: K, v: AppSettings[K]) {
    if (draft) draft = { ...draft, [key]: v };
  }

  onMount(refresh);
  $: if (open) refresh();
  $: locale; // re-render translated labels on language switch

  // Esc via the window (#15 a11y): works regardless of what holds focus.
  // Backdrop-click dismiss was REMOVED on purpose — accidental dismissal
  // of an unsaved settings draft is the worse failure (UX_SPEC §9).
  function onWindowKey(e: KeyboardEvent) {
    if (open && e.key === "Escape") open = false;
  }
</script>

<svelte:window on:keydown={onWindowKey} />

{#if open}
  <div
    class="overlay"
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.title")}
  >
    <div class="sheet" tabindex="-1">
      <div class="head">
        <h2>{t("settings.title")}</h2>
        <button class="close" on:click={() => (open = false)} aria-label={t("common.close")}>✕</button>
      </div>

      {#if draft}
        <section>
          <h3>{t("settings.general")}</h3>
          <label class="row">
            <span>{t("settings.language")}</span>
            <select value={field("language")} on:change={(e) => setField("language", e.currentTarget.value)}>
              <option value="en">English</option>
              <option value="vi">Tiếng Việt</option>
            </select>
          </label>
          <label class="row">
            <span>{t("settings.theme")}</span>
            <select value={field("theme")} on:change={(e) => setField("theme", e.currentTarget.value)}>
              <option value="dark">{t("settings.theme.dark")}</option>
              <option value="light">{t("settings.theme.light")}</option>
              <option value="contrast">{t("settings.theme.contrast")}</option>
            </select>
          </label>
        </section>

        <section>
          <h3>{t("settings.color")}</h3>
          <p class="note">{t("settings.colorNote")}</p>
        </section>

        <section>
          <h3>{t("settings.performance")}</h3>
          <label class="row" title={t("settings.previewSize.tip")}>
            <span>{t("settings.previewSize")}</span>
            <input
              type="number"
              min="256"
              max="2560"
              step="128"
              value={field("preview_max_dim")}
              on:change={(e) => setField("preview_max_dim", Number(e.currentTarget.value))}
            />
          </label>
          <label class="row" title={t("settings.cacheBudget.tip")}>
            <span>{t("settings.cacheBudget")}</span>
            <input
              type="number"
              min="64"
              max="4096"
              step="64"
              value={field("cache_budget_mb")}
              on:change={(e) => setField("cache_budget_mb", Number(e.currentTarget.value))}
            />
          </label>
          <label class="row" title={t("settings.gpuNote")}>
            <span>{t("settings.gpu")}</span>
            <input
              type="checkbox"
              checked={field("gpu_enabled")}
              on:change={(e) => setField("gpu_enabled", e.currentTarget.checked)}
            />
          </label>
          <p class="note">{t("settings.gpuNote")}</p>
        </section>

        <section>
          <h3>{t("settings.updates")}</h3>
          <label class="row">
            <span>{t("settings.channel")}</span>
            <select value={field("update_channel")} on:change={(e) => setField("update_channel", e.currentTarget.value)}>
              <option value="ask">{t("settings.channel.ask")}</option>
              <option value="none">{t("settings.channel.none")}</option>
              <option value="stable">{t("settings.channel.stable")}</option>
              <option value="beta">{t("settings.channel.beta")}</option>
            </select>
          </label>
          <p class="note">{t("settings.updatesNote")}</p>
        </section>

        <section>
          <h3>{t("settings.privacy")}</h3>
          <label class="row">
            <span>{t("settings.offline")}</span>
            <input
              type="checkbox"
              checked={field("offline_mode")}
              on:change={(e) => setField("offline_mode", e.currentTarget.checked)}
            />
          </label>
          <p class="note">{t("settings.privacyNote")}</p>
        </section>

        <section>
          <h3>{t("settings.advanced")}</h3>
          <label class="row">
            <span>{t("settings.logLevel")}</span>
            <select value={field("log_level")} on:change={(e) => setField("log_level", e.currentTarget.value)} aria-label={t("settings.logLevel")}>
              <option value="error">{t("settings.loglevel.error")}</option>
              <option value="warn">{t("settings.loglevel.warn")}</option>
              <option value="info">{t("settings.loglevel.info")}</option>
              <option value="debug">{t("settings.loglevel.debug")}</option>
            </select>
          </label>
          <div class="row">
            <span>{t("settings.cacheLocation")}</span>
            <span class="mono value">~/.melography</span>
          </div>
        </section>

        <div class="actions">
          <button class="primary" on:click={save}>{t("presets.saveBtn")}</button>
        </div>
      {/if}
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
    z-index: 50;
  }
  .sheet {
    width: min(620px, 92vw);
    max-height: 84vh;
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
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  section h3 {
    font-size: 13px;
    color: var(--fg-secondary);
    margin: 0;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    font-size: 13px;
  }
  .row select,
  .row input[type="number"] {
    min-width: 180px;
  }
  .value {
    font-size: 12px;
    color: var(--fg-tertiary);
  }
  .note {
    font-size: 11.5px;
    color: var(--fg-tertiary);
    margin: 0;
    line-height: 1.45;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--stroke-subtle);
  }
</style>
