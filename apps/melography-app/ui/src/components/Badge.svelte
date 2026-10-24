<script lang="ts">
  export let tone: "ok" | "warn" | "info" | "muted" | "danger" = "muted";
  export let title: string | undefined = undefined;
</script>

<span class="badge {tone}" {title}>
  <slot />
</span>

<style>
  .ok { color: var(--ok); border-color: var(--ok); }
  .warn { color: var(--warn); border-color: var(--warn); }
  .info { color: var(--info); border-color: var(--info); }
  .muted { color: var(--fg-secondary); }
  .danger { color: var(--danger); border-color: var(--danger); }
</style>
