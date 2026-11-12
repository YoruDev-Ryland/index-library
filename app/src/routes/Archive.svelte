<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { basename, bytes, day, relDays } from "../lib/format";
  import { placeOf, systemFolder } from "../lib/files";
  import type { ArchiveEntry } from "../lib/types";

  let list = $state<ArchiveEntry[] | null>(null);
  let busy = $state(false);
  let confirmEmpty = $state(false);
  let reason = $state<string>("all");
  const load = () => api.archiveList().then((l) => (list = l)).catch((e) => app.fail(e));
  $effect(() => { void app.revision; load(); });
  const roots = $derived(app.settings?.library_roots ?? []);
  const kept = (e: ArchiveEntry) => e.delete_after > 1e17;
  const total = $derived((list ?? []).filter((e) => !kept(e)).reduce((a, e) => a + e.size, 0));
  const purgeable = $derived((list ?? []).filter((e) => !kept(e)).length);
  const now = Date.now() / 1000;

  // why each file is here, in words
  const reasonName = (r: string | null) => r === "duplicate" ? "Duplicate" : r === "cleanup" ? "Cleanup" : r ? r.charAt(0).toUpperCase() + r.slice(1) : "Other";
  const reasonTone = (r: string | null) => r === "duplicate" ? "info" : r === "cleanup" ? "plain" : "warn";
  const reasons = $derived([...new Set((list ?? []).map((e) => reasonName(e.reason)))]);
  const shown = $derived((list ?? []).filter((e) => reason === "all" || reasonName(e.reason) === reason));
  // one card per system, biggest first
  const groups = $derived.by(() => {
    const m = new Map<string, ArchiveEntry[]>();
    for (const e of shown) {
      const k = e.system_id ? app.systemName(e.system_id) : systemFolder(e.original_path, roots) || "Other";
      m.set(k, [...(m.get(k) ?? []), e]);
    }
    return [...m.entries()].map(([name, items]) => ({ name, items: items.sort((a, b) => b.archived_at - a.archived_at), size: items.reduce((a, e) => a + e.size, 0) })).sort((a, b) => b.size - a.size);
  });

  async function run(f: () => Promise<unknown>, ok: string) {
    busy = true;
    try { await f(); app.toast(ok); await load(); app.refresh(); } catch (e) { app.fail(e); } finally { busy = false; confirmEmpty = false; }
  }
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Archive</h1>
      <p>Files removed by duplicates, cleanup or an action wait here for {app.settings?.archive_hold_days ?? 7} days, so you can check nothing's missing first. Each system keeps its archive in an <code>_archive</code> folder that every PC using this library can see.</p>
    </div>
    <span class="spacer"></span>
    {#if purgeable}<button class="btn danger" disabled={busy} onclick={() => (confirmEmpty = true)}>Empty archive ({bytes(total)})</button>{/if}
  </div>
  {#if list === null}<p class="faint">Loading…</p>
  {:else if list.length === 0}<div class="empty"><h2>The archive is empty</h2><p>When a file is removed or replaced, it lands here first.</p></div>
  {:else}
    {#if reasons.length > 1}
      <div class="page-tabs" role="tablist">
        <button role="tab" aria-selected={reason === "all"} class:on={reason === "all"} onclick={() => (reason = "all")}>All <span>{list.length}</span></button>
        {#each reasons as r (r)}<button role="tab" aria-selected={reason === r} class:on={reason === r} onclick={() => (reason = r)}>{r} <span>{list.filter((e) => reasonName(e.reason) === r).length}</span></button>{/each}
      </div>
    {/if}
    {#each groups as g (g.name)}
      <section class="gcard">
        <div class="gcard-head">
          <div class="grow"><h3>{g.name} <span class="count">{g.items.length} file{g.items.length === 1 ? "" : "s"} · {bytes(g.size)}</span></h3></div>
        </div>
        <ul class="grows">
          {#each g.items as e (e.id)}
            <li>
              <span class="gpill {reasonTone(e.reason)}">{reasonName(e.reason)}</span>
              <div class="grow-body">
                <div class="grow-name">{basename(e.original_path)}</div>
                <div class="grow-meta">
                  <span>{bytes(e.size)}</span>
                  <span>was in {placeOf(e.original_path, roots)}</span>
                  {#if kept(e)}<span class="hold">kept until you restore it</span>
                  {:else}<span class:due={e.delete_after <= now}>deleted {relDays(e.delete_after)} ({day(e.delete_after)})</span>{/if}
                </div>
              </div>
              <div class="grow-actions">
                <button class="btn small" disabled={busy} onclick={() => run(() => api.archiveRestore(e.id), `Restored ${basename(e.original_path)}`)}>Restore</button>
                <button class="btn small ghost" disabled={busy} title="Wait another {app.settings?.archive_hold_days ?? 7} days before deleting" onclick={() => run(() => api.archiveExtend([e.id], app.settings?.archive_hold_days ?? 7), "Hold extended")}>Keep longer</button>
                <button class="btn small ghost del" disabled={busy} onclick={() => run(() => api.archivePurge([e.id]), `Deleted ${basename(e.path)}`)}>Delete</button>
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  {/if}
</div>

{#if confirmEmpty}
  <div class="modal-backdrop">
    <div class="modal" role="alertdialog" aria-modal="true" style="width: min(520px, calc(100vw - 48px))">
      <header><h2>Delete every archived file?</h2><p>This permanently deletes {purgeable} files ({bytes(total)}) from the NAS. Only do this once you've checked nothing you need is among them. Files marked “kept until you restore it” are not deleted.</p></header>
      <footer>
        <button class="btn ghost" onclick={() => (confirmEmpty = false)}>Cancel</button>
        <button class="btn primary" style="background: var(--bad); border-color: var(--bad)" disabled={busy} onclick={() => run(() => api.archivePurgeAll(), "Archive emptied")}>Delete {purgeable} files</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .spacer { flex: 1; }
  .due { color: var(--warn); font-weight: 600; }
  /* deleting is permanent: the button only shows on the row you're on, and turns red when aimed at */
  .grows li :global(.del) { opacity: 0; color: var(--ink-soft); }
  .grows li:hover :global(.del), .grows li:focus-within :global(.del) { opacity: 1; }
  .grows li :global(.del:hover), .grows li :global(.del:focus-visible) { color: var(--bad); }
  .hold { color: var(--warn); }
  code { font-size: 13px; background: var(--panel-2); padding: 0 4px; border-radius: 3px; }
</style>
