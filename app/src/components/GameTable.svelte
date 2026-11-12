<script lang="ts">
  import Icon from "./Icon.svelte";
  import { playOf } from "../lib/play";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { bytes, makerOf } from "../lib/format";
  import type { GameRow, GameSort } from "../lib/types";
  import StatusChips from "./StatusChips.svelte";

  const ROW = 30, PAGE = 400, OVERSCAN = 12;
  let rows = $state<(GameRow | undefined)[]>([]);
  let total = $state(0);
  let scrollTop = $state(0);
  let height = $state(600);
  let pending = new Set<number>();
  let gen = 0;
  let anchor: number | null = null;
  let viewport: HTMLDivElement;

  async function loadPage(p: number, g: number) {
    if (pending.has(p)) return;
    pending.add(p);
    try {
      const page = await api.games(app.filter, app.sort, p * PAGE, PAGE);
      if (g !== gen) return;
      total = page.total;
      if (rows.length !== total) { rows.length = total; for (let k = page.total; k < rows.length; k++) rows[k] = undefined; }
      page.rows.forEach((r, i) => (rows[p * PAGE + i] = r));
    } catch (e) { app.fail(e); } finally { pending.delete(p); }
  }

  // New query (filter/sort): start over at the top. Library changed (revision): reload the
  // pages already on screen in place, keeping rows and scroll position.
  let lastQuery = "";
  $effect(() => {
    const q = JSON.stringify(app.filter) + app.sort;
    void app.revision;
    gen++;
    pending = new Set();
    if (q !== lastQuery) {
      lastQuery = q;
      rows = [];
      total = 0;
      if (viewport) viewport.scrollTop = 0;
      scrollTop = 0;
      loadPage(0, gen);
    } else {
      const pages = new Set<number>();
      for (let p = Math.floor(first / PAGE); p <= Math.floor(Math.max(first, last - 1) / PAGE); p++) pages.add(p);
      if (pages.size === 0) pages.add(0);
      for (const p of pages) loadPage(p, gen);
    }
  });

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(total, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  $effect(() => {
    for (let p = Math.floor(first / PAGE); p <= Math.floor(Math.max(first, last - 1) / PAGE); p++) {
      if (p * PAGE < total && rows[p * PAGE] === undefined) loadPage(p, gen);
    }
  });

  function click(e: MouseEvent, i: number, r: GameRow) {
    const sel = new Set(e.ctrlKey || e.metaKey || e.shiftKey ? app.selection : []);
    if (e.shiftKey && anchor !== null) {
      const [a, b] = [Math.min(anchor, i), Math.max(anchor, i)];
      for (let k = a; k <= b; k++) { const x = rows[k]; if (x) sel.add(x.id); }
    } else if (e.ctrlKey || e.metaKey) {
      sel.has(r.id) ? sel.delete(r.id) : sel.add(r.id);
      anchor = i;
    } else { sel.add(r.id); anchor = i; }
    app.selection = sel;
    app.focusId = r.id;
  }
  function toggle(r: GameRow, i: number) {
    const sel = new Set(app.selection);
    sel.has(r.id) ? sel.delete(r.id) : sel.add(r.id);
    app.selection = sel; anchor = i;
  }
  async function selectAll() { app.selection = new Set(await api.gameIdsFor(app.filter)); }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") { app.selection = new Set(); return; }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const cur = rows.findIndex((r) => r?.id === app.focusId);
    const n = Math.max(0, Math.min(total - 1, cur + (e.key === "ArrowDown" ? 1 : -1)));
    const r = rows[n];
    if (r) { app.selectOnly(r.id); anchor = n; }
    const top = n * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + height) viewport.scrollTop = top + ROW - height;
  }
  function sortBy(s: GameSort) { app.sort = s; }
  const allSel = $derived(total > 0 && app.selection.size >= total);
  const showSystem = $derived(!app.filter.system);
</script>

<div class="table" role="grid" aria-rowcount={total}>
  <div class="head" class:sys={showSystem}>
    <span class="cb"><input type="checkbox" checked={allSel} aria-label="Select all" onchange={() => (allSel ? (app.selection = new Set()) : selectAll())} /></span>
    <button class:on={app.sort === "title"} onclick={() => sortBy("title")}>Title</button>
    <span>Played</span>
    {#if showSystem}<button class:on={app.sort === "system"} onclick={() => sortBy("system")}>System</button>{/if}
    <button class="c-region" class:on={app.sort === "region"} onclick={() => sortBy("region")}>Region</button>
    <span class="c-format">Format</span>
    <button class="num c-size" class:on={app.sort === "size"} onclick={() => sortBy("size")}>Size</button>
    <span>Status</span>
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_static_element_interactions -->
  <div class="viewport" bind:this={viewport} bind:clientHeight={height} onscroll={(e) => (scrollTop = (e.currentTarget as HTMLDivElement).scrollTop)} tabindex="0" onkeydown={key}>
    <div style="height: {total * ROW}px; position: relative">
      {#each { length: last - first } as _, k (first + k)}
        {@const i = first + k}
        {@const r = rows[i]}
        <div class="row" class:sys={showSystem} class:sel={r && app.selection.has(r.id)} class:focus={r && app.focusId === r.id} style="top: {i * ROW}px" role="row" aria-rowindex={i + 1}>
          {#if r}
            <span class="cb"><input type="checkbox" checked={app.selection.has(r.id)} onchange={() => toggle(r, i)} aria-label="Select {r.title}" /></span>
            <button class="cell title" onclick={(e) => click(e, i, r)} ondblclick={() => app.selectOnly(r.id)}>
              <span class="t">{r.title}</span>
              {#each r.tags.slice(0, 3) as t}<span class="tag">{t}</span>{/each}
              {#if r.files > 1}<span class="faint">· {r.files} files</span>{/if}
            </button>
            <span class="cell play">
              {#if r.favorite}<span class="fav" title="Favourite"><Icon name="star-filled" size={13} label="Favourite" /></span>{/if}
              {#if r.play}<span class="gpill {playOf(r.play)?.tone}">{playOf(r.play)?.short}</span>{/if}
              {#if r.score}<span class="score">{r.score}<small>/10</small></span>{/if}
            </span>
            {#if showSystem}<span class="cell sysname"><span class="dot" style="background: {makerOf(r.system_id).color}"></span>{app.systemName(r.system_id)}</span>{/if}
            <span class="cell muted c-region">{r.regions.join(", ")}</span>
            <span class="cell muted c-format">{r.files ? r.exts.join(" ") : ""}</span>
            <span class="cell num muted c-size">{r.files ? bytes(r.size) : ""}</span>
            <span class="cell chips c-status">
              {#if r.shelf.copies}<span class="chip shelf" title="{r.shelf.copies === 1 ? 'A copy' : `${r.shelf.copies} copies`} on your shelf{r.files ? '' : ' (no files)'}">{r.files ? "Shelf" : "Shelf only"}{r.shelf.best ? ` · ${r.shelf.best}` : ""}{r.shelf.copies > 1 ? ` ×${r.shelf.copies}` : ""}</span>{/if}
              <StatusChips status={r.status} dat={r.dat} />
            </span>
          {:else}
            <span class="cb"></span><span class="cell faint">…</span>
          {/if}
        </div>
      {/each}
    </div>
    {#if total === 0}
      <div class="empty">
        <h2>No games match</h2>
        <p>{app.filter.search ? "Try a shorter search, or clear the filters." : "Run a scan to index this library."}</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .table { display: flex; flex-direction: column; min-height: 0; height: 100%; container: tbl / inline-size; }
  /* narrower list (a small window, the drawer open): the least needed columns go first */
  @container tbl (max-width: 1060px) {
    .c-status { display: none !important; }
    .head, .row { grid-template-columns: 34px minmax(200px, 1fr) 132px 130px 80px 80px; }
    .head.sys, .row.sys { grid-template-columns: 34px minmax(200px, 1fr) 132px 150px 120px 80px 80px; }
  }
  @container tbl (max-width: 860px) {
    .c-format { display: none !important; }
    .head, .row { grid-template-columns: 34px minmax(180px, 1fr) 132px 120px 80px; }
    .head.sys, .row.sys { grid-template-columns: 34px minmax(180px, 1fr) 132px 150px 110px 80px; }
  }
  @container tbl (max-width: 720px) {
    .c-region { display: none !important; }
    .head, .row { grid-template-columns: 34px minmax(160px, 1fr) 124px 80px; }
    .head.sys, .row.sys { grid-template-columns: 34px minmax(160px, 1fr) 124px 140px 80px; }
  }
  @container tbl (max-width: 560px) {
    .c-size { display: none !important; }
    .head, .row { grid-template-columns: 34px minmax(140px, 1fr) 120px; }
    .head.sys, .row.sys { grid-template-columns: 34px minmax(140px, 1fr) 120px 130px; }
  }
  .head, .row { display: grid; grid-template-columns: 34px minmax(240px, 1fr) 132px 150px 90px 80px 190px; align-items: center; }
  .head.sys, .row.sys { grid-template-columns: 34px minmax(220px, 1fr) 132px 170px 140px 80px 80px 190px; }
  .play { display: flex; align-items: center; gap: 6px; }
  .score { font-weight: 700; font-size: 13px; } .score small { color: var(--ink-faint); font-weight: 500; }
  .fav { color: var(--signal); }
  .head { height: 32px; border-bottom: 1px solid var(--seam); color: var(--ink-soft); font-size: 13px; font-weight: 600; }
  .head button { border: 0; background: none; text-align: left; font-weight: 600; cursor: pointer; padding: 0 8px; color: inherit; }
  .head button.on { color: var(--accent-text); }
  .head span { padding: 0 8px; }
  .viewport { flex: 1; overflow-y: auto; overflow-x: hidden; outline: none; position: relative; }
  .row { position: absolute; left: 0; right: 0; height: 30px; border-bottom: 1px solid color-mix(in srgb, var(--seam) 45%, transparent); }
  .row:hover { background: color-mix(in srgb, var(--panel) 65%, transparent); }
  .row.sel { background: color-mix(in srgb, var(--accent) 11%, transparent); }
  .row.focus { box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent); }
  .cb { display: grid; place-items: center; }
  .cell { padding: 0 8px; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; min-width: 0; }
  .title { border: 0; background: none; text-align: left; cursor: default; display: flex; gap: 6px; align-items: baseline; height: 100%; align-items: center; }
  .t { overflow: hidden; text-overflow: ellipsis; font-weight: 500; }
  .tag { font-size: 12px; color: var(--ink-soft); background: var(--panel-2); border-radius: 3px; padding: 0 5px; flex: none; }
  .sysname { display: flex; align-items: center; gap: 6px; color: var(--ink-soft); }
  .chips { display: flex; gap: 4px; }
  .num { text-align: right; }
</style>
