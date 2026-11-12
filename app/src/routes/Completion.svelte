<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { Completion, CompletionEntry, CompletionOptions, CompletionState, CompletionSummary } from "../lib/types";
  import { blankCopy } from "../lib/shelf";

  // what counts, remembered on this PC
  const KEY = "lo.completion";
  function readOpts(): CompletionOptions {
    // English releases by default: every language at once is thousands of games you'd never collect
    const def: CompletionOptions = { regions: [], unlicensed: false, aftermarket: false, languages: ["En"], source: "digital" };
    try { const o = JSON.parse(localStorage.getItem(KEY) ?? "{}"); return { regions: o.regions ?? [], unlicensed: !!o.unlicensed, aftermarket: !!o.aftermarket, languages: o.languages ?? def.languages, source: o.source ?? def.source }; }
    catch { return def; }
  }
  let opts = $state<CompletionOptions>(readOpts());
  $effect(() => { const o = $state.snapshot(opts); try { localStorage.setItem(KEY, JSON.stringify(o)); } catch { /* this session only */ } });

  let summary = $state<CompletionSummary[] | null>(null);
  let system = $state<string | null>(null);
  let detail = $state<Completion | null>(null);
  let loading = $state(false);
  let filter = $state<"all" | CompletionState>("missing");
  let query = $state("");

  const pct = (have: number, total: number) => (total ? Math.floor((have / total) * 1000) / 10 : 0);
  const fmtPct = (n: number) => (n >= 100 ? "100" : n.toFixed(1).replace(/\.0$/, ""));

  async function loadSummary() {
    try {
      summary = await api.completionSummary($state.snapshot(opts));
      if (!system || !summary.some((s) => s.system === system)) system = summary[0]?.system ?? null;
    } catch (e) { app.fail(e); summary = []; }
  }
  async function loadDetail() {
    if (!system) { detail = null; return; }
    loading = true;
    try { detail = await api.completion(system, $state.snapshot(opts)); } catch (e) { app.fail(e); } finally { loading = false; }
  }
  // options or the library changed: measure again
  $effect(() => { void app.revision; void opts.unlicensed; void opts.aftermarket; void opts.regions.join(); void opts.languages.join(); void opts.source; loadSummary(); });
  $effect(() => { void system; void app.revision; void opts.unlicensed; void opts.aftermarket; void opts.regions.join(); void opts.languages.join(); void opts.source; loadDetail(); });

  /** "On my shelf": a copy of the preferred release, loose until you say more */
  async function shelve(e: CompletionEntry) {
    if (!detail) return;
    try {
      const c = await api.shelfAdd(detail.system, e.release, null, { ...blankCopy() });
      app.toast(`On your shelf: ${e.title}. Open it in the library to add its box, condition and price.`);
      lastShelved = c.id;
      await loadDetail(); loadSummary();
    } catch (err) { app.fail(err); }
  }
  let lastShelved = $state<string | null>(null);
  async function undoShelve() {
    if (!lastShelved) return;
    try { await api.shelfRemove(lastShelved); lastShelved = null; await loadDetail(); loadSummary(); } catch (err) { app.fail(err); }
  }

  const regionChoices = $derived(app.settings?.region_order ?? ["USA", "Europe", "Japan"]);
  function toggleRegion(r: string) { opts.regions = opts.regions.includes(r) ? opts.regions.filter((x) => x !== r) : [...opts.regions, r]; }

  const shown = $derived.by(() => {
    if (!detail) return [] as CompletionEntry[];
    const q = query.trim().toLowerCase();
    return detail.entries.filter((e) => (filter === "all" || e.state === filter) && (!q || e.title.toLowerCase().includes(q) || e.releases.some((r) => r.toLowerCase().includes(q))));
  });
  const LAMP: Record<CompletionState, { tone: string; label: string }> = {
    have: { tone: "ok", label: "Have" },
    unverified: { tone: "warn", label: "Not verified" },
    missing: { tone: "", label: "Missing" },
  };

  function show(e: CompletionEntry) {
    if (e.game_id == null || !detail) return;
    app.filter = { system: detail.system };
    app.view = "library";
    app.selectOnly(e.game_id);
  }
  async function copy(text: string) {
    try { await navigator.clipboard.writeText(text); app.toast(`Copied “${text}”`); } catch (e) { app.fail(e); }
  }
  async function copyMissing() {
    if (!detail) return;
    const names = detail.entries.filter((e) => e.state === "missing").map((e) => e.release);
    try { await navigator.clipboard.writeText(names.join("\n")); app.toast(`Copied ${names.length.toLocaleString()} missing releases`); } catch (e) { app.fail(e); }
  }
  async function getLists() {
    try { await api.downloadDats(); app.toast("Downloading game lists — progress is in the jobs bar"); } catch (e) { app.fail(e); }
  }
</script>

<div class="page comp">
  <div class="page-head">
    <div>
      <h1>Completion</h1>
      <p>Every game in each console's list once (1G1R): regional releases and re-releases count as one, and having any of them counts. Verified games matched the list's fingerprints; the others are here by name.</p>
    </div>
  </div>

  <div class="opts" role="group" aria-label="What counts">
    <span class="lbl">Count</span>
    <div class="seg">
      {#each [["digital", "Files"], ["physical", "Shelf"], ["either", "Either"]] as [v, l] (v)}
        <button class:on={opts.source === v} aria-pressed={opts.source === v} onclick={() => (opts.source = v as CompletionOptions["source"])} title={v === "digital" ? "Games you have files of" : v === "physical" ? "Games on your shelf" : "Games you have either way"}>{l}</button>
      {/each}
    </div>
    <span class="lbl">Games in</span>
    <div class="seg">
      <button class:on={opts.languages.includes("En")} aria-pressed={opts.languages.includes("En")} onclick={() => (opts.languages = ["En"])}>English</button>
      <button class:on={!opts.languages.length} aria-pressed={!opts.languages.length} onclick={() => (opts.languages = [])}>Any language</button>
    </div>
    <span class="lbl">Released in</span>
    <div class="seg">
      <button class:on={!opts.regions.length} aria-pressed={!opts.regions.length} onclick={() => (opts.regions = [])}>Any region</button>
      {#each regionChoices as r (r)}<button class:on={opts.regions.includes(r)} aria-pressed={opts.regions.includes(r)} onclick={() => toggleRegion(r)}>{r}</button>{/each}
    </div>
    <label class="check"><input type="checkbox" bind:checked={opts.unlicensed} /> Unlicensed</label>
    <label class="check"><input type="checkbox" bind:checked={opts.aftermarket} /> Aftermarket and homebrew</label>
    <span class="note">Betas, prototypes, demos and BIOS files never count.</span>
  </div>

  {#if summary === null}<p class="faint">Measuring your collection…</p>
  {:else if summary.length === 0}
    <div class="empty"><h2>No game lists yet</h2><p>Completion is measured against each console's No-Intro or Redump list.</p><button class="btn primary" onclick={getLists}>Download game lists</button></div>
  {:else}
    <div class="split">
      <ul class="systems" aria-label="Consoles">
        {#each summary as s (s.system)}
          {@const p = pct(s.have + s.unverified, s.total)}
          <li>
            <button class:on={system === s.system} aria-current={system === s.system ? "true" : undefined} onclick={() => (system = s.system)}>
              <span class="row"><span class="name">{s.name}</span><span class="num">{fmtPct(p)}%</span></span>
              <span class="meter" aria-hidden="true"><span class="fill" style:width="{pct(s.have, s.total)}%"></span><span class="maybe" style:width="{pct(s.unverified, s.total)}%"></span></span>
              <span class="sub">{(s.have + s.unverified).toLocaleString()} of {s.total.toLocaleString()}{s.unverified ? ` · ${s.unverified.toLocaleString()} not verified` : ""}</span>
            </button>
          </li>
        {/each}
      </ul>

      <section class="detail" aria-busy={loading}>
        {#if detail}
          {@const p = pct(detail.have + detail.unverified, detail.total)}
          <header>
            <div class="big"><span class="pc">{fmtPct(p)}%</span><span class="of">{(detail.have + detail.unverified).toLocaleString()} of {detail.total.toLocaleString()} games {opts.source === "physical" ? "on your shelf" : opts.source === "either" ? "here or on your shelf" : `here · ${detail.have.toLocaleString()} verified`}</span></div>
            <p class="facts">
              {#if detail.unverified}<span><span class="chip warn">{detail.unverified.toLocaleString()} here but not verified</span></span>{/if}
              <span class="faint">Against {detail.list ?? "the console's list"}{detail.clone_list ? ", with Retool's clone list joining differently named releases" : ""}.</span>
            </p>
            {#if detail.unverified}<p class="hint">Games “not verified” are here under a matching name, but no file matched the list's fingerprints: an unfingerprinted file, a different dump, or a format the list doesn't describe (CHD, CIA…). Fingerprint the console to check them.</p>{/if}
          </header>
          <div class="bar">
            <div class="page-tabs" role="tablist">
              {#each [["missing", "Missing", detail.missing], ["unverified", "Not verified", detail.unverified], ["have", "Have", detail.have], ["all", "All", detail.total]] as [k, l, n] (k)}
                <button role="tab" aria-selected={filter === k} class:on={filter === k} onclick={() => (filter = k as typeof filter)}>{l} <span>{(n as number).toLocaleString()}</span></button>
              {/each}
            </div>
            <span class="spacer"></span>
            <input type="search" placeholder="Search" aria-label="Search games" bind:value={query} />
            {#if lastShelved}<button class="btn small ghost" onclick={undoShelve}>Undo last</button>{/if}
            {#if detail.missing}<button class="btn small" onclick={copyMissing} title="Copy the preferred release of every missing game, one per line">Copy missing list</button>{/if}
          </div>
          {#if shown.length === 0}<p class="faint none">{query ? "No games match." : filter === "missing" ? "Nothing missing: this console is complete." : "None."}</p>
          {:else}
            <ul class="games">
              {#each shown as e (e.title + e.release)}
                <li>
                  <span class="chip {LAMP[e.state].tone}">{LAMP[e.state].label}</span>
                  <div class="body">
                    <div class="title">{e.title}</div>
                    <div class="meta">
                      <span>{e.release}</span>
                      {#if e.releases.length > 1}<span title={e.releases.join("\n")}>{e.releases.length} releases</span>{/if}
                      {#if e.regions.length}<span>{e.regions.join(", ")}</span>{/if}
                      {#if e.shelf}<span class="onshelf">{e.shelf === 1 ? "On your shelf" : `${e.shelf} on your shelf`}</span>{/if}
                    </div>
                  </div>
                  {#if !e.shelf}<button class="btn small ghost" onclick={() => shelve(e)} title="Add a copy of {e.release} to your shelf">On my shelf</button>{/if}
                  {#if e.game_id != null}<button class="btn small ghost" onclick={() => show(e)}>Show</button>
                  {:else}<button class="btn small ghost" onclick={() => copy(e.release)} title="Copy the release name">Copy name</button>{/if}
                </li>
              {/each}
            </ul>
          {/if}
        {:else if loading}<p class="faint">Measuring…</p>{/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .opts { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 16px; margin-bottom: 16px; }
  .opts .lbl { font-weight: 600; font-size: 13px; color: var(--ink-soft); }
  .seg { display: inline-flex; border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s); overflow: hidden; }
  .seg button { border: 0; background: var(--panel); padding: 0 12px; height: 28px; cursor: pointer; color: var(--ink-soft); font-weight: 500; }
  .seg button + button { border-left: 1px solid var(--seam); }
  .seg button.on { background: var(--ink); color: var(--plastic); font-weight: 600; }
  .check { display: inline-flex; align-items: center; gap: 6px; font-weight: 500; cursor: pointer; }
  .opts .note { font-size: 13px; color: var(--ink-faint); }

  /* the page doesn't scroll: the consoles and the games scroll each on their own */
  .page.comp { display: flex; flex-direction: column; overflow: hidden; padding-bottom: 0; }
  .split { flex: 1; min-height: 0; display: grid; grid-template-columns: 250px minmax(0, 1fr); gap: 20px; }
  .systems { list-style: none; margin: 0; padding: 0 6px 24px 0; display: flex; flex-direction: column; gap: 2px; overflow-y: auto; min-height: 0; }
  .detail { overflow-y: auto; min-height: 0; padding: 0 4px 32px 0; }
  .systems button { width: 100%; text-align: left; border: 0; background: none; padding: 7px 10px 8px; border-radius: var(--radius-s); cursor: pointer; display: flex; flex-direction: column; gap: 4px; color: var(--ink); }
  .systems button:hover { background: var(--panel); }
  .systems button.on { background: var(--panel); box-shadow: inset 0 0 0 1px var(--seam); }
  .systems .row { display: flex; align-items: baseline; gap: 8px; }
  .systems .name { flex: 1; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .systems .num { font-variant-numeric: tabular-nums; font-weight: 600; }
  .systems .sub { font-size: 12.5px; color: var(--ink-faint); font-variant-numeric: tabular-nums; }
  .meter { display: flex; height: 4px; border-radius: 2px; background: var(--panel-2); overflow: hidden; }
  .meter .fill { background: var(--ink-soft); }
  .meter .maybe { background: color-mix(in srgb, var(--warn) 55%, transparent); }
  .systems button.on .meter .fill { background: var(--accent); }

  .detail header { margin-bottom: 10px; }
  .big { display: flex; align-items: baseline; gap: 12px; }
  .pc { font-size: 34px; font-weight: 600; font-variant-numeric: tabular-nums; line-height: 1.1; }
  .of { color: var(--ink-soft); font-size: 15px; font-variant-numeric: tabular-nums; }
  .facts { display: flex; flex-wrap: wrap; gap: 6px 14px; align-items: center; margin: 6px 0 0; font-size: 13px; }
  .hint { margin: 6px 0 0; font-size: 13px; color: var(--ink-soft); max-width: 75ch; }
  .bar { display: flex; align-items: flex-end; gap: 10px; position: sticky; top: 0; background: var(--plastic); padding-top: 6px; z-index: 1; }
  .bar .page-tabs { margin-bottom: 0; flex: 1; }
  .bar .spacer { display: none; }
  .bar input { width: 200px; margin-bottom: 6px; }
  .bar .btn { margin-bottom: 6px; }
  .page-tabs span { color: var(--ink-faint); font-weight: 500; margin-left: 4px; }
  .none { padding: 18px 0; }
  .games { list-style: none; margin: 0; padding: 0; }
  .games li { display: flex; gap: 12px; align-items: center; padding: 7px 2px; border-bottom: 1px solid color-mix(in srgb, var(--seam) 60%, transparent); content-visibility: auto; contain-intrinsic-size: auto 46px; }
  .games .chip { flex: none; width: 112px; }
  .body { flex: 1; min-width: 0; }
  .title { font-weight: 600; overflow-wrap: anywhere; }
  .meta { display: flex; flex-wrap: wrap; gap: 2px 14px; font-size: 13px; color: var(--ink-faint); }
  .meta .onshelf { color: var(--ink-soft); font-weight: 500; }
  @media (max-width: 900px) { .split { grid-template-columns: 1fr; grid-template-rows: minmax(120px, 30%) 1fr; } }
</style>
