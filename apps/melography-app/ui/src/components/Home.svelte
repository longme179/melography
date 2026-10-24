<script lang="ts">
  /** S1 Home (UX_SPEC §3, M4 #23): recents with provenance lines, a big
   *  drop zone, an honest empty state with format chips (HEIC/AVIF
   *  experimental per the M0 codec matrix). Broken files are inline
   *  error rows — never modals. The window-wide drag-drop lives here
   *  (the Tauri webview event, not HTML5 DnD — path access). */
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { t, locale } from "../lib/i18n";
  import { recents, toast } from "../lib/stores";
  import { listRecents, purgeRecents, type RecentItem } from "../lib/ipc";

  export let onOpenPath: (path: string) => void = () => {};
  export let onOpenDialog: () => void = () => {};
  /** Multi-file drop → S6 batch queue (session 08): one file opens, a
   *  handful becomes a batch — the two intents stay distinct. */
  export let onBatchFiles: (paths: string[]) => void = () => {};

  let dragOver = false;
  let unlisten: (() => void) | null = null;
  let hoverPath: string | null = null;

  async function refresh() {
    try {
      recents.set(await listRecents());
    } catch (e) {
      console.error("recents list failed", e);
    }
  }

  async function purge() {
    try {
      const n = await purgeRecents();
      toast.set(t("home.purged", { n }));
      await refresh();
    } catch (e) {
      toast.set(String(e));
    }
  }

  function open(r: RecentItem) {
    if (!r.missing) onOpenPath(r.path);
  }

  function fmtDate(iso: string): string {
    if (!iso) return "";
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    return d.toLocaleString($locale === "vi" ? "vi-VN" : "en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    });
  }

  onMount(async () => {
    await refresh();
    // Window-wide drop (Tauri v2 drag-drop event; HTML5 drops cannot see
    // file paths in the webview sandbox).
    unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over") {
        dragOver = true;
        hoverPath = event.payload.paths[0] ?? null;
      } else if (event.payload.type === "drop") {
        dragOver = false;
        const paths = event.payload.paths ?? [];
        if (paths.length > 1) {
          onBatchFiles(paths); // a handful → the queue (P2's flow)
        } else if (paths[0]) {
          onOpenPath(paths[0]);
        }
      } else {
        dragOver = false;
        hoverPath = null;
      }
    });
  });
  onDestroy(() => unlisten?.());
</script>

<section class="home" class:dragover={dragOver} aria-label={t("home.title")}>
  <div class="hero">
    <h1>MELOGRAPHY</h1>
    <p class="tagline">{t("home.drop")}</p>
    <div class="chips" aria-hidden="true">
      {#each ["JPEG", "PNG", "TIFF", "WebP"] as f (f)}
        <span class="chip">{f}</span>
      {/each}
      <span class="chip experimental" title={t("home.heicBadge")}>HEIC / AVIF</span>
    </div>
    <button class="primary" on:click={onOpenDialog}>{t("home.open")}</button>
  </div>

  {#if $recents.length}
    <div class="recents">
      <div class="recents-head">
        <h2>{t("home.recents")}</h2>
        {#if $recents.some((r) => r.missing)}
          <button class="link" on:click={purge}>{t("home.purgeBroken")}</button>
        {/if}
      </div>
      <ul role="list">
        {#each $recents as r (r.path)}
          <!-- Real <button> inside the li (#15 a11y: a li may not carry
               role=button; native buttons bring keyboard + focus free). -->
          <li class="recent-row" class:missing={r.missing}>
            <button
              class="recent"
              aria-disabled={r.missing}
              aria-label={r.missing ? `${r.name} — ${t("home.missing")}` : r.name}
              on:click={() => open(r)}
            >
              <img
                class="thumb"
                src={r.thumb_data_url ?? ""}
                alt=""
                width="48"
                height="48"
                loading="lazy"
              />
              <div class="meta">
                <span class="name">{r.name}</span>
                <span class="path mono">{r.path}</span>
                <span class="line">
                  {#if r.missing}
                    <span class="err">⚠ {t("home.missing")}</span>
                  {:else}
                    {#if r.width}
                      <span class="mono">{r.width}×{r.height}</span>
                      <span>·</span>
                    {/if}
                    <span class={r.has_recipe ? "has-recipe" : ""}>
                      {r.has_recipe ? t("home.recipe") : t("home.noRecipe")}
                    </span>
                    {#if r.last_opened_utc}
                      <span>·</span>
                      <span>{t("home.lastOpened")} {fmtDate(r.last_opened_utc)}</span>
                    {/if}
                  {/if}
                </span>
              </div>
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {:else}
    <p class="empty">{t("home.empty")}</p>
  {/if}
</section>

<style>
  .home {
    height: 100%;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-4);
    gap: var(--space-4);
  }
  .home.dragover::after {
    content: "⤓";
    position: fixed;
    inset: 8px;
    border: 2px dashed var(--accent-400);
    border-radius: 12px;
    background: color-mix(in srgb, var(--bg-app) 80%, transparent);
    display: grid;
    place-items: center;
    font-size: 64px;
    color: var(--accent-400);
    pointer-events: none;
  }
  .hero {
    text-align: center;
    margin-top: 6vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
  }
  .hero h1 {
    font-size: 28px;
    letter-spacing: 0.12em;
    margin: 0;
    font-weight: 300;
  }
  .tagline {
    color: var(--fg-secondary);
    margin: 0;
    font-size: 15px;
  }
  .chips {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
    justify-content: center;
  }
  .chip {
    font-size: 11px;
    padding: 2px 10px;
    border-radius: 999px;
    border: 1px solid var(--stroke-subtle);
    color: var(--fg-secondary);
  }
  .chip.experimental {
    border-style: dashed;
    color: var(--warn);
    border-color: var(--warn);
  }
  .recents {
    width: min(720px, 100%);
  }
  .recents-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .recents-head h2 {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 var(--space-2);
  }
  .link {
    background: none;
    border: none;
    color: var(--info);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .recent-row {
    display: block;
  }
  .recent-row.missing .recent {
    cursor: default;
    opacity: 0.7;
    border-style: dashed;
    border-color: var(--danger);
  }
  .recent {
    display: flex;
    width: 100%;
    text-align: left;
    gap: var(--space-3);
    align-items: center;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
    cursor: pointer;
    background: var(--bg-panel);
    font: inherit;
    color: inherit;
  }
  .recent-row:not(.missing) .recent:hover,
  .recent-row:not(.missing) .recent:focus-visible {
    background: var(--bg-hover);
  }
  .thumb {
    width: 48px;
    height: 48px;
    object-fit: cover;
    border-radius: 6px;
    background: var(--bg-viewport);
    flex-shrink: 0;
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .path {
    font-size: 11px;
    color: var(--fg-tertiary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    font-size: 11px;
    color: var(--fg-tertiary);
    display: flex;
    gap: 6px;
  }
  .has-recipe {
    color: var(--ok);
  }
  .err {
    color: var(--danger);
  }
  .empty {
    color: var(--fg-tertiary);
    font-size: 13px;
    max-width: 460px;
    text-align: center;
  }
</style>
