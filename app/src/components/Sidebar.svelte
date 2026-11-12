<script lang="ts">
  import Icon from "./Icon.svelte";
  import { app, type View } from "../lib/state.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { bytes } from "../lib/format";
  import { api } from "../lib/api";
  import ImportCollection from "./ImportCollection.svelte";

  const tools: { view: View; label: string }[] = [
    { view: "dashboard", label: "Overview" },
    { view: "completion", label: "Completion" },
    { view: "duplicates", label: "Duplicates" },
    { view: "cleanup", label: "Cleanup" },
    { view: "review", label: "Match review" },
    { view: "archive", label: "Archive" },
    { view: "history", label: "Change history" },
    { view: "settings", label: "Settings" },
  ];
  const total = $derived(app.systems.reduce((a, s) => a + s.games, 0));
  const favs = $derived((app.settings?.favorite_systems ?? []).map((id) => app.systems.find((s) => s.id === id)).filter((s) => !!s));
  const others = $derived(app.systems.filter((s) => !app.isFavorite(s.id)));

  // collections, grouped under the system all their games are for
  const collGroups = $derived.by(() => {
    const m = new Map<string, typeof app.collections>();
    for (const c of app.collections) {
      const k = c.system ? app.systemName(c.system) : c.count ? "Several systems" : "Empty";
      m.set(k, [...(m.get(k) ?? []), c]);
    }
    return [...m.entries()].sort(([a], [b]) => Number(a === "Empty") - Number(b === "Empty") || a.localeCompare(b));
  });
  let folded = $state<Set<string>>(new Set());
  const fold = (k: string) => { const s = new Set(folded); s.has(k) ? s.delete(k) : s.add(k); folded = s; };
  let naming = $state(false);
  let addMenu = $state(false);
  let importing = $state(false);
  let newName = $state("");
  async function createColl() {
    const n = newName.trim();
    if (!n) { naming = false; return; }
    try { const id = await api.createCollection(n); naming = false; newName = ""; await app.loadCollections(); app.openCollection(id); } catch (e) { app.fail(e); }
  }

  function pick(system: string | null) {
    app.view = "library";
    app.filter = { ...app.filter, system };
    app.selection = new Set();
    app.focusId = null;
  }
</script>

<nav class="side">
  <button class="all" class:on={app.view === "library" && !app.filter.system} onclick={() => pick(null)}>
    <span>All games</span><span class="n">{total.toLocaleString()}</span>
  </button>
  <div class="lists">
  <div class="shelf">
    {#snippet spine(s: (typeof app.systems)[number])}
      {@const fav = app.isFavorite(s.id)}
      {@const img = app.systemImages[s.id]}
      <div class="spine" class:on={app.view === "library" && app.filter.system === s.id}>
        {#if img}<img class="bg" src={convertFileSrc(img)} alt="" crossorigin="anonymous" />{/if}
        <button class="pick" onclick={() => pick(s.id)} title="{s.name} · {bytes(s.bytes)}">
          <span class="name">{s.name}</span>
          <span class="n">{s.games.toLocaleString()}</span>
        </button>
        <button class="star" class:fav onclick={() => app.toggleFavorite(s.id)} aria-pressed={fav} aria-label={fav ? `Remove ${s.name} from favorites` : `Add ${s.name} to favorites`} title={fav ? "Remove from favorites" : "Add to favorites"}><Icon name={fav ? "star-filled" : "star"} size={14} /></button>
      </div>
    {/snippet}
    {#if favs.length}
      <div class="group">Favorites</div>
      {#each favs as s (s!.id)}{@render spine(s!)}{/each}
      <div class="group">All systems</div>
    {/if}
    {#each others as s (s.id)}{@render spine(s)}{/each}
    {#if app.systems.length === 0}<p class="faint hint">Systems appear here after the first scan.</p>{/if}
  </div>
  <div class="colls">
    <div class="colls-head">
      <span>Collections</span>
      <div class="addwrap" data-pop>
        <button class="add" title="New or import" aria-label="New or import a collection" aria-expanded={addMenu} onclick={() => (addMenu = !addMenu)}><Icon name="plus" /></button>
        {#if addMenu}
          <div class="addmenu" role="menu">
            <button role="menuitem" onclick={() => { addMenu = false; naming = true; newName = ""; }}>New collection</button>
            <button role="menuitem" onclick={() => { addMenu = false; importing = true; }}>Import collection…</button>
          </div>
        {/if}
      </div>
    </div>
    {#if naming}
      <form class="newc" onsubmit={(e) => { e.preventDefault(); createColl(); }}>
        <!-- svelte-ignore a11y_autofocus -->
        <input autofocus placeholder="Collection name" bind:value={newName} onkeydown={(e) => { if (e.key === "Escape") naming = false; }} onblur={createColl} />
      </form>
    {/if}
    <div class="tree">
      {#each collGroups as [group, list] (group)}
        <button class="branch" onclick={() => fold(group)} aria-expanded={!folded.has(group)}><span class="caret" class:shut={folded.has(group)}><Icon name="chevron" size={12} /></span>{group}</button>
        {#if !folded.has(group)}
          {#each list as c (c.id)}
            <button class="leaf" class:on={app.view === "collection" && app.collectionId === c.id} onclick={() => app.openCollection(c.id)} title={c.next ? `Up next: ${c.next}` : c.name}>
              <span class="cname">{c.name}</span>
              <span class="prog">{#if c.count}{c.done}/{c.count - c.skipped}{/if}</span>
              {#if c.count}<span class="bar"><span style:width="{Math.round((c.done / Math.max(1, c.count - c.skipped)) * 100)}%"></span></span>{/if}
            </button>
          {/each}
        {/if}
      {/each}
      {#if !app.collections.length && !naming}<p class="faint hint">Group games into collections with a play order — select games in the library and use “Add to collection”, or press the + button.</p>{/if}
    </div>
  </div>
  </div>
  <div class="tools">
    {#each tools as t}
      <button class:on={app.view === t.view} onclick={() => (app.view = t.view)}>{t.label}</button>
    {/each}
  </div>
</nav>
<svelte:window onclick={(e) => { if (!(e.target as HTMLElement | null)?.closest("[data-pop]")) addMenu = false; }} />
{#if importing}<ImportCollection onclose={() => (importing = false)} />{/if}

<style>
  .side { display: flex; flex-direction: column; min-height: 0; background: var(--panel-2); border-right: 1px solid var(--seam); }
  button { display: flex; align-items: center; gap: 8px; width: 100%; border: 0; background: none; text-align: left; cursor: pointer; }
  .all { margin-top: 8px; padding: 7px 16px; font-weight: 600; }
  .all.on, .tools button.on { color: var(--accent-text); }
  .n { margin-left: auto; color: var(--ink-faint); font-size: 13px; }
  /* systems above, collections below, each scrolling on its own */
  /* collections are as tall as their list until they reach half the sidebar, then scroll;
     the systems take the rest and scroll on their own */
  .lists { flex: 1; min-height: 0; display: grid; grid-template-rows: minmax(0, 1fr) fit-content(50%); }
  .shelf { min-height: 0; overflow-y: auto; padding: 4px 10px 10px; display: flex; flex-direction: column; gap: 3px; }
  .colls { min-height: 0; display: flex; flex-direction: column; border-top: 1px solid var(--seam); }
  .colls-head { display: flex; align-items: center; justify-content: space-between; padding: 8px 10px 4px 14px; font-size: 13px; font-weight: 600; color: var(--ink-soft); }
  .add { width: 24px; height: 24px; justify-content: center; border-radius: 50%; color: var(--ink-soft); font-size: 15px; }
  .addwrap { position: relative; }
  .addmenu { position: absolute; right: 0; bottom: 30px; z-index: 40; width: 190px; padding: 4px; background: var(--panel); border: 1px solid var(--seam); border-radius: var(--radius-m); box-shadow: var(--shadow-pop); }
  .addmenu button { padding: 7px 10px; border-radius: var(--radius-s); font-weight: 500; }
  .addmenu button:hover { background: var(--panel-2); }
  .add:hover { background: var(--panel); color: var(--accent-text); }
  .newc { padding: 2px 10px 6px; }
  .newc input { width: 100%; }
  .tree { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 0 8px 10px; }
  .branch { padding: 6px 6px 3px; font-size: 12.5px; font-weight: 600; color: var(--ink-faint); gap: 5px; }
  .caret { display: inline-block; transition: transform .12s; font-size: 10px; }
  .caret.shut { transform: rotate(-90deg); }
  .leaf { position: relative; display: grid; grid-template-columns: 1fr auto; padding: 5px 8px 7px 22px; border-radius: var(--radius-m); gap: 2px 8px; }
  .leaf:hover { background: var(--panel); }
  .leaf.on { background: var(--panel); box-shadow: inset 0 0 0 1px var(--seam); }
  .leaf.on .cname { color: var(--ink); }
  .cname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .prog { font-size: 12px; color: var(--ink-faint); }
  .bar { grid-column: 1 / -1; height: 3px; border-radius: 2px; background: color-mix(in srgb, var(--seam) 70%, transparent); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--ok); }
  /* each system is a cartridge spine: a ridged label with the maker's colour tab */
  /* each system is a tile with its console photo, darkened so the label reads */
  .spine { position: relative; flex: none; display: flex; align-items: stretch; height: 32px; border-radius: var(--radius-s); background: #2b2b31; overflow: hidden; isolation: isolate; }
  .bg { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: center 45%; filter: brightness(.42) saturate(.85); z-index: -2; transition: filter .15s; }
  .spine::after { content: ""; position: absolute; inset: 0; background: linear-gradient(90deg, #000000b0 0%, #00000055 55%, #00000020 100%); z-index: -1; }
  .spine:hover .bg { filter: brightness(.55) saturate(1); }
  .spine.on { box-shadow: 0 0 0 2px var(--accent); }
  .spine.on .bg { filter: brightness(.6) saturate(1); }
  .pick { flex: 1; min-width: 0; padding: 0 4px 0 12px; color: #fff; }
  .pick .n { color: #ffffffb0; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; text-shadow: 0 1px 2px #0008; }
  .star { width: 28px; flex: none; justify-content: center; color: #ffffffa0; font-size: 14px; opacity: 0; }
  .star.fav { color: var(--signal); opacity: 1; }
  .spine:hover .star, .star:focus-visible { opacity: 1; }
  .star:hover { color: var(--signal); }
  .group { padding: 10px 4px 3px; font-size: 12.5px; font-weight: 600; color: var(--ink-soft); }
  .group:first-child { padding-top: 2px; }
  .hint { padding: 6px; font-size: 13px; }
  .tools { border-top: 1px solid var(--seam); padding: 8px 0 10px; }
  .tools button { padding: 5px 16px; color: var(--ink-soft); font-weight: 500; }
  .tools button:hover { color: var(--ink); }
</style>
