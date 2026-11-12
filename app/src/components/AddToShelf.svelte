<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { blankCopy } from "../lib/shelf";
  import type { CopyFields, ReleaseHit } from "../lib/types";
  import CopyForm from "./CopyForm.svelte";

  let { system: initialSystem = null, release: initialRelease = null, onclose, onadded }: {
    system?: string | null; release?: string | null; onclose: () => void; onadded?: () => void;
  } = $props();

  // where the dialog starts: read once
  const start = untrack(() => ({ system: initialSystem, release: initialRelease }));
  let system = $state(start.system ?? app.filter.system ?? app.systems[0]?.id ?? "snes");
  let query = $state("");
  let hits = $state<ReleaseHit[]>([]);
  let searching = $state(false);
  /** the release picked: from the list, or typed */
  let picked = $state<{ release: string; title: string; regions: string[]; typed: boolean } | null>(start.release ? { release: start.release, title: start.release, regions: [], typed: false } : null);
  let region = $state("");
  let fields = $state<CopyFields>(blankCopy());
  let busy = $state(false);
  let added = $state(0);

  let timer: ReturnType<typeof setTimeout>;
  $effect(() => {
    const q = query.trim(), s = system;
    clearTimeout(timer);
    if (!q) { hits = []; return; }
    timer = setTimeout(async () => {
      searching = true;
      try { hits = await api.shelfReleases(s, q); } catch (e) { app.fail(e); } finally { searching = false; }
    }, 160);
  });

  async function add(another: boolean) {
    if (!picked) return;
    busy = true;
    try {
      await api.shelfAdd(system, picked.release, picked.typed && region.trim() ? region.trim() : null, $state.snapshot(fields));
      added++;
      app.toast(`On your shelf: ${picked.title}`);
      app.refresh();
      onadded?.();
      if (another) { picked = null; query = ""; hits = []; fields = { ...blankCopy(), location: fields.location }; }
      else onclose();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const SOURCE: Record<ReleaseHit["source"], string> = { list: "From the console's list", titledb: "From the title database", catalog: "From IGDB's catalog" };
</script>

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="as-title" style="width: min(680px, calc(100vw - 48px))">
    <header>
      <h2 id="as-title">Add to your shelf</h2>
      <p>{picked ? "Describe your copy." : "Find the exact release you have, then describe your copy."}{added ? ` ${added} added so far.` : ""}</p>
    </header>
    <div class="body">
      {#if !picked}
        <div class="find">
          <select bind:value={system} aria-label="Console">
            {#each app.allSystems as [id, name] (id)}<option value={id}>{name}</option>{/each}
          </select>
          <!-- svelte-ignore a11y_autofocus -->
          <input type="search" placeholder="Search the console's games" bind:value={query} autofocus aria-label="Search releases" />
        </div>
        {#if hits.length}
          <p class="src">{SOURCE[hits[0].source]}</p>
          <ul class="hits">
            {#each hits as h (h.release)}
              <li><button onclick={() => (picked = { release: h.release, title: h.title, regions: h.regions, typed: h.source !== "list" })}>
                <span class="t">{h.title}</span><span class="r">{h.release}</span>
              </button></li>
            {/each}
          </ul>
        {:else if query.trim() && !searching}
          <p class="none">Nothing in the list matches. You can add it as you typed it:</p>
        {/if}
        {#if query.trim() && !searching}
          <button class="btn small typed" onclick={() => (picked = { release: query.trim(), title: query.trim(), regions: [], typed: true })}>Add “{query.trim()}” as typed</button>
        {/if}
      {:else}
        <div class="picked">
          <div><b>{picked.title}</b><span class="r">{picked.release} · {app.systemName(system)}</span></div>
          <button class="btn small ghost" onclick={() => (picked = null)}>Change</button>
        </div>
        {#if picked.typed && !picked.release.includes("(")}
          <label class="reg"><span>Region</span><input type="text" placeholder="USA, Europe, Japan…" bind:value={region} /></label>
        {/if}
        <CopyForm bind:fields />
      {/if}
    </div>
    <footer>
      <button class="btn" disabled={busy} onclick={onclose}>{added ? "Done" : "Cancel"}</button>
      {#if picked}
        <button class="btn" disabled={busy} onclick={() => add(true)}>Add and add another</button>
        <button class="btn primary" disabled={busy} onclick={() => add(false)}>Add to shelf</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .find { display: grid; grid-template-columns: 200px 1fr; gap: 8px; margin-bottom: 10px; }
  .src { margin: 4px 0 6px; font-size: 12.5px; color: var(--ink-faint); }
  .hits { list-style: none; margin: 0 0 10px; padding: 0; max-height: 46vh; overflow: auto; border: 1px solid var(--seam); border-radius: var(--radius-s); }
  .hits li + li { border-top: 1px solid color-mix(in srgb, var(--seam) 60%, transparent); }
  .hits button { width: 100%; text-align: left; border: 0; background: none; padding: 7px 10px; cursor: pointer; display: flex; flex-direction: column; gap: 1px; color: var(--ink); }
  .hits button:hover, .hits button:focus-visible { background: var(--panel-2); }
  .t { font-weight: 600; }
  .r { font-size: 12.5px; color: var(--ink-faint); }
  .none { color: var(--ink-soft); margin: 6px 0; }
  .typed { margin-bottom: 12px; }
  .picked { display: flex; align-items: center; gap: 10px; padding: 8px 10px; border: 1px solid var(--seam); border-radius: var(--radius-s); margin-bottom: 14px; }
  .picked div { flex: 1; display: flex; flex-direction: column; }
  .reg { display: grid; grid-template-columns: 120px 1fr; gap: 14px; align-items: center; margin-bottom: 10px; }
  .reg span { font-size: 13px; color: var(--ink-soft); font-weight: 600; }
  .body { padding-bottom: 14px !important; }
</style>
