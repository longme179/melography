<script lang="ts">
  // Error state (UX_SPEC §9): message + cause + next action, localized from
  // the Rust taxonomy codes; inline banner, never a dead end.
  import { t, tErrorCode } from "../lib/i18n";
  import { lastError } from "../lib/stores";
  import { onMount } from "svelte";

  let detail = "";

  $: if ($lastError) {
    detail = tErrorCode($lastError.code, $lastError.message);
  }

  function dismiss() {
    lastError.set(null);
  }

  // Screen-reader announcement (a11y, UX_SPEC §10).
  onMount(() => {
    return () => {};
  });
</script>

{#if $lastError}
  <div class="banner" role="alert" aria-live="assertive">
    <div class="body">
      <strong>{t("error.title")}</strong>
      <span>{detail}</span>
      {#if $lastError.context}
        <code class="mono context">{$lastError.context}</code>
      {/if}
    </div>
    <button on:click={dismiss}>{t("error.dismiss")}</button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    background: color-mix(in srgb, var(--danger) 12%, var(--bg-elevated));
    border: 1px solid var(--danger);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    margin: var(--space-2) var(--space-3);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    color: var(--fg-primary);
  }
  .context {
    color: var(--fg-tertiary);
    font-size: 11px;
  }
</style>
