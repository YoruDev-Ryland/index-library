<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { bytes, makerOf } from "../lib/format";
  import type { PlayEntry, PlaySummary, Stats } from "../lib/types";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { PLAY } from "../lib/play";
  let s = $state<Stats | null>(null);
  let play = $state<PlaySummary | null>(null);
  $effect(() => {
    void app.revision;
    api.stats().then((x) => (s = x)).catch((e) => app.fail(e));
    api.playSummary().then((x) => (play = x)).catch(() => (play = null));
  });
  const marked = $derived(play ? Object.values(play.counts).reduce((a, n) => a + n, 0) : 0);
  const anyHashed = $derived(!!s && s.systems.some((x) => x.hashed > 0));
  const anyVerified = $derived(!!s && s.systems.some((x) => x.verified > 0));
  function openGame(e: PlayEntry) {
    if (e.game_id === null) return;
    app.filter = { system: e.system }; app.focusId = e.game_id; app.selection = new Set([e.game_id]); app.view = "library";
  }
  async function hashAll() { try { await api.hash({ kind: "All" }); app.toast("Fingerprinting the library — progress is in the jobs bar"); } catch (e) { app.fail(e); } }
  const short = (d: string | null) => d ? new Date(d + "T00:00").toLocaleDateString(undefined, { month: "short", day: "numeric" }) : "";
  const totals = $derived(s ? s.systems.reduce((a, x) => ({ games: a.games + x.games, bytes: a.bytes + x.bytes, hashed: a.hashed + x.hashed, files: a.files + x.files, verified: a.verified + x.verified }), { games: 0, bytes: 0, hashed: 0, files: 0, verified: 0 }) : null);
  const maxBytes = $derived(s ? Math.max(1, ...s.systems.map((x) => x.bytes)) : 1);
  const sorted = $derived(s ? [...s.systems].sort((a, b) => b.bytes - a.bytes) : []);
  const needDecrypt = $derived(s ? s.systems.reduce((a, x) => a + (x.status_counts["decrypted=false"] ?? 0), 0) : 0);
  function open(id: string, extra: Record<string, string | null> = {}) { app.filter = { system: id, ...extra }; app.view = "library"; }
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Overview</h1>
      {#if totals}<p>{totals.games.toLocaleString()} games in {s!.systems.length} systems, {bytes(totals.bytes)}.{#if anyHashed} {Math.round((totals.hashed / Math.max(1, totals.files)) * 100)}% fingerprinted{#if anyVerified}, {totals.verified.toLocaleString()} files verified against DATs{/if}.{/if}</p>{/if}
    </div>
  </div>

  {#if play}
    <section class="play">
      <div class="col">
        <h2>Playing now</h2>
        {#if play.playing.length}
          <div class="strip">
            {#each play.playing as e (e.title + e.system)}
              <button class="pcard" onclick={() => openGame(e)} disabled={e.game_id === null}>
                <span class="art">{#if e.cover}<img src={convertFileSrc(e.cover)} alt="" crossorigin="anonymous" loading="lazy" />{/if}</span>
                <span class="pt">{e.title}</span>
                <span class="faint small">{app.systemName(e.system)}{e.started ? ` · since ${short(e.started)}` : ""}{e.hours ? ` · ${e.hours} h` : ""}</span>
              </button>
            {/each}
          </div>
        {:else}<p class="muted empty-note">Nothing marked as playing. Open a game and set its status in “Your play”.</p>{/if}
        <h2 class="mt">Recently finished</h2>
        {#if play.finished.length}
          <ul class="done">
            {#each play.finished as e (e.title + e.system)}
              <li><button onclick={() => openGame(e)} disabled={e.game_id === null}>
                <span class="dtitle">{e.title}</span>
                <span class="faint small">{app.systemName(e.system)}</span>
                <span class="faint small dd">{short(e.finished)}</span>
                <span class="dscore">{#if e.score}{e.score}<small>/10</small>{/if}</span>
              </button></li>
            {/each}
          </ul>
        {:else}<p class="muted empty-note">Finished games appear here with your score.</p>{/if}
      </div>
      <div class="col side">
        <h2>Your numbers</h2>
        <dl class="nums">
          <dt>Marked</dt><dd>{marked.toLocaleString()} game{marked === 1 ? "" : "s"}</dd>
          {#if play.average_score !== null}<dt>Average score</dt><dd>{play.average_score.toFixed(1)}<small>/10</small></dd>{/if}
          {#if play.hours}<dt>Hours logged</dt><dd>{Math.round(play.hours).toLocaleString()}</dd>{/if}
          {#if play.favorites}<dt>Favourites</dt><dd>{play.favorites}</dd>{/if}
        </dl>
        <ul class="statuses">
          {#each PLAY.filter((p) => play!.counts[p.id]) as p (p.id)}
            <li><button onclick={() => { app.filter = { play: p.id }; app.view = "library"; }}><span class="gpill {p.tone}">{p.label}</span><span class="n">{play!.counts[p.id]}</span></button></li>
          {/each}
        </ul>
        {#if app.collections.length}
          <h2 class="mt">Collections</h2>
          <ul class="colls">
            {#each app.collections as c (c.id)}
              <li><button onclick={() => app.openCollection(c.id)}>
                <span class="cn">{c.name}</span><span class="faint small">{c.done}/{c.count - c.skipped}</span>
                <span class="bar2"><span style:width="{Math.round((c.done / Math.max(1, c.count - c.skipped)) * 100)}%"></span></span>
                {#if c.next}<span class="faint small nx">Up next: {c.next}</span>{/if}
              </button></li>
            {/each}
          </ul>
        {/if}
      </div>
    </section>
  {/if}

  {#if s}
    <h2 class="mt storage">Storage</h2>
    <div class="callouts">
      {#if needDecrypt > 0}<button class="callout warn" onclick={() => { app.filter = { status_key: "decrypted", status_value: "false" }; app.view = "library"; }}><b>{needDecrypt}</b> game{needDecrypt === 1 ? "" : "s"} still encrypted</button>{/if}
      {#if s.dupe_bytes > 0}<button class="callout" onclick={() => (app.view = "duplicates")}><b>{bytes(s.dupe_bytes)}</b> in duplicate copies</button>{/if}
      {#if !anyHashed}<button class="callout" onclick={hashAll}><b>Not fingerprinted yet</b> — fingerprint every file to find identical copies and verify dumps</button>{/if}
      {#if s.archive_count > 0}<button class="callout" onclick={() => (app.view = "archive")}><b>{bytes(s.archive_bytes)}</b> waiting in the archive ({s.archive_count} files)</button>{/if}
    </div>
    <table class="list">
      <thead><tr><th>System</th><th class="num">Games</th><th style="width: 34%">Size</th>{#if anyHashed}<th class="num">Fingerprinted</th>{/if}{#if anyVerified}<th class="num">Verified</th>{/if}<th>Status</th></tr></thead>
      <tbody>
        {#each sorted as x (x.id)}
          <tr>
            <td><button class="sys" onclick={() => open(x.id)}><span class="dot" style="background: {makerOf(x.id).color}"></span>{x.name}</button></td>
            <td class="num">{x.games.toLocaleString()}</td>
            <td><div class="barrow"><div class="bar"><span style="width: {Math.max(0.5, (x.bytes / maxBytes) * 100)}%; background: {makerOf(x.id).color}"></span></div><em>{bytes(x.bytes)}</em></div></td>
            {#if anyHashed}<td class="num">{x.hashed ? `${Math.round((x.hashed / Math.max(1, x.files)) * 100)}%` : "—"}</td>{/if}
            {#if anyVerified}<td class="num">{x.verified ? x.verified.toLocaleString() : "—"}</td>{/if}
            <td>
              {#if x.status_counts["decrypted=true"]}<span class="chip ok">{x.status_counts["decrypted=true"]} decrypted</span>{/if}
              {#if x.status_counts["decrypted=false"]}<button class="chip warn asbtn" onclick={() => open(x.id, { status_key: "decrypted", status_value: "false" })}>{x.status_counts["decrypted=false"]} encrypted</button>{/if}
              {#if x.status_counts["compressed=true"]}<span class="chip info">{x.status_counts["compressed=true"]} compressed</span>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .play { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 28px; margin-bottom: 28px; padding-bottom: 24px; border-bottom: 1px solid var(--seam); }
  @media (max-width: 1100px) { .play { grid-template-columns: 1fr; } }
  .play h2 { font-size: 15px; margin-bottom: 10px; }
  .mt { margin-top: 22px; }
  h2.storage { margin: 0 0 12px; font-size: 15px; }
  .small { font-size: 12.5px; }
  .empty-note { margin: 0; font-size: 13.5px; }
  .strip { display: grid; grid-template-columns: repeat(auto-fill, minmax(124px, 1fr)); gap: 14px; }
  .pcard { display: flex; flex-direction: column; gap: 4px; border: 0; background: none; padding: 0; text-align: left; cursor: pointer; min-width: 0; }
  .pcard .art { aspect-ratio: 2 / 3; border-radius: var(--radius-s); overflow: hidden; background: var(--panel-2); border: 1px solid var(--seam); }
  .pcard .art img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .pcard:hover .art { border-color: var(--key-edge); }
  .pt { font-weight: 600; line-height: 1.25; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .done, .statuses, .colls { list-style: none; margin: 0; padding: 0; }
  .done li button { display: grid; grid-template-columns: minmax(0, 1fr) auto 56px 48px; gap: 12px; align-items: baseline; width: 100%; border: 0; border-bottom: 1px solid color-mix(in srgb, var(--seam) 55%, transparent); background: none; padding: 7px 2px; text-align: left; cursor: pointer; }
  .done li button:hover { background: var(--panel); }
  .dtitle { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dd { text-align: right; }
  .dscore { text-align: right; font-weight: 700; } .dscore small, .nums small { color: var(--ink-faint); font-weight: 500; }
  .nums { display: grid; grid-template-columns: 1fr auto; gap: 5px 12px; margin: 0 0 12px; font-size: 14px; }
  .nums dt { color: var(--ink-soft); } .nums dd { margin: 0; font-weight: 600; text-align: right; }
  .statuses li button, .colls li button { display: flex; align-items: center; gap: 8px; width: 100%; border: 0; background: none; padding: 5px 2px; cursor: pointer; text-align: left; }
  .statuses li button:hover, .colls li button:hover { background: var(--panel); }
  .statuses .n { margin-left: auto; font-weight: 600; }
  .colls li button { flex-wrap: wrap; padding: 6px 2px; }
  .cn { font-weight: 600; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar2 { flex: 1 0 100%; height: 3px; border-radius: 2px; background: color-mix(in srgb, var(--seam) 70%, transparent); overflow: hidden; }
  .bar2 span { display: block; height: 100%; background: var(--ok); }
  .nx { flex: 1 0 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .callouts { display: flex; gap: 10px; flex-wrap: wrap; margin-bottom: 20px; }
  .callout { border: 1px solid var(--seam); border-bottom-color: var(--key-edge); background: var(--panel); color: var(--ink); border-radius: var(--radius-s); padding: 10px 14px; cursor: pointer; text-align: left; }
  .callout:hover { background: var(--panel-2); }
  .callout b { font-size: 18px; margin-right: 4px; }
  .callout.warn { border-color: color-mix(in srgb, var(--warn) 50%, var(--seam)); }
  .sys { display: flex; align-items: center; gap: 8px; border: 0; background: none; cursor: pointer; font-weight: 500; padding: 0; }
  .barrow { display: flex; align-items: center; gap: 8px; }
  .bar { position: relative; flex: 1; height: 10px; background: var(--panel-2); border-radius: 2px; }
  .bar span { position: absolute; inset: 0 auto 0 0; border-radius: 2px; opacity: .8; }
  .barrow em { font-style: normal; font-size: 13px; width: 64px; text-align: right; color: var(--ink-soft); }
  .asbtn { border: 0; background: none; padding: 0; font: inherit; font-size: 12.5px; font-weight: 600; color: var(--ink); cursor: pointer; }
  .asbtn:hover { text-decoration: underline; text-underline-offset: 3px; }
</style>
