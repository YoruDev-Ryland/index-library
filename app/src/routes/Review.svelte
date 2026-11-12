<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { MetaCandidate, ReviewItem, SourceStatus } from "../lib/types";
  import CandidateThumb from "../components/CandidateThumb.svelte";
  import { consoleNote } from "../lib/console";
  import SkippedList from "../components/SkippedList.svelte";

  let items = $state<ReviewItem[] | null>(null);
  let queries = $state<Record<number, string>>({});
  let busy = $state<Record<number, boolean>>({});
  const load = () => api.reviewList().then((x) => { items = x; for (const i of x) queries[i.game_id] ??= i.title; }).catch((e) => app.fail(e));
  // again whenever a job changes the library (a lookup, or the review being settled)
  $effect(() => { void app.revision; load(); });

  // one console at a time, a page at a time
  const PAGE = 25;
  let sys = $state("");
  let shown = $state(PAGE);
  const systems = $derived.by(() => {
    const n = new Map<string, number>();
    for (const i of items ?? []) n.set(i.system_id, (n.get(i.system_id) ?? 0) + 1);
    return [...n].map(([id, count]) => ({ id, count, name: app.systemName(id) })).sort((a, b) => a.name.localeCompare(b.name));
  });
  const filtered = $derived((items ?? []).filter((i) => !sys || i.system_id === sys));
  const visible = $derived(filtered.slice(0, shown));
  function pick(id: string) { sys = id; shown = PAGE; }
  $effect(() => { if (sys && !systems.some((s) => s.id === sys)) sys = ""; });
  // candidates for another console, or barely alike in name, are folded away; they're rarely the one
  let opened = $state<Record<number, boolean>>({});
  let skippedList = $state<SkippedList>();

  let sources = $state<SourceStatus[]>([]);
  onMount(() => { api.sourcesStatus().then((x) => (sources = x)).catch(() => {}); });
  const using = $derived(sources.filter((x) => x.active));
  const unused = $derived(sources.filter((x) => !x.active && !x.id.startsWith("titledb-psp")));
  const switchMissing = $derived(sources.some((x) => x.id === "titledb-switch" && !x.active) && (items ?? []).some((i) => i.system_id === "switch"));
  async function getSwitchDb() {
    try { await api.downloadSwitchTitledb(); app.toast("Downloading the Switch title database — Switch games here are looked up again when it's done"); } catch (e) { app.fail(e); }
  }
  const drop = (id: number) => (items = (items ?? []).filter((x) => x.game_id !== id));
  const sourceName: Record<string, string> = { steamgriddb: "SteamGridDB", igdb: "IGDB", rawg: "RAWG", screenscraper: "ScreenScraper", "titledb-switch": "Switch title database", "titledb-psp": "PSP title list" };
  const year = (c: MetaCandidate) => c.release?.slice(0, 4) ?? "";

  async function choose(i: ReviewItem, c: MetaCandidate) {
    busy[i.game_id] = true;
    try { await api.chooseMetadata(i.game_id, c); drop(i.game_id); app.toast(`Saved info for ${i.title}`); app.refresh(); }
    catch (e) { app.fail(e); } finally { busy[i.game_id] = false; }
  }
  async function noMatch(i: ReviewItem) {
    try { await api.rejectMetadata(i.game_id); drop(i.game_id); skippedList?.reload(); } catch (e) { app.fail(e); }
  }
  // remembered: not suggested again until you look the game up yourself or bring it back below
  async function skip(i: ReviewItem) {
    try { await api.skip("match", i.skip_key, i.title, i.system_id); drop(i.game_id); skippedList?.reload(); } catch (e) { app.fail(e); }
  }
  async function search(i: ReviewItem) {
    const q = (queries[i.game_id] ?? "").trim();
    if (!q) return;
    busy[i.game_id] = true;
    try {
      const c = await api.searchMetadata(i.game_id, q);
      items = (items ?? []).map((x) => (x.game_id === i.game_id ? { ...x, candidates: c } : x));
      if (c.length === 0) app.toast(`Nothing found for “${q}”`, "error");
    } catch (e) { app.fail(e); } finally { busy[i.game_id] = false; }
  }
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Match review</h1>
      <p>Games where the lookup wasn't sure. Click the right cover to use it, search again with a different name, or mark the game as having no match. Titles that differ only in a subtitle, “Disney's”, “&” or numerals are matched for you when the source confirms the console.</p>
      {#if sources.length}
        <p class="srcline">
          <span>Looked up in: {using.length ? using.map((x) => x.name).join(", ") : "nothing yet"}</span>
          {#if unused.length}<span class="faint">· Not used: {unused.map((x) => x.name).join(", ")} — see Settings → Game info sources</span>{/if}
        </p>
      {/if}
      {#if switchMissing}
        <p class="hintbox">Switch games match exactly by title ID with the Switch title database, which also has new releases. <button class="btn small primary" onclick={getSwitchDb}>Download it (~90 MB)</button></p>
      {/if}
    </div>
    {#if items?.length}<span class="spacer"></span><span class="muted">{items.length} to review</span>{/if}
  </div>
  {#if systems.length > 1}
    <div class="filter">
      <label for="review-sys">Console</label>
      <select id="review-sys" value={sys} onchange={(e) => pick((e.currentTarget as HTMLSelectElement).value)}>
        <option value="">All consoles · {items?.length ?? 0}</option>
        {#each systems as s (s.id)}<option value={s.id}>{s.name} · {s.count}</option>{/each}
      </select>
    </div>
  {/if}
  {#if items === null}<p class="faint">Loading…</p>
  {:else if items.length === 0}<div class="empty"><h2>Nothing to review</h2><p>Use “Get game info” in the library. Games that need a decision show up here.</p></div>
  {:else}
    {#each visible as i (i.game_id)}
      {@const likely = (c: MetaCandidate) => c.console !== "other" && c.score >= 0.3}
      {@const here = i.candidates.filter(likely)}
      {@const elsewhere = i.candidates.filter((c) => !likely(c))}
      <section class="item" class:busy={busy[i.game_id]}>
        <div class="who">
          <h2>{i.title}</h2>
          <p class="muted">{app.systemName(i.system_id)}</p>
          {#if i.file_name}<p class="faint file">{i.file_name}</p>{/if}
          <form class="search" onsubmit={(e) => { e.preventDefault(); search(i); }}>
            <input type="text" bind:value={queries[i.game_id]} aria-label="Search name for {i.title}" />
            <button class="btn small" type="submit" disabled={busy[i.game_id]}>Search</button>
          </form>
          <div class="acts">
            <button class="btn small" onclick={() => noMatch(i)} title="None of these: the game keeps no info, and isn't looked up again until you do it yourself">No match</button>
            <button class="btn small ghost" onclick={() => skip(i)} title="Not now: it isn't suggested again until you look the game up yourself or bring it back below">Skip</button>
          </div>
        </div>
        <div class="cands">
          {#each opened[i.game_id] || here.length === 0 ? [...here, ...elsewhere] : here as c (c.source + c.source_id)}
            {@const cn = consoleNote(c, app.systemName(i.system_id))}
            <button class="cand" class:wrong={c.console === "other"} disabled={busy[i.game_id]} onclick={() => choose(i, c)} title="Use “{c.title}”">
              <CandidateThumb candidate={c} />
              <span class="name">{c.title}</span>
              <span class="meta">{sourceName[c.source] ?? c.source}{year(c) ? ` · ${year(c)}` : ""}</span>
              <span class="score" class:good={c.score >= 0.85}>{Math.round(c.score * 100)}% name match</span>
              <span class="gpill {cn.tone} cons" title={(c.platforms ?? []).join(", ")}>{cn.text}</span>
            </button>
          {:else}
            <p class="faint none">No candidates. Try searching with a different name.</p>
          {/each}
          {#if elsewhere.length && here.length && !opened[i.game_id]}
            <button class="more-link elsewhere" onclick={() => (opened[i.game_id] = true)}>
              Show {elsewhere.length} {elsewhere.every((c) => c.console === "other") ? "for other consoles" : "less alike in name"}
            </button>
          {/if}
        </div>
      </section>
    {/each}
    {#if filtered.length > shown}
      <div class="more">
        <button class="btn" onclick={() => (shown += PAGE)}>Show {Math.min(PAGE, filtered.length - shown)} more</button>
        <span class="faint">{shown} of {filtered.length}</span>
      </div>
    {/if}
  {/if}
  {#if items !== null}<SkippedList bind:this={skippedList} kind="match" noun={{ one: "game", many: "games" }} onchange={load} />{/if}
</div>

<style>
  .srcline { display: flex; flex-wrap: wrap; gap: 4px 8px; font-size: 13.5px; margin-top: 6px; }
  .hintbox { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-top: 8px; padding: 8px 12px; border: 1px solid var(--seam); border-radius: var(--radius-m); background: var(--panel); }
  .spacer { flex: 1; }
  .item { display: grid; grid-template-columns: 260px 1fr; gap: 20px; padding: 18px 0; border-bottom: 1px solid var(--seam); }
  .item.busy { opacity: .6; }
  .who h2 { font-size: 18px; line-height: 1.2; }
  .who p { margin: 3px 0 0; }
  .file { font-size: 13px; word-break: break-word; }
  .search { display: flex; gap: 6px; margin-top: 12px; }
  .search input { flex: 1; height: 28px; }
  .acts { display: flex; gap: 6px; margin-top: 8px; }
  .cands { display: grid; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); gap: 14px 12px; align-content: start; }
  .cand { display: flex; flex-direction: column; gap: 4px; padding: 0; border: 0; background: none; text-align: left; cursor: pointer; border-radius: var(--radius-s); }
  .cand:hover :global(.thumb), .cand:focus-visible :global(.thumb) { outline: 3px solid var(--accent); outline-offset: 1px; }
  .name { font-weight: 600; line-height: 1.2; font-size: 14px; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .meta, .score { font-size: 12.5px; color: var(--ink-soft); }
  .score.good { color: var(--ok); }
  .none { grid-column: 1 / -1; }
  .cons { font-size: 12px; font-weight: 500; white-space: normal; line-height: 1.3; }
  .cand.wrong { opacity: .6; }
  .filter { display: flex; align-items: center; gap: 10px; margin: 4px 0 6px; font-size: 13.5px; color: var(--ink-soft); }
  .elsewhere { grid-column: 1 / -1; justify-self: start; }
  .more { display: flex; align-items: center; gap: 12px; padding: 18px 0 4px; }
</style>
