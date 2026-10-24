<script lang="ts">
  /** S6 Batch export queue (UX_SPEC §7, session 08 #23 part 2): rows
   *  (thumb, name, look, size, status), global pause/cancel, per-file
   *  actionable errors (disk full → folder chooser + retry). The queue
   *  renders in a worker thread — this screen is a VIEW, the editor
   *  stays fully usable underneath. */
  import { onMount, onDestroy } from "svelte";
  import {
    batchAdd,
    batchThumb,
    batchPause,
    batchResume,
    batchCancel,
    batchCancelAll,
    batchRetry,
    batchRemove,
    batchClear,
    imageFilesDialog,
    directoryDialog,
    listPresets,
  } from "../lib/ipc";
  import { t, locale } from "../lib/i18n";
  import {
    batchJobs,
    batchPaused,
    presets,
    state,
    lastError,
    toast,
    type BatchJob,
  } from "../lib/stores";
  import Badge from "./Badge.svelte";

  export let onBack: () => void = () => {};
  /** Files dropped on Home while no folder is chosen yet (session 08):
   *  shown as a pending banner — they queue the moment a folder is
   *  picked. Never silently dropped, never auto-picked (consent). */
  export let pendingFiles: string[] | null = null;
  export let onPendingConsumed: () => void = () => {};

  // Add-panel settings (all files of one Add share them).
  let presetId = ""; // "" = current recipe / straight convert
  let format = "png";
  let depth = 16;
  let quality = 92;
  let destDir: string | null = null;
  let destDirName = "";
  let busy = false;

  // Lazy row thumbnails (Rust caches; this map is per-render-session).
  const thumbs: Map<number, string> = new Map();
  const thumbPending = new Set<number>();

  $: depths = format === "png" ? [8, 16] : format === "jpeg" ? [8] : [16];
  $: if (format === "jpeg") depth = 8;
  else if (format === "tiff") depth = 16;
  else if (!depths.includes(depth)) depth = 16;

  $: doneCount = $batchJobs.filter((j) => j.status === "done").length;
  $: activeCount = $batchJobs.filter(
    (j) => j.status === "queued" || j.status === "rendering",
  ).length;
  $: queuedCount = $batchJobs.filter((j) => j.status === "queued").length;
  $: totalBytes = $batchJobs.reduce((s, j) => s + (j.bytes ?? 0), 0);
  $: hasRows = $batchJobs.length > 0;

  /** Live region text (UX_SPEC §10: SR announcements for render status). */
  $: live =
    activeCount === 0
      ? hasRows && doneCount === $batchJobs.length
        ? t("batch.live.done", { n: doneCount })
        : ""
      : t("batch.live.rendering", {
          n: $batchJobs.findIndex((j) => j.status === "rendering") + 1 || 1,
          m: $batchJobs.length,
        });

  // Locale-aware (VI = comma decimals) — UX_SPEC §10, #15; follows live language.
  $: mbFmt = new Intl.NumberFormat($locale === "vi" ? "vi-VN" : "en-US", {
    maximumFractionDigits: 1,
  });

  /** Lazy thumbs: every row without a cached one asks Rust once (the
   * backend caches the PNG — repeated opens are instant). */
  $: for (const j of $batchJobs) {
    if (!thumbs.has(j.id)) ensureThumb(j);
  }

  function fmtBytes(b: number | null): string {
    return b === null ? "" : `${mbFmt.format(b / 1e6)} MB`;
  }

  function lookLabel(j: BatchJob): string {
    if (j.preset_name) return j.preset_name;
    return t("batch.recipe.convert");
  }

  function hintKey(hint: string | null): string | null {
    return hint ? `batch.${hint}` : null;
  }

  async function refreshPresets() {
    try {
      presets.set(await listPresets());
    } catch (e) {
      console.error(e);
    }
  }

  async function ensureThumb(j: BatchJob) {
    if (thumbs.has(j.id) || thumbPending.has(j.id)) return;
    thumbPending.add(j.id);
    try {
      const url = await batchThumb(j.id);
      thumbs.set(j.id, url);
    } catch {
      /* honest glyph: the row shows the file chip, never a spinner */
    } finally {
      thumbPending.delete(j.id);
    }
  }

  async function chooseFolder() {
    const dir = await directoryDialog(t("batch.outFolder"));
    if (dir) {
      destDir = dir;
      destDirName = dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
      // Dropped files waiting for exactly this: queue them now.
      if (pendingFiles && pendingFiles.length > 0) {
        await queuePaths(pendingFiles);
        onPendingConsumed();
      }
    }
  }

  async function queuePaths(paths: string[]) {
    if (!destDir) {
      toast.set(t("batch.folderNotChosen"));
      return;
    }
    busy = true;
    try {
      const snap = await batchAdd(
        paths,
        presetId === "" ? null : presetId,
        { format, depth, quality },
        destDir,
      );
      batchJobs.set(snap.jobs);
      batchPaused.set(snap.paused);
      toast.set(t("batch.added", { n: paths.length }));
    } catch (e) {
      lastError.set(e as never);
    } finally {
      busy = false;
    }
  }

  async function addFiles() {
    if (busy) return;
    if (!destDir) {
      toast.set(t("batch.folderNotChosen"));
      return;
    }
    const paths = await imageFilesDialog();
    if (paths.length === 0) return;
    await queuePaths(paths);
  }

  function apply(snap: { jobs: BatchJob[]; paused: boolean }) {
    batchJobs.set(snap.jobs);
    batchPaused.set(snap.paused);
  }

  async function act(fn: () => Promise<{ jobs: BatchJob[]; paused: boolean }>) {
    try {
      apply(await fn());
    } catch (e) {
      lastError.set(e as never);
    }
  }

  /** Disk-full recovery: pick a folder, retry that row into it. */
  async function retryWithFolder(id: number) {
    const dir = await directoryDialog(t("batch.outFolder"));
    if (!dir) return;
    await act(() => batchRetry(id, dir));
    toast.set(t("batch.retry"));
  }

  function statusClass(s: string): string {
    return `status ${s}`;
  }

  onMount(refreshPresets);
  onDestroy(() => thumbs.clear());
</script>

<section class="batch" aria-label={t("batch.title")}>
  <header class="head">
    <div>
      <h1>{t("batch.title")}</h1>
      {#if hasRows}
        <p class="summary" aria-live="polite">{live}</p>
      {/if}
    </div>
    <button on:click={onBack}>{t("batch.back")}</button>
  </header>

  <div class="adder" role="group" aria-label={t("batch.addFiles")}>
    <button class="primary add" disabled={busy} on:click={addFiles}>
      {t("batch.addFiles")}
    </button>
    <label class="field">
      {t("batch.look")}
      <select bind:value={presetId}>
        <option value="">
          {$state.imageOpen ? t("batch.recipe.current") : t("batch.recipe.convert")}
        </option>
        {#each $presets as p (p.id)}
          <option value={p.id}>{p.name}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      {t("export.format")}
      <select bind:value={format}>
        <option value="png">PNG</option>
        <option value="jpeg">JPEG</option>
        <option value="tiff">TIFF</option>
      </select>
    </label>
    <label class="field">
      {t("export.depth")}
      <select bind:value={depth}>
        {#each depths as d (d)}
          <option value={d}>{d}-bit</option>
        {/each}
      </select>
    </label>
    {#if format === "jpeg"}
      <label class="field">
        {t("export.quality")}
        <input type="range" min="1" max="100" bind:value={quality} />
        <span class="mono">{quality}</span>
      </label>
    {/if}
    <button class="field folder" on:click={chooseFolder} title={destDir ?? ""}>
      {t("batch.outFolder")}: <strong>{destDirName || "—"}</strong>
      <span class="link">{t("batch.chooseFolder")}</span>
    </button>
  </div>

  {#if pendingFiles && pendingFiles.length > 0}
    <div class="pending" role="status">
      <span>{t("batch.added", { n: pendingFiles.length })}</span>
      <button class="primary" on:click={chooseFolder}>{t("batch.chooseFolder")}</button>
      <button on:click={onPendingConsumed}>{t("batch.remove")}</button>
    </div>
  {/if}

  {#if $batchPaused && hasRows}
    <p class="paused" role="status">{t("batch.pausedNote")}</p>
  {/if}

  {#if !hasRows}
    <div class="empty">
      <h2>{t("batch.empty.title")}</h2>
      <p>{t("batch.empty.body")}</p>
    </div>
  {:else}
  <div class="queue-actions">
    <span class="summary">
      {t("batch.summary", {
        done: doneCount,
        active: activeCount,
        queued: queuedCount,
        mb: mbFmt.format(totalBytes / 1e6),
      })}
    </span>
    <span class="spacer"></span>
    {#if $batchPaused}
      <button on:click={() => act(batchResume)}>{t("batch.resume")}</button>
    {:else}
      <button on:click={() => act(batchPause)}>{t("batch.pause")}</button>
    {/if}
    <button on:click={() => act(batchCancelAll)}>{t("batch.cancelQueued")}</button>
    <button on:click={() => act(batchClear)}>{t("batch.clearFinished")}</button>
  </div>

  <ul class="rows" role="list" aria-label={t("batch.title")}>
    {#each $batchJobs as j (j.id)}
      {#key $locale}
        <li class="row {j.status}" role="listitem" aria-label="{j.name} — {t(`batch.status.${j.status}`)}">
          <div class="thumb">
            {#if thumbs.get(j.id)}
              <img src={thumbs.get(j.id)} alt="" width="44" height="44" />
            {:else}
              <span class="glyph" title={j.source}>🖼</span>
            {/if}
          </div>
          <div class="info">
            <span class="name" title={j.source}>{j.name}</span>
            <span class="meta">
              {lookLabel(j)} · {j.format.toUpperCase()} {j.depth}-bit
              {#if j.dims}· {j.dims[0]}×{j.dims[1]}{/if}
            </span>
            {#if j.warnings?.includes("export.lut_skipped_by_film")}
              <span class="warn" title={t("batch.warn.lutSkipped")}>
                <Badge tone="warn">{t("batch.warn.lutSkipped")}</Badge>
              </span>
            {/if}
          </div>
          <span class={statusClass(j.status)}>
            {t(`batch.status.${j.status}`)}
            {#if j.status === "done" && j.bytes}
              · {fmtBytes(j.bytes)}
            {/if}
          </span>
          <div class="row-actions">
            {#if j.status === "queued"}
              <button on:click={() => act(() => batchCancel(j.id))}>{t("batch.cancel")}</button>
            {/if}
            {#if j.status === "error" || j.status === "cancelled"}
              {#if j.error?.code === "io.disk_full" || j.error?.code === "io.dest_missing"}
                <button class="primary" on:click={() => retryWithFolder(j.id)}>
                  {t("batch.chooseFolderRetry")}
                </button>
              {:else}
                <button class="primary" on:click={() => act(() => batchRetry(j.id, null))}>
                  {t("batch.retry")}
                </button>
              {/if}
            {/if}
            {#if j.status !== "rendering"}
              <button on:click={() => act(() => batchRemove(j.id))}>{t("batch.remove")}</button>
            {/if}
          </div>
          {#if j.status === "error" && j.error}
            <div class="error-detail" role="alert">
              <span class="code mono">{j.error.code}</span>
              <span class="msg">{j.error.message}</span>
              {#if hintKey(j.error.hint)}
                <span class="hint">{t(hintKey(j.error.hint) ?? "")}</span>
              {/if}
            </div>
          {/if}
        </li>
      {/key}
    {/each}
  </ul>
  {/if}
</section>

<style>
  .batch {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    padding: var(--space-4);
    gap: var(--space-3);
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
  }
  .head h1 {
    font-size: 20px;
    font-weight: 600;
    margin: 0;
  }
  .summary {
    color: var(--fg-tertiary);
    font-size: 12px;
    margin: 4px 0 0;
  }
  .adder {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    background: var(--bg-panel);
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
    padding: var(--space-3);
  }
  .adder .add {
    padding: 6px 14px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 13px;
  }
  .field.folder {
    cursor: pointer;
    background: none;
    border: 1px dashed var(--stroke-strong);
    border-radius: 6px;
    padding: 4px 10px;
    color: var(--fg-secondary);
  }
  .field.folder .link {
    color: var(--info);
  }
  .paused {
    background: color-mix(in srgb, var(--warn) 14%, var(--bg-panel));
    color: var(--warn);
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 13px;
    margin: 0;
  }
  .pending {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--info) 12%, var(--bg-panel));
    border: 1px solid var(--info);
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 13px;
  }
  .empty {
    text-align: center;
    margin: auto;
    max-width: 460px;
    color: var(--fg-secondary);
  }
  .empty h2 {
    font-size: 16px;
    margin: 0 0 var(--space-2);
  }
  .queue-actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .row {
    display: grid;
    grid-template-columns: 44px 1fr auto auto;
    gap: var(--space-3);
    align-items: center;
    background: var(--bg-panel);
    border: 1px solid var(--stroke-subtle);
    border-radius: 8px;
    padding: var(--space-2) var(--space-3);
  }
  .row.error {
    grid-template-columns: 44px 1fr auto auto;
    grid-template-rows: auto auto;
  }
  .row.error .error-detail {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    font-size: 12px;
    border-top: 1px dashed var(--stroke-subtle);
    padding-top: var(--space-2);
  }
  .error-detail .code {
    color: var(--danger, #e5484d);
  }
  .error-detail .msg {
    color: var(--fg-secondary);
  }
  .error-detail .hint {
    color: var(--fg-primary);
  }
  .thumb {
    width: 44px;
    height: 44px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--bg-viewport);
    display: grid;
    place-items: center;
  }
  .glyph {
    font-size: 18px;
    opacity: 0.5;
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 40ch;
  }
  .meta {
    font-size: 11px;
    color: var(--fg-tertiary);
  }
  .warn {
    font-size: 11px;
  }
  .status {
    font-size: 12px;
    padding: 2px 10px;
    border-radius: 999px;
    border: 1px solid var(--stroke-subtle);
    color: var(--fg-secondary);
    white-space: nowrap;
  }
  .status.rendering {
    color: var(--info);
    border-color: var(--info);
  }
  .status.done {
    color: var(--ok);
    border-color: var(--ok);
  }
  .status.error {
    color: var(--danger, #e5484d);
    border-color: var(--danger, #e5484d);
  }
  .status.cancelled {
    opacity: 0.6;
  }
  .row-actions {
    display: flex;
    gap: var(--space-1);
  }
</style>
