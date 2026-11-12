<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { ActionInfo, Requirement } from "../lib/types";
  let { action, fileIds, onclose }: { action: ActionInfo; fileIds: number[]; onclose: () => void } = $props();
  let reqs = $state<Requirement[] | null>(null);
  let busy = $state(false);
  const missing = $derived((reqs ?? []).filter((r) => !r.ok));
  $effect(() => { api.requirements(action.id).then((r) => (reqs = r)).catch((e) => app.fail(e)); });

  async function run() {
    busy = true;
    try { await api.runAction(action.id, fileIds); app.toast(`${action.name} started on ${fileIds.length} file${fileIds.length === 1 ? "" : "s"}`); onclose(); }
    catch (e) { app.fail(e); } finally { busy = false; }
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="ac-title" style="width: min(560px, calc(100vw - 48px))">
    <header>
      <h2 id="ac-title">{action.name}: {fileIds.length} file{fileIds.length === 1 ? "" : "s"}</h2>
      <p>{action.description} Originals are kept in the system's archive folder for {app.settings?.archive_hold_days ?? 7} days.</p>
    </header>
    <div class="body">
      {#if reqs === null}<p class="faint">Checking tools…</p>
      {:else}
        <ul>
          {#each reqs as r}
            <li><span class="dot" style="background: {r.ok ? 'var(--ok)' : 'var(--bad)'}"></span><span><b>{r.name}</b>{#if !r.ok}<br /><span class="muted">{r.hint}</span>{/if}</span></li>
          {/each}
        </ul>
      {/if}
    </div>
    <footer>
      {#if missing.length}<span class="left">Install what's missing, or set its path in Settings.</span><button class="btn" onclick={() => { app.view = "settings"; onclose(); }}>Open settings</button>{/if}
      <button class="btn ghost" onclick={onclose}>Cancel</button>
      <button class="btn primary" disabled={busy || reqs === null || missing.length > 0} onclick={run}>Run {action.name}</button>
    </footer>
  </div>
</div>

<style>
  ul { list-style: none; padding: 0; margin: 4px 0 12px; display: flex; flex-direction: column; gap: 8px; }
  li { display: flex; gap: 10px; align-items: baseline; }
</style>
