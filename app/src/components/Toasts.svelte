<script lang="ts">
  import { app } from "../lib/state.svelte";
</script>
<div class="toasts" aria-live="polite">
  {#each app.toasts as t (t.id)}
    <div class="toast {t.kind}" role={t.kind === "error" ? "alert" : "status"}>{t.text}</div>
  {/each}
</div>
<style>
  .toasts { position: fixed; right: 18px; bottom: 54px; display: flex; flex-direction: column; gap: 8px; z-index: 60; max-width: 460px; }
  .toast { display: flex; gap: 10px; align-items: baseline; background: var(--panel); border: 1px solid var(--seam); padding: 9px 13px; border-radius: var(--radius-s); box-shadow: var(--shadow-pop); white-space: pre-wrap; }
  .toast::before { content: ""; flex: none; width: 7px; height: 7px; border-radius: 50%; background: var(--ok); box-shadow: 0 0 0 2px var(--ok-bg); transform: translateY(-1px); }
  .toast.error::before { background: var(--bad); box-shadow: 0 0 0 2px var(--bad-bg); }
</style>
