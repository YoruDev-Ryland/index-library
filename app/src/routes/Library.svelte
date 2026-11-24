<script lang="ts">
  import AddToShelf from "../components/AddToShelf.svelte";
  import Icon from "../components/Icon.svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { ActionInfo, ApplicableAction } from "../lib/types";
  import { PLAY } from "../lib/play";
  import { gamesText } from "../lib/gameList";
  import GameTable from "../components/GameTable.svelte";
  import CoverGrid from "../components/CoverGrid.svelte";
  import DetailPanel from "../components/DetailPanel.svelte";
  import RenameModal from "../components/RenameModal.svelte";
  import ActionConfirm from "../components/ActionConfirm.svelte";

  let search = $state(app.filter.search ?? "");
  let timer: ReturnType<typeof setTimeout>;
  $effect(() => {
    const s = search;
    clearTimeout(timer);
    timer = setTimeout(() => { if ((app.filter.search ?? "") !== s) app.filter = { ...app.filter, search: s || null }; }, 180);
  });

  type StatusPick = "" | "needs-decrypt" | "decrypted" | "compressed";
  let statusPick = $state<StatusPick>("");
  function setStatus(v: StatusPick) {
    statusPick = v;
    const m: Record<StatusPick, [string | null, string | null]> = { "": [null, null], "needs-decrypt": ["decrypted", "false"], decrypted: ["decrypted", "true"], compressed: ["compressed", "true"] };
    app.filter = { ...app.filter, status_key: m[v][0], status_value: m[v][1] };
  }
  const set = (k: "region" | "dat_status" | "meta_state" | "play" | "owned", v: string) => (app.filter = { ...app.filter, [k]: v || null });

  // filter labels, for the Filters panel and the chips of active filters
  const statusLabels: Record<StatusPick, string> = { "": "Any", "needs-decrypt": "Needs decrypting", decrypted: "Decrypted", compressed: "Compressed" };
  const datLabels: Record<string, string> = { verified: "Verified", bad: "Bad dumps", unknown: "Not in DAT", unhashed: "Not hashed" };
  const infoLabels: Record<string, string> = { matched: "Has info", review: "Needs review", none: "No info" };
  const playLabels: Record<string, string> = { none: "Not marked", favorite: "Favourites", scored: "Scored", ...Object.fromEntries(PLAY.map((p) => [p.id, p.label])) };
  const chips = $derived.by(() => {
    const out: { label: string; clear: () => void }[] = [];
    if (statusPick) out.push({ label: `Files: ${statusLabels[statusPick]}`, clear: () => setStatus("") });
    if (app.filter.region) out.push({ label: `Region: ${app.filter.region}`, clear: () => set("region", "") });
    if (app.filter.dat_status) out.push({ label: `Verification: ${datLabels[app.filter.dat_status] ?? app.filter.dat_status}`, clear: () => set("dat_status", "") });
    if (app.filter.meta_state) out.push({ label: `Info: ${infoLabels[app.filter.meta_state] ?? app.filter.meta_state}`, clear: () => set("meta_state", "") });
    if (app.filter.play) out.push({ label: `Played: ${playLabels[app.filter.play] ?? app.filter.play}`, clear: () => set("play", "") });
    if (app.filter.owned) out.push({ label: `Have: ${ownedLabels[app.filter.owned]}`, clear: () => set("owned", "") });
    return out;
  });
  let filterOpen = $state(false);
  const gameCount = $derived(app.filter.system ? app.systems.find((x) => x.id === app.filter.system)?.games ?? 0 : app.systems.reduce((a, x) => a + x.games, 0));
  // popovers close on a click anywhere outside them
  function outside(e: MouseEvent) {
    const t = e.target as HTMLElement | null;
    if (!t?.closest("[data-pop]")) { filterOpen = false; scanMenu = false; collMenu = false; }
  }
  let scanMenu = $state(false);
  let collMenu = $state(false);
  let collName = $state("");
  async function addTo(id: string, name: string) {
    collMenu = false;
    try { const n = await api.addToCollection(id, selected); app.toast(n ? `Added ${n} game${n === 1 ? "" : "s"} to ${name}` : `Already in ${name}`); app.loadCollections(); } catch (e) { app.fail(e); }
  }
  async function newColl() {
    const n = collName.trim();
    if (!n) return;
    collMenu = false; collName = "";
    try { const id = await api.createCollection(n, selected); app.toast(`Created ${n} with ${selected.length} game${selected.length === 1 ? "" : "s"}`); await app.loadCollections(); app.openCollection(id); } catch (e) { app.fail(e); }
  }
  let renameIds = $state<number[] | null>(null);
  let running = $state<{ action: ActionInfo; ids: number[] } | null>(null);
  let selActions = $state<ApplicableAction[]>([]);
  const selected = $derived([...app.selection]);
  $effect(() => {
    const ids = selected;
    if (ids.length === 0) { selActions = []; return; }
    api.actionsForGames(ids.slice(0, 5000)).then((a) => (selActions = a)).catch(() => (selActions = []));
  });
  const systemTitle = $derived(app.filter.system ? app.systemName(app.filter.system) : "All games");
  const filtered = $derived(!!(app.filter.region || app.filter.dat_status || app.filter.meta_state || app.filter.status_key || app.filter.search || app.filter.play || app.filter.owned));

  async function scan(full: boolean) { try { await api.scan(full); } catch (e) { app.fail(e); } }
  async function hashSel() { try { await api.hash({ kind: "Games", value: selected }); } catch (e) { app.fail(e); } }
  async function hashSystem() { try { await api.hash(app.filter.system ? { kind: "System", value: app.filter.system } : { kind: "All" }); } catch (e) { app.fail(e); } }
  async function lookupSel() { try { await api.matchMetadata(selected, true); } catch (e) { app.fail(e); } }
  /** Copies the games in this view (all pages), or just `ids` when given, as a plain list */
  async function copyList(ids?: number[]) {
    try {
      let rows = (await api.games(app.filter, "title", 0, 1_000_000_000)).rows;
      if (ids) {
        const want = new Set(ids);
        rows = rows.filter((r) => want.has(r.id));
        if (rows.length < want.size) rows = (await api.games({}, "title", 0, 1_000_000_000)).rows.filter((r) => want.has(r.id));
      }
      if (!rows.length) { app.toast("No games to copy"); return; }
      await navigator.clipboard.writeText(gamesText(rows, (id) => app.systemName(id)));
      app.toast(`Copied ${rows.length.toLocaleString()} game${rows.length === 1 ? "" : "s"}`);
    } catch (e) { app.fail(e); }
  }
  async function renameAll() { renameIds = await api.gameIdsFor(app.filter); }
  async function infoAll() { try { const ids = await api.gameIdsFor(app.filter); await api.matchMetadata(ids); app.toast(`Looking up ${ids.length.toLocaleString()} games — matches that need a decision go to Match review`); } catch (e) { app.fail(e); } }
  function onKey(e: KeyboardEvent) {
    if (renameIds || running || document.querySelector(".modal-backdrop")) return;
    const t = e.target as HTMLElement | null;
    const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
    // Ctrl/Cmd+A selects every game in the current view (all pages, not just what's on screen)
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a" && !typing) {
      e.preventDefault();
      api.gameIdsFor(app.filter).then((ids) => (app.selection = new Set(ids))).catch((err) => app.fail(err));
      return;
    }
    if (e.key === "Escape" && !typing && app.focusId !== null) { app.focusId = null; e.preventDefault(); }
  }
  // your physical collection
  const ownedLabels: Record<string, string> = { digital: "Files", physical: "On the shelf", both: "Files and shelf" };
  let addingShelf = $state(false);
</script>

<svelte:window onkeydown={onKey} onclick={outside} />
{#if addingShelf}<AddToShelf onclose={() => (addingShelf = false)} />{/if}
<div class="lib" class:with-detail={app.focusId !== null}>
  <div class="center">
    <div class="titlebar">
      <h1>{systemTitle}</h1>
      {#if !filtered && gameCount}<span class="count">{gameCount.toLocaleString()} games</span>{/if}
      <input class="search" type="search" placeholder="Search titles and file names" bind:value={search} aria-label="Search" />
      <div class="pop" data-pop>
        <button class="btn" class:active={chips.length > 0} aria-expanded={filterOpen} onclick={() => (filterOpen = !filterOpen)}>
          Filters{#if chips.length}<span class="badge">{chips.length}</span>{/if}<Icon name="chevron" size={13} />
        </button>
        {#if filterOpen}
          <div class="panel" role="dialog" aria-label="Filters">
            <label><span>Have</span>
              <select value={app.filter.owned ?? ""} onchange={(e) => set("owned", (e.currentTarget as HTMLSelectElement).value)}>
                <option value="">Files or shelf</option>{#each Object.entries(ownedLabels) as [k, v]}<option value={k}>{v}</option>{/each}
              </select></label>
            <label><span>Files</span>
              <select value={statusPick} onchange={(e) => setStatus((e.currentTarget as HTMLSelectElement).value as StatusPick)}>
                <option value="">Any</option><option value="needs-decrypt">Needs decrypting</option><option value="decrypted">Decrypted</option><option value="compressed">Compressed</option>
              </select></label>
            <label><span>Region</span>
              <select value={app.filter.region ?? ""} onchange={(e) => set("region", (e.currentTarget as HTMLSelectElement).value)}>
                <option value="">Any</option>{#each ["USA", "Europe", "Japan", "World", "Korea", "Asia"] as r}<option>{r}</option>{/each}
              </select></label>
            <label><span>Verification</span>
              <select value={app.filter.dat_status ?? ""} onchange={(e) => set("dat_status", (e.currentTarget as HTMLSelectElement).value)}>
                <option value="">Any</option>{#each Object.entries(datLabels) as [k, v]}<option value={k}>{v}</option>{/each}
              </select></label>
            <label><span>Game info</span>
              <select value={app.filter.meta_state ?? ""} onchange={(e) => set("meta_state", (e.currentTarget as HTMLSelectElement).value)}>
                <option value="">Any</option>{#each Object.entries(infoLabels) as [k, v]}<option value={k}>{v}</option>{/each}
              </select></label>
            <label><span>Played</span>
              <select value={app.filter.play ?? ""} onchange={(e) => set("play", (e.currentTarget as HTMLSelectElement).value)}>
                <option value="">Any</option><option value="none">Not marked</option>
                {#each PLAY as p}<option value={p.id}>{p.label}</option>{/each}
                <option value="favorite">Favourites</option><option value="scored">Scored</option>
              </select></label>
            {#if chips.length}<button class="btn small ghost reset" onclick={() => { statusPick = ""; app.filter = { system: app.filter.system, search: app.filter.search }; }}>Reset filters</button>{/if}
          </div>
        {/if}
      </div>
      <span class="spacer"></span>
      <button class="btn" onclick={() => (addingShelf = true)} title="Add a game you own physically">Add to shelf</button>
      <div class="seg" role="group" aria-label="View">
        <button class:on={!app.grid} onclick={() => app.setGrid(false)}>List</button>
        <button class:on={app.grid} onclick={() => app.setGrid(true)}>Covers</button>
      </div>
      <div class="split pop" data-pop>
        <button class="btn" onclick={() => scan(false)} title="Re-read folders whose contents changed">Rescan</button>
        <button class="btn more" aria-label="More library actions" aria-expanded={scanMenu} onclick={() => (scanMenu = !scanMenu)}><Icon name="chevron" size={14} /></button>
        {#if scanMenu}
          <div class="menu" role="menu">
            <button role="menuitem" onclick={() => { scanMenu = false; scan(true); }}><b>Full rescan</b><span class="muted">Re-read every folder, including files changed in place. Slower.</span></button>
            <div class="menu-sep">For every game in this view</div>
            <button role="menuitem" onclick={() => { scanMenu = false; infoAll(); }}><b>Get game info</b><span class="muted">Names, details and covers; unsure matches go to Match review.</span></button>
            <button role="menuitem" onclick={() => { scanMenu = false; renameAll(); }}><b>Clean up names…</b><span class="muted">Preview the new names before anything changes.</span></button>
            <button role="menuitem" onclick={() => { scanMenu = false; hashSystem(); }}><b>Fingerprint</b><span class="muted">Checksums for every file, in the background.</span></button>
            <button role="menuitem" onclick={() => { scanMenu = false; copyList(); }}><b>Copy list</b><span class="muted">Every title, one per line, to paste anywhere.</span></button>
          </div>
        {/if}
      </div>
    </div>
    {#if chips.length}
      <div class="chips-row">
        {#each chips as c (c.label)}<button class="fchip" onclick={c.clear} title="Remove this filter">{c.label}<Icon name="close" size={12} /></button>{/each}
        {#if chips.length > 1}<button class="btn small ghost" onclick={() => { statusPick = ""; app.filter = { system: app.filter.system, search: app.filter.search }; }}>Clear all</button>{/if}
      </div>
    {/if}

    {#if selected.length > 0}
      <div class="selbar">
        <b>{selected.length.toLocaleString()} selected</b>
        {#each selActions as a}<button class="btn primary small" onclick={() => (running = { action: a.action, ids: a.file_ids })}>{a.action.name} ({a.file_ids.length})</button>{/each}
        <button class="btn small" onclick={() => (renameIds = selected)}>Clean up names…</button>
        <button class="btn small" onclick={hashSel}>Hash</button>
        <button class="btn small" onclick={lookupSel}>Get game info</button>
        <button class="btn small" onclick={() => copyList(selected)} title="Copy these titles, one per line">Copy list</button>
        <div class="split pop" data-pop>
          <button class="btn small" aria-expanded={collMenu} onclick={() => (collMenu = !collMenu)}>Add to collection <Icon name="chevron" size={13} /></button>
          {#if collMenu}
            <div class="menu left" role="menu">
              {#each app.collections as c (c.id)}<button role="menuitem" onclick={() => addTo(c.id, c.name)}><b>{c.name}</b><span class="muted">{c.count} game{c.count === 1 ? "" : "s"}</span></button>{/each}
              <form class="newcoll" onsubmit={(e) => { e.preventDefault(); newColl(); }}><input placeholder="New collection…" bind:value={collName} /><button class="btn small primary" disabled={!collName.trim()}>Create</button></form>
            </div>
          {/if}
        </div>
        <span class="spacer"></span>
        <button class="btn ghost small" onclick={() => (app.selection = new Set())}>Clear selection</button>
      </div>
    {/if}

    <div class="content">
      {#if app.grid}<CoverGrid />{:else}<GameTable />{/if}
    </div>
  </div>
  {#if app.focusId !== null}
    {#key app.focusId}<DetailPanel id={app.focusId} />{/key}
  {/if}
</div>

{#if renameIds}<RenameModal gameIds={renameIds} onclose={() => (renameIds = null)} />{/if}
{#if running}<ActionConfirm action={running.action} fileIds={running.ids} onclose={() => (running = null)} />{/if}

<style>
  .lib { position: relative; display: grid; grid-template-columns: 1fr; height: 100%; }
  .lib.with-detail { grid-template-columns: 1fr 460px; }
  /* a window too narrow for list and drawer side by side: the drawer slides over the list */
  @media (max-width: 1360px) {
    .lib.with-detail { grid-template-columns: 1fr; }
    .lib > :global(.detail) { position: absolute; top: 0; right: 0; bottom: 0; width: min(460px, 100%); z-index: 20; box-shadow: -12px 0 28px color-mix(in srgb, #000 22%, transparent); }
  }
  .center { display: flex; flex-direction: column; min-width: 0; min-height: 0; container: center / inline-size; }
  @container center (max-width: 820px) {
    .count { display: none; }
    .search { margin-left: 4px; }
  }
  @container center (max-width: 620px) {
    .seg { display: none; }
    .titlebar h1 { max-width: 34%; }
  }
  /* one row: what you're looking at, how to narrow it, how to see it, what to do with it */
  .titlebar { display: flex; align-items: center; gap: 8px; padding: 12px 16px 10px; border-bottom: 1px solid var(--seam); min-width: 0; }
  .titlebar h1 { font-size: 19px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 72px; flex: 0 1 auto; }
  .count { color: var(--ink-faint); font-size: 13px; white-space: nowrap; margin-right: 6px; }
  .search { flex: 1 1 140px; min-width: 120px; max-width: 320px; margin-left: 8px; }
  .pop { position: relative; }
  .btn.active { border-color: var(--key-edge); background: var(--panel-2); }
  .badge { display: inline-grid; place-items: center; min-width: 18px; height: 18px; padding: 0 5px; border-radius: 9px; background: var(--accent); color: var(--accent-ink); font-size: 11.5px; font-weight: 700; }
  .panel { position: absolute; left: 0; top: 36px; z-index: 30; width: 300px; display: grid; gap: 8px; padding: 12px; background: var(--panel); border: 1px solid var(--seam); border-radius: var(--radius-m); box-shadow: var(--shadow-pop); }
  .panel label { display: grid; grid-template-columns: 92px 1fr; align-items: center; gap: 8px; font-size: 13px; color: var(--ink-soft); }
  .panel select { width: 100%; }
  .panel .reset { justify-self: end; }
  .chips-row { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; padding: 7px 16px; border-bottom: 1px solid var(--seam); }
  .fchip { display: inline-flex; align-items: center; gap: 6px; height: 24px; padding: 0 6px 0 9px; border: 1px solid var(--seam); border-radius: var(--radius-s); background: var(--panel); font-size: 12.5px; font-weight: 500; color: var(--ink); cursor: pointer; }
  .fchip :global(.icon) { color: var(--ink-faint); }
  .fchip:hover { border-color: var(--key-edge); }
  .fchip:hover :global(.icon) { color: var(--ink); }
  .menu-sep { padding: 8px 10px 2px; font-size: 12px; font-weight: 600; color: var(--ink-faint); border-top: 1px solid var(--seam); margin-top: 4px; }
  .split { position: relative; display: inline-flex; }
  .split .btn:first-child { border-radius: var(--radius-s) 0 0 var(--radius-s); }
  .split .more { border-left: 0; border-radius: 0 var(--radius-s) var(--radius-s) 0; padding: 0 8px; }
  .menu { position: absolute; right: 0; top: 34px; z-index: 30; width: 280px; background: var(--panel); border: 1px solid var(--seam); border-radius: var(--radius-m); box-shadow: var(--shadow-pop); padding: 4px; }
  .menu button { display: flex; flex-direction: column; gap: 2px; width: 100%; border: 0; background: none; text-align: left; padding: 8px 10px; border-radius: var(--radius-s); cursor: pointer; }
  .menu button:hover { background: var(--panel-2); }
  .menu span { font-size: 13px; }
  .menu.left { left: 0; right: auto; max-height: 360px; overflow-y: auto; }
  .newcoll { display: flex; gap: 6px; padding: 6px; border-top: 1px solid var(--seam); margin-top: 4px; }
  .newcoll input { flex: 1; min-width: 0; }
  .spacer { flex: 1; }
  .seg { display: inline-flex; border: 1px solid var(--seam); border-radius: var(--radius-s); overflow: hidden; }
  .seg button { border: 0; background: var(--panel); padding: 0 10px; height: 28px; cursor: pointer; }
  .seg button.on { background: var(--ink); color: var(--panel); }
  .selbar { display: flex; align-items: center; gap: 8px; padding: 6px 16px; min-height: 40px; background: var(--panel-2); border-top: 1px solid var(--seam); border-bottom: 1px solid var(--seam); flex-wrap: wrap; }
  .content { flex: 1; min-height: 0; }
</style>
