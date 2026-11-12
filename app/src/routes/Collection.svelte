<script lang="ts">
  import Icon from "../components/Icon.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { PLAY, emptyPlay, isDone, isOut, playOf } from "../lib/play";
  import type { CollectionDetail, CollectionItem, GameRow, PlayStatus } from "../lib/types";

  let c = $state<CollectionDetail | null>(null);
  let busy = $state(false);
  let editingName = $state(false);
  let name = $state("");
  let notes = $state("");
  let notesOpen = $state(false);
  let confirmDelete = $state(false);
  let noteFor = $state<string | null>(null);
  let noteText = $state("");
  let drag = $state<number | null>(null);
  let over = $state<number | null>(null);
  let query = $state("");
  let found = $state<GameRow[]>([]);
  let searchTimer: ReturnType<typeof setTimeout>;
  let open = $state(false);
  let hi = $state(-1);
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();

  async function load() {
    const id = app.collectionId;
    if (!id) return;
    try { c = await api.collection(id); name = c.name; notes = c.notes ?? ""; } catch (e) { app.fail(e); c = null; }
  }
  $effect(() => { void app.collectionId; void app.revision; load(); });

  // play order: skipped and dropped games don't get a number; the first one not done is up next
  const order = $derived.by(() => {
    let n = 0;
    return (c?.items ?? []).map((i) => (isOut(i.play?.status) ? null : ++n));
  });
  const nextIdx = $derived((c?.items ?? []).findIndex((i) => !isDone(i.play?.status) && !isOut(i.play?.status)));
  const counts = $derived.by(() => {
    const it = c?.items ?? [];
    return { total: it.length, done: it.filter((i) => isDone(i.play?.status)).length, out: it.filter((i) => isOut(i.play?.status)).length, missing: it.filter((i) => i.game_id === null).length };
  });

  async function saveItems(items: CollectionItem[]) {
    if (!c) return;
    const id = c.id;
    c = { ...c, items };
    try { await api.setCollectionItems(id, items.map((i) => [i.ref, i.note])); await app.loadCollections(); } catch (e) { app.fail(e); load(); }
  }
  function move(from: number, to: number) {
    if (!c || from === to || to < 0 || to >= c.items.length) return;
    const items = [...c.items];
    const [it] = items.splice(from, 1);
    items.splice(to, 0, it);
    saveItems(items);
  }
  const remove = (i: number) => c && saveItems(c.items.filter((_, j) => j !== i));
  function startNote(it: CollectionItem) { noteFor = it.ref; noteText = it.note ?? ""; }
  function saveNote() {
    if (!c || noteFor === null) return;
    const items = c.items.map((i) => (i.ref === noteFor ? { ...i, note: noteText.trim() || null } : i));
    noteFor = null;
    saveItems(items);
  }

  async function setStatus(it: CollectionItem, s: PlayStatus | null) {
    if (it.game_id === null) return;
    try { await api.setPlay(it.game_id, { ...(it.play ?? emptyPlay()), status: s }); await load(); app.loadCollections(); app.refresh(); } catch (e) { app.fail(e); }
  }
  async function setScore(it: CollectionItem, v: number | null) {
    if (it.game_id === null) return;
    try { await api.setPlay(it.game_id, { ...(it.play ?? emptyPlay()), score: v }); await load(); } catch (e) { app.fail(e); }
  }

  async function rename() {
    editingName = false;
    if (!c || !name.trim() || name.trim() === c.name) { name = c?.name ?? ""; return; }
    try { await api.updateCollection(c.id, name.trim(), c.notes); await load(); app.loadCollections(); } catch (e) { app.fail(e); }
  }
  async function saveNotes() {
    if (!c || (notes.trim() || null) === (c.notes ?? null)) return;
    try { await api.updateCollection(c.id, c.name, notes.trim() || null); await load(); } catch (e) { app.fail(e); }
  }
  async function del() {
    if (!c) return;
    if (!confirmDelete) { confirmDelete = true; return; }
    try { await api.updateCollection(c.id, c.name, c.notes, true); await app.loadCollections(); app.view = "library"; app.toast(`Deleted “${c.name}” — the games themselves are untouched`); } catch (e) { app.fail(e); }
  }

  // add games by searching the library. The results stay open after adding, so a series
  // (Ryza 1, 2, 3) is one search and three clicks; the query is selected, so typing a
  // different name replaces it.
  $effect(() => {
    const q = query.trim();
    clearTimeout(searchTimer);
    if (q.length < 2) { found = []; hi = -1; return; }
    searchTimer = setTimeout(async () => {
      try { found = (await api.games({ search: q }, "title", 0, 30)).rows; } catch { found = []; }
      hi = -1;
      open = true;
    }, 200);
  });
  const have = $derived(new Set((c?.items ?? []).map((i) => i.game_id).filter((x): x is number => x !== null)));
  const addable = $derived(found.filter((g) => !have.has(g.id)));

  async function add(games: GameRow[]) {
    if (!c) return;
    const ids = games.map((g) => g.id).filter((id) => !have.has(id));
    if (!ids.length) return;
    try {
      await api.addToCollection(c.id, ids);
      await load();
      app.loadCollections();
      if (ids.length > 1) app.toast(`Added ${ids.length} games`);
    } catch (e) { app.fail(e); }
    input?.focus();
    input?.select();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") { if (open && found.length) open = false; else query = ""; return; }
    if (!found.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      open = true;
      const d = e.key === "ArrowDown" ? 1 : -1;
      hi = (hi + d + found.length) % found.length;
      list?.children[hi + 1]?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && open && hi >= 0) {
      e.preventDefault();
      add([found[hi]]);
    }
  }
  function onFocusOut(e: FocusEvent) {
    const to = e.relatedTarget as Node | null;
    if (!to || !(e.currentTarget as HTMLElement).contains(to)) open = false;
  }
  async function exportFile() {
    if (!c) return;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({ defaultPath: `${c.name.replace(/[\\/:*?"<>|]/g, "-")}.json`, filters: [{ name: "Collection", extensions: ["json"] }] });
      if (!path) return;
      await api.writeTextFile(path.endsWith(".json") ? path : `${path}.json`, await api.collectionExport(c.id));
      app.toast(`Saved ${c.name}`);
    } catch (e) { app.fail(e); }
  }
  async function copyJson() {
    if (!c) return;
    try { await navigator.clipboard.writeText(await api.collectionExport(c.id)); app.toast("Copied the collection as JSON"); } catch (e) { app.fail(e); }
  }
  function openGame(it: CollectionItem) {
    if (it.game_id === null) return;
    app.view = "library";
    app.filter = { system: it.system };
    app.focusId = it.game_id;
    app.selection = new Set([it.game_id]);
  }
</script>

<div class="page">
  {#if !c}<p class="faint">Loading…</p>
  {:else}
    <div class="head">
      <div class="grow">
        {#if editingName}
          <!-- svelte-ignore a11y_autofocus -->
          <input class="name-input" aria-label="Collection name" autofocus bind:value={name} onblur={rename} onkeydown={(e) => { if (e.key === "Enter") rename(); if (e.key === "Escape") { name = c!.name; editingName = false; } }} />
        {:else}
          <h1><button class="name" title="Rename" onclick={() => (editingName = true)}>{c.name}</button></h1>
        {/if}
        <p class="summary">
          {#if counts.total}
            <span><b>{counts.done}</b> of {counts.total - counts.out} played</span>
            {#if counts.out}<span>{counts.out} skipped or dropped</span>{/if}
            {#if nextIdx >= 0}<span>Up next: <b>{c.items[nextIdx].title}</b></span>{:else if counts.done}<span class="good">All done</span>{/if}
          {:else}<span>No games yet — search below to add some, or select games in the library and use “Add to collection”.</span>{/if}
        </p>
        {#if counts.total}<div class="progress"><span style:width="{Math.round((counts.done / Math.max(1, counts.total - counts.out)) * 100)}%"></span></div>{/if}
      </div>
      <div class="head-actions">
        <button class="btn small ghost" onclick={() => (notesOpen = !notesOpen)}>{c.notes ? "Notes" : "Add notes"}</button>
        <button class="btn small ghost" onclick={exportFile} title="Save as a .json file you can share or import elsewhere">Export…</button>
        <button class="btn small ghost" onclick={copyJson} title="Copy the collection as JSON">Copy</button>
        <button class="btn small ghost" class:danger={confirmDelete} onclick={del} onblur={() => (confirmDelete = false)}>{confirmDelete ? "Click again to delete" : "Delete collection"}</button>
      </div>
    </div>
    {#if notesOpen || c.notes}
      <textarea class="notes" rows="3" placeholder="Notes about this collection — why this order, what to look out for…" bind:value={notes} onblur={saveNotes}></textarea>
    {/if}

    <div class="adder" onfocusout={onFocusOut}>
      <input type="search" placeholder="Add games: type part of a name…" aria-label="Add games" role="combobox" aria-expanded={open && found.length > 0} aria-controls="add-results"
        bind:this={input} bind:value={query} onkeydown={onKey} onfocus={() => (open = true)} onclick={() => (open = true)} />
      {#if open && found.length}
        <ul class="found" id="add-results" role="listbox" bind:this={list}>
          <li class="found-head">
            <span class="faint">{found.length}{found.length === 30 ? "+" : ""} {found.length === 1 ? "match" : "matches"}{addable.length < found.length ? ` · ${found.length - addable.length} already in` : ""}</span>
            {#if addable.length > 1}<button class="btn small ghost" onclick={() => add(addable)}>Add all {addable.length}</button>{/if}
          </li>
          {#each found as g, i (g.id)}
            {@const inside = have.has(g.id)}
            <li role="option" aria-selected={i === hi}>
              <button class="pick" class:hi={i === hi} class:inside disabled={inside} onclick={() => add([g])} onmouseenter={() => (hi = i)}>
                <span class="ft">{g.title}</span>
                <span class="faint">{app.systemName(g.system_id)}{g.regions.length ? ` · ${g.regions.join(", ")}` : ""}</span>
                <span class="plus">{inside ? "Added" : "Add"}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <ol class="order">
      {#each c.items as it, i (it.ref)}
        {@const p = playOf(it.play?.status)}
        <li class:out={isOut(it.play?.status)} class:done={isDone(it.play?.status)} class:next={i === nextIdx} class:over={over === i && drag !== null && drag !== i}
          draggable="true"
          ondragstart={(e) => { drag = i; e.dataTransfer?.setData("text/plain", String(i)); }}
          ondragover={(e) => { e.preventDefault(); over = i; }}
          ondragleave={() => { if (over === i) over = null; }}
          ondrop={(e) => { e.preventDefault(); if (drag !== null) move(drag, i); drag = null; over = null; }}
          ondragend={() => { drag = null; over = null; }}>
          <span class="grip" title="Drag to reorder" aria-hidden="true"><Icon name="grip" /></span>
          <span class="num">{order[i] ?? "–"}</span>
          <div class="thumb">{#if it.cover}<img src={convertFileSrc(it.cover)} alt="" crossorigin="anonymous" loading="lazy" />{/if}</div>
          <div class="info">
            <div class="title">
              <button class="tlink" disabled={it.game_id === null} onclick={() => openGame(it)}>{it.title}</button>
              {#if i === nextIdx}<span class="nexttag">Up next</span>{/if}
            </div>
            <div class="sub">
              <span>{app.systemName(it.system)}</span>
              {#if it.game_id === null}<span class="warn">not in the library right now</span>{/if}
              {#if it.play?.finished && isDone(it.play.status)}<span>finished {it.play.finished}</span>{:else if it.play?.started && it.play.status === "playing"}<span>since {it.play.started}</span>{/if}
            </div>
            {#if noteFor === it.ref}
              <!-- svelte-ignore a11y_autofocus -->
              <input class="note-input" aria-label="Note for this entry" autofocus placeholder="A note for this entry (why here, why skip…)" bind:value={noteText} onblur={saveNote} onkeydown={(e) => { if (e.key === "Enter") saveNote(); if (e.key === "Escape") noteFor = null; }} />
            {:else if it.note}
              <button class="note" onclick={() => startNote(it)}>{it.note}</button>
            {/if}
            {#if it.play?.notes}<div class="review">“{it.play.notes}”</div>{/if}
          </div>
          <div class="ctrl">
            <select class="status s-{p?.tone ?? 'none'}" value={it.play?.status ?? ""} disabled={it.game_id === null} onchange={(e) => setStatus(it, ((e.currentTarget as HTMLSelectElement).value || null) as PlayStatus | null)} aria-label="Play status of {it.title}">
              <option value="">No status</option>
              {#each PLAY as s}<option value={s.id}>{s.label}</option>{/each}
            </select>
            <select class="score" value={it.play?.score ?? ""} disabled={it.game_id === null} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; setScore(it, v ? Number(v) : null); }} aria-label="Score for {it.title}">
              <option value="">–/10</option>
              {#each [10, 9, 8, 7, 6, 5, 4, 3, 2, 1] as n}<option value={n}>{n}/10</option>{/each}
            </select>
            <div class="mini">
              <button title="Move up" aria-label="Move up" disabled={i === 0} onclick={() => move(i, i - 1)}><Icon name="up" /></button>
              <button title="Move down" aria-label="Move down" disabled={i === c.items.length - 1} onclick={() => move(i, i + 1)}><Icon name="down" /></button>
              <button title="Note for this entry" aria-label="Note" onclick={() => startNote(it)}><Icon name="edit" /></button>
              <button title="Remove from collection" aria-label="Remove" onclick={() => remove(i)}><Icon name="close" /></button>
            </div>
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .head { display: flex; gap: 16px; align-items: flex-start; margin-bottom: 10px; }
  .grow { flex: 1; min-width: 0; }
  h1 { margin: 0; }
  .name { border: 0; background: none; padding: 0; font: inherit; color: inherit; cursor: text; text-align: left; }
  .name:hover { text-decoration: underline dotted; text-underline-offset: 5px; }
  .name-input { font-size: 24px; font-weight: 700; width: min(520px, 100%); }
  .summary { display: flex; flex-wrap: wrap; gap: 4px 0; margin: 6px 0 0; color: var(--ink-soft); }
  .summary span + span::before { content: "·"; margin: 0 8px; color: var(--ink-faint); }
  .summary .good { color: var(--ok); font-weight: 600; }
  .progress { margin-top: 8px; height: 5px; max-width: 420px; border-radius: 3px; background: color-mix(in srgb, var(--seam) 70%, transparent); overflow: hidden; }
  .progress span { display: block; height: 100%; background: var(--ok); }
  .head-actions { display: flex; gap: 4px; }
  .notes { width: 100%; max-width: 760px; margin-bottom: 10px; }
  .adder { position: relative; max-width: 520px; margin: 4px 0 14px; }
  .adder input { width: 100%; }
  .found { position: absolute; z-index: 5; left: 0; right: 0; top: 100%; margin: 4px 0 0; padding: 0 4px 4px; list-style: none; max-height: min(60vh, 440px); overflow-y: auto; background: var(--panel); border: 1px solid var(--seam); border-radius: var(--radius-m); box-shadow: var(--shadow-pop); }
  .found-head { position: sticky; top: 0; display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 36px; padding: 4px 4px 4px 8px; margin: 0 -4px 2px; background: var(--panel); border-bottom: 1px solid var(--seam); font-size: 13px; }
  .pick { display: grid; grid-template-columns: 1fr auto auto; gap: 10px; align-items: center; width: 100%; border: 0; background: none; padding: 6px 8px; border-radius: var(--radius-s); text-align: left; cursor: pointer; color: inherit; font: inherit; }
  .pick.hi { background: var(--panel-2); }
  .pick.inside { cursor: default; }
  .pick.inside .ft { color: var(--ink-soft); font-weight: 500; }
  .pick.inside .plus { color: var(--ok); font-weight: 600; }
  .ft { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .plus { color: var(--accent-text); font-weight: 700; font-size: 13px; }

  .order { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; max-width: 1100px; }
  .order li { display: grid; grid-template-columns: 16px 30px 46px 1fr auto; gap: 10px; align-items: center; padding: 8px 12px 8px 6px; border: 1px solid var(--seam); border-radius: var(--radius-m); background: var(--panel); }
  .order li.next { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .order li.done .title .tlink { color: var(--ink-soft); }
  .order li.out { opacity: .55; }
  .order li.out .tlink { text-decoration: line-through; }
  .order li.over { border-color: var(--accent); border-style: dashed; }
  .grip { display: flex; color: var(--ink-faint); cursor: grab; user-select: none; }
  .num { font-size: 20px; font-weight: 700; color: var(--ink-faint); text-align: right; font-variant-numeric: tabular-nums; }
  .order li.next .num { color: var(--accent-text); }
  .thumb { width: 46px; height: 62px; border-radius: var(--radius-s); background: var(--panel-2); overflow: hidden; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .info { min-width: 0; }
  .title { display: flex; align-items: center; gap: 8px; }
  .tlink { border: 0; background: none; padding: 0; font: inherit; font-weight: 700; font-size: 15.5px; color: var(--ink); cursor: pointer; text-align: left; overflow-wrap: anywhere; }
  .tlink:hover:not(:disabled) { text-decoration: underline; }
  .tlink:disabled { cursor: default; }
  .nexttag { font-size: 11.5px; font-weight: 700; padding: 1px 7px; border-radius: 9px; background: var(--accent); color: var(--accent-ink); }
  .sub { display: flex; flex-wrap: wrap; gap: 0 12px; font-size: 13px; color: var(--ink-faint); margin-top: 1px; }
  .sub .warn { color: var(--warn); }
  .note { display: block; border: 0; background: none; padding: 0; margin-top: 3px; font: inherit; font-size: 13.5px; color: var(--ink-soft); cursor: text; text-align: left; }
  .note::before { content: "Note · "; color: var(--ink-faint); }
  .note-input { margin-top: 4px; width: 100%; max-width: 520px; }
  .review { font-size: 13px; color: var(--ink-soft); font-style: italic; margin-top: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 560px; }
  .ctrl { display: flex; align-items: center; gap: 6px; }
  /* native selects size to their longest option, so every row lines up and nothing is cut */
  .status { width: auto; min-width: 150px; padding-right: 28px; font-weight: 600; }
  .status.s-ok { color: var(--ok); } .status.s-accent { color: var(--accent-text); } .status.s-info { color: var(--info); }
  .status.s-warn { color: var(--warn); } .status.s-bad { color: var(--bad); }
  .score { width: auto; min-width: 84px; padding-right: 28px; }
  .mini { display: flex; gap: 2px; }
  .mini button { display: grid; place-items: center; width: 26px; height: 26px; border: 0; border-radius: var(--radius-s); background: none; color: var(--ink-faint); cursor: pointer; }
  .mini button:hover:not(:disabled) { background: var(--panel-2); color: var(--ink); }
  .mini button:disabled { opacity: .3; cursor: default; }
</style>
