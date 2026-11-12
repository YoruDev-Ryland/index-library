<script lang="ts">
  import Icon from "./Icon.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { bytes, makerOf } from "../lib/format";
  import { collapse, placeOf } from "../lib/files";
  import type { ActionInfo, FileDetail, GameDetail, PlayInfo, PlayStatus } from "../lib/types";
  import { PLAY, emptyPlay } from "../lib/play";
  import StatusChips from "./StatusChips.svelte";
  import RenameModal from "./RenameModal.svelte";
  import ActionConfirm from "./ActionConfirm.svelte";
  import AddToShelf from "./AddToShelf.svelte";
  import CopyForm from "./CopyForm.svelte";
  import { conditionLabel, fieldsOf, money, partsLabel } from "../lib/shelf";
  import type { CopyFields, ShelfCopy } from "../lib/types";

  let { id }: { id: number } = $props();
  let d = $state<GameDetail | null>(null);
  let extraArt = $state<Record<string, string>>({});
  let renaming = $state(false);
  let running = $state<{ action: ActionInfo; ids: number[] } | null>(null);
  let longDesc = $state(false);
  let openHashes = $state<Set<number>>(new Set());
  let broken = $state<Set<string>>(new Set());

  // your play status: edited here, saved as you go
  let play = $state<PlayInfo>(emptyPlay());
  let review = $state("");
  let addColl = $state("");
  // your physical copies: adding one, and the one being edited
  let adding = $state(false);
  let editing = $state<{ id: string; fields: CopyFields } | null>(null);
  async function saveCopy() {
    if (!editing) return;
    try { await api.shelfUpdate(editing.id, $state.snapshot(editing.fields)); editing = null; app.refresh(); } catch (e) { app.fail(e); }
  }
  let removing = $state<string | null>(null);
  async function removeCopy(c: ShelfCopy) {
    if (removing !== c.id) { removing = c.id; return; }
    try { await api.shelfRemove(c.id); removing = null; editing = null; app.refresh(); if (d && d.files.length === 0 && d.copies.length === 1) app.focusId = null; } catch (e) { app.fail(e); }
  }
  /** the release a new copy most likely is: the one this game's files were verified as */
  const likelyRelease = $derived(d?.files.find((f) => f.dat_status === "verified" && f.dat_game)?.dat_game ?? null);
  $effect(() => {
    void app.revision;
    api.gameDetail(id).then((x) => {
      d = x;
      play = x.play ?? emptyPlay();
      review = play.notes ?? "";
      // wide hero art and the logo come from SteamGridDB the first time a game is opened
      if (!x.art._extra) api.fetchGameArt(id).then((a) => (extraArt = a)).catch(() => {});
    }).catch(() => (d = null));
  });

  const roots = $derived(app.settings?.library_roots ?? []);
  const art = $derived({ ...(d?.art ?? {}), ...extraArt });
  const ok = (p: string | undefined) => (p && !broken.has(p) ? p : undefined);
  const src = (p: string) => convertFileSrc(p);
  const fail = (p: string) => () => { broken = new Set([...broken, p]); };
  // the backdrop: wide art first, then the cover blurred, then the console's photo
  const wide = $derived(ok(art.hero) ?? ok(art.banner) ?? ok(art.screenshot));
  const cover = $derived(ok(art.cover) ?? ok(art.icon));
  const logo = $derived(ok(art.logo));
  const sysImg = $derived(d ? app.systemImages[d.row.system_id] : undefined);
  const backdrop = $derived(wide ?? cover ?? sysImg);
  // a cover stretched to fill the header is blurred; the console's photo is shown as it is, darkened
  const blurred = $derived(!wide && !!cover && backdrop === cover);
  const consolePhoto = $derived(!wide && !cover && !!sysImg);
  const meta = $derived(d?.meta && !d.meta.candidates ? d.meta : null);
  const maker = $derived(d ? makerOf(d.row.system_id) : null);

  // ---- facts ----
  function niceDate(s: string | null | undefined): string | null {
    if (!s) return null;
    const m = /^(\d{4})-?(\d{2})-?(\d{2})/.exec(s);
    if (m) return new Date(+m[1], +m[2] - 1, +m[3]).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
    if (/^\d+$/.test(s) && s.length >= 9) return new Date(+s * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
    return s;
  }
  const sourceName: Record<string, string> = { steamgriddb: "SteamGridDB", igdb: "IGDB", rawg: "RAWG", screenscraper: "ScreenScraper", "titledb-switch": "Switch title database", "titledb-psp": "PSP title list" };
  const totalSize = $derived((d?.files ?? []).reduce((a, f) => a + f.size, 0));

  // ---- what each file is ----
  const SW_ID = /\[([0-9A-Fa-f]{16})\]/;
  function tidOf(f: FileDetail): string | null {
    const p = f.probe?.id;
    if (p && /^[0-9A-Fa-f]{16}$/.test(p)) return p.toUpperCase();
    return SW_ID.exec(f.name)?.[1]?.toUpperCase() ?? null;
  }
  function versionOf(f: FileDetail): string | null {
    const ex = (f.probe?.extra ?? {}) as Record<string, unknown>;
    if (typeof ex.version === "string") return ex.version;
    return /\[v(\d+)\]/i.exec(f.name)?.[1] ?? null;
  }
  type Kind = { label: string; tone: string; order: number };
  function kindOf(f: FileDetail): Kind {
    const tid = tidOf(f);
    if (tid && d?.row.system_id === "switch") {
      const low = parseInt(tid.slice(-4), 16) & 0x1fff;
      if (low === 0) return { label: "Base game", tone: "ok", order: 0 };
      if (low === 0x800) return { label: "Update", tone: "info", order: 1 };
      if (low & 0x1000) return { label: `DLC ${low & 0xfff}`, tone: "info", order: 2 + (low & 0xfff) };
    }
    const disc = /\(Disc (\d+)\)/i.exec(f.name)?.[1];
    if (disc) return { label: `Disc ${disc}`, tone: "ok", order: +disc };
    if (/^(cue|m3u)$/i.test(f.ext)) return { label: f.ext.toUpperCase(), tone: "plain", order: 50 };
    return { label: formatOf(f), tone: /^(rar|zip|7z)$/i.test(f.ext) ? "plain" : "ok", order: 100 };
  }
  function formatOf(f: FileDetail): string {
    const ex = (f.probe?.extra ?? {}) as Record<string, unknown>;
    const c = typeof ex.container === "string" ? ex.container : null;
    if (c && c !== f.ext && c !== "archive") return `${c.toUpperCase()} (named .${f.ext})`;
    return f.ext.toUpperCase();
  }
  const files = $derived.by(() => {
    if (!d) return [];
    const rows = collapse(d.files, (f) => f.path, (f) => f.size).map((r) => {
      const f = r.items[0];
      const ex = (f.probe?.extra ?? {}) as Record<string, any>;
      const inside = (ex.unpacked?.members ?? ex.members ?? []) as { name: string }[];
      return { ...r, f, kind: kindOf(f), version: versionOf(f), place: placeOf(f.path, roots), inside: inside.length };
    });
    return rows.sort((a, b) => a.kind.order - b.kind.order || a.name.localeCompare(b.name));
  });
  // the game's folder: the one all its files share, else the system folder
  const gameFolder = $derived.by(() => {
    if (!d?.files.length) return null;
    const dirs = [...new Set(d.files.map((f) => f.path.slice(0, Math.max(f.path.lastIndexOf("/"), f.path.lastIndexOf("\\")))))];
    if (dirs.length === 1) return dirs[0];
    const root = roots.find((r) => d!.files[0].path.startsWith(r));
    return root ? `${root.replace(/[\\/]+$/, "")}/${d.files[0].path.slice(root.replace(/[\\/]+$/, "").length + 1).split(/[\\/]/)[0]}` : dirs[0];
  });
  const where = $derived(gameFolder ? placeOf(gameFolder + "/x", roots) : "");

  // ---- doing things ----
  async function hash() { try { await api.hash({ kind: "Games", value: [id] }); app.toast("Fingerprinting — progress is in the jobs bar"); } catch (e) { app.fail(e); } }
  async function chooseAgain() {
    try { await api.chooseAgain(id); app.toast("Looking it up — choose the right one in Match review when it's done"); } catch (e) { app.fail(e); }
  }
  async function lookup() { try { await api.matchMetadata([id], true); app.toast("Looking up game info — progress is in the jobs bar"); } catch (e) { app.fail(e); } }
  async function reveal(p: string) { try { await api.reveal(p); } catch (e) { app.fail(e); } }
  async function copy(text: string, what: string) {
    try { await navigator.clipboard.writeText(text); app.toast(`Copied ${what}`); } catch { app.toast("Couldn't reach the clipboard", "error"); }
  }
  async function refreshArt() {
    try {
      const a = await api.fetchGameArt(id, true);
      extraArt = a;
      if (!a.hero && !a.logo && !a.banner) app.toast("No wide artwork or logo found for this game", "error");
    } catch (e) { app.fail(e); }
  }
  async function savePlay(patch: Partial<PlayInfo>) {
    const next = { ...play, ...patch };
    play = next;
    try { play = await api.setPlay(id, next); app.refresh(); app.loadCollections(); } catch (e) { app.fail(e); }
  }
  const setStatus = (s: PlayStatus) => savePlay({ status: play.status === s ? null : s });
  const setScore = (n: number) => savePlay({ score: play.score === n ? null : n });
  async function addToColl(v: string) {
    addColl = "";
    if (!v) return;
    try {
      if (v === "__new") {
        const name = d?.row.title ? `${d.row.title.split(/[:\-–]/)[0].trim()} series` : "New collection";
        const cid = await api.createCollection(name, [id]);
        await app.loadCollections(); app.openCollection(cid); return;
      }
      await api.addToCollection(v, [id]); app.loadCollections(); app.refresh();
    } catch (e) { app.fail(e); }
  }
  const toggleHash = (fid: number) => { const s = new Set(openHashes); s.has(fid) ? s.delete(fid) : s.add(fid); openHashes = s; };
</script>

<aside class="detail">
  {#if d}
    <header class="hero" class:blurred class:console={consolePhoto} style:--maker={maker?.color}>
      {#if backdrop}<img class="bg" src={src(backdrop)} alt="" crossorigin="anonymous" onerror={fail(backdrop)} />{/if}
      <div class="shade"></div>
      <div class="hero-top">
        <span class="sys"><span class="dot"></span>{app.systemName(d.row.system_id)}</span>
        <button class="close" aria-label="Close details" title="Close (Esc)" onclick={() => (app.focusId = null)}><Icon name="close" size={15} /></button>
      </div>
      <div class="hero-bottom">
        {#if cover}<img class="cover" src={src(cover)} alt="Cover of {d.row.title}" crossorigin="anonymous" onerror={fail(cover)} />{/if}
        {#if logo}<img class="logo" src={src(logo)} alt="" crossorigin="anonymous" onerror={fail(logo)} />{/if}
      </div>
    </header>

    <div class="body">
      <div class="titles">
        <h2 class="selectable">{d.row.title}</h2>
        <p class="line">
          {#if d.row.regions.length}<span>{d.row.regions.join(", ")}</span>{/if}
          {#if niceDate(meta?.release)}<span>{niceDate(meta?.release)}</span>{/if}
          {#if meta?.developer}<span>{meta.developer}</span>{/if}
          {#if d.parsed?.rev}<span>Rev {d.parsed.rev}</span>{/if}
        </p>
        {#if d.canonical && d.canonical !== d.row.title}<p class="faint small selectable">DAT name: {d.canonical}</p>{/if}
        <div class="chips">
          <StatusChips status={d.row.status} dat={d.row.dat} />
          {#each d.row.tags as t}<span class="chip plain">{t}</span>{/each}
        </div>
      </div>

      <!-- the main things to do with this game -->
      <div class="toolbar">
        {#if gameFolder}<button class="tool main" onclick={() => reveal(gameFolder!)} title={gameFolder}><b>Open folder</b><span>in your file manager</span></button>{/if}
        {#if d.row.meta_state === "review"}<button class="tool" onclick={() => (app.view = "review")}><b>Pick a match</b><span>several were found</span></button>
        {:else}<button class="tool" onclick={lookup}><b>{meta ? "Refresh info" : "Get game info"}</b><span>names, details, art</span></button>{/if}
        {#if d.files.length}
          <button class="tool" onclick={() => (renaming = true)}><b>Clean up names</b><span>preview first</span></button>
          <button class="tool" onclick={hash} title="Compute CRC32, SHA-1 and MD5 for every file"><b>Fingerprint</b><span>checksums for every file</span></button>
        {/if}
        <button class="tool" onclick={() => copy(d!.row.title, "the title")}><b>Copy title</b><span>to the clipboard</span></button>
        <button class="tool" onclick={refreshArt}><b>More artwork</b><span>banner and logo</span></button>
      </div>
      {#if d.actions.length}
        <div class="acts">
          {#each d.actions as a}
            <button class="act" onclick={() => (running = { action: a.action, ids: a.file_ids })}>
              <b>{a.action.name}</b><span>{a.file_ids.length} file{a.file_ids.length === 1 ? "" : "s"}</span>
            </button>
          {/each}
        </div>
      {/if}

      <section class="copies">
        <div class="files-head">
          <h3>Your copies {#if d.copies.length}<span class="faint">{d.copies.length}</span>{/if}</h3>
          <button class="btn small" onclick={() => (adding = true)}>Add copy</button>
        </div>
        {#if !d.copies.length}<p class="faint small">Not on your shelf. Add a copy if you own this game physically.</p>{/if}
        <ul class="shelf">
          {#each d.copies as c (c.id)}
            <li>
              {#if editing?.id === c.id}
                <div class="editing">
                  <div class="crel">{c.release}</div>
                  <CopyForm bind:fields={editing.fields} />
                  <div class="cbtns">
                    <button class="btn small ghost danger" onclick={() => removeCopy(c)}>{removing === c.id ? "Click again to remove" : "Remove copy"}</button>
                    <span class="spacer"></span>
                    <button class="btn small" onclick={() => (editing = null)}>Cancel</button>
                    <button class="btn small primary" onclick={saveCopy}>Save</button>
                  </div>
                </div>
              {:else}
                <button class="copy" onclick={() => { editing = { id: c.id, fields: fieldsOf(c) }; removing = null; }} title="Edit this copy">
                  <span class="chip plain">{partsLabel(c)}</span>
                  <span class="cbody">
                    <span class="crel">{c.release}{c.edition ? ` · ${c.edition}` : ""}</span>
                    <span class="cmeta">
                      {#if c.condition}<span>{conditionLabel(c.condition)}</span>{/if}
                      {#if c.location}<span>{c.location}</span>{/if}
                      {#if c.price_cents != null}<span>{money(c.price_cents, app.settings?.currency)}{c.bought ? ` · ${c.bought}` : ""}</span>{:else if c.bought}<span>{c.bought}</span>{/if}
                    </span>
                    {#if c.notes}<span class="cnote">{c.notes}</span>{/if}
                  </span>
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      </section>

      <section class="play">
        <h3>Your play <button class="fav" class:on={play.favorite} title={play.favorite ? "Remove from favourites" : "Mark as a favourite"} aria-pressed={play.favorite} onclick={() => savePlay({ favorite: !play.favorite })}><Icon name={play.favorite ? "star-filled" : "star"} size={16} /></button></h3>
        <div class="form">
          <span class="lbl" id="st-lbl">Status</span>
          <div class="statuses" role="group" aria-labelledby="st-lbl">
            {#each PLAY as p}<button class="st t-{p.tone}" class:on={play.status === p.id} aria-pressed={play.status === p.id} title={p.label} onclick={() => setStatus(p.id)}>{p.short}</button>{/each}
          </div>

          <span class="lbl" id="sc-lbl">Score</span>
          <div class="scorerow">
            <div class="scale" role="group" aria-labelledby="sc-lbl">
              {#each [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as n}<button class="pip" class:lit={(play.score ?? 0) >= n} class:at={play.score === n} aria-pressed={play.score === n} title={play.score === n ? "Clear the score" : `${n}/10`} onclick={() => setScore(n)}>{n}</button>{/each}
            </div>
            <span class="scoreval">{play.score ? `${play.score}/10` : "—"}</span>
          </div>

          <span class="lbl low">Played</span>
          <div class="dates">
            <label><span>Started</span><input type="date" value={play.started ?? ""} onchange={(e) => savePlay({ started: (e.currentTarget as HTMLInputElement).value || null })} /></label>
            <label><span>Finished</span><input type="date" value={play.finished ?? ""} onchange={(e) => savePlay({ finished: (e.currentTarget as HTMLInputElement).value || null })} /></label>
            <label><span>Hours</span><input type="number" min="0" step="0.5" value={play.hours ?? ""} onchange={(e) => { const v = (e.currentTarget as HTMLInputElement).value; savePlay({ hours: v ? Number(v) : null }); }} /></label>
          </div>

          <label class="lbl" for="rv">Notes</label>
          <textarea id="rv" class="review" rows="3" placeholder="What you thought, where you left off…" bind:value={review} onblur={() => { if ((review.trim() || null) !== (play.notes ?? null)) savePlay({ notes: review.trim() || null }); }}></textarea>

          <span class="lbl">Collections</span>
          <div class="incoll">
            {#each d.collections as [cid, cname, pos] (cid)}<button class="cchip" onclick={() => app.openCollection(cid)} title="#{pos + 1} in {cname}">{cname} <span>#{pos + 1}</span></button>{/each}
            <select value={addColl} onchange={(e) => addToColl((e.currentTarget as HTMLSelectElement).value)} aria-label="Add to a collection">
              <option value="">{d.collections.length ? "Add to another…" : "Add to a collection…"}</option>
              {#each app.collections.filter((c) => !d!.collections.some(([i]) => i === c.id)) as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
              <option value="__new">New collection with this game</option>
            </select>
          </div>
        </div>
      </section>

      {#if meta}
        <section>
          <h3>About</h3>
          {#if meta.description}
            <p class="desc selectable" class:clamp={!longDesc}>{meta.description}</p>
            {#if meta.description.length > 320}<button class="more-link" onclick={() => (longDesc = !longDesc)}>{longDesc ? "Show less" : "Read more"}</button>{/if}
          {/if}
          <dl>
            {#if niceDate(meta.release)}<dt>Released</dt><dd>{niceDate(meta.release)}</dd>{/if}
            {#if meta.developer}<dt>Developer</dt><dd>{meta.developer}</dd>{/if}
            {#if meta.publisher}<dt>Publisher</dt><dd>{meta.publisher}</dd>{/if}
            {#if meta.genres.length}<dt>Genre</dt><dd>{meta.genres.join(", ")}</dd>{/if}
            {#if d.parsed?.title_id}<dt>Title ID</dt><dd class="mono selectable">{d.parsed.title_id}</dd>{/if}
            {#if d.parsed?.serial}<dt>Serial</dt><dd class="mono selectable">{d.parsed.serial}</dd>{/if}
            {#if meta.console === "match"}<dt>Console</dt><dd><span class="gpill ok">{app.systemName(d.row.system_id)}</span></dd>
            {:else if meta.console === "other"}<dt>Console</dt><dd><span class="gpill bad">Matched a game for {(meta.platforms ?? []).join(", ")}</span> <button class="more-link" onclick={chooseAgain}>Find the {app.systemName(d.row.system_id)} one</button></dd>
            {:else}<dt>Console</dt><dd class="faint">Not checked{app.settings?.keys?.igdb_client_id || app.settings?.keys?.rawg ? "" : " — set up IGDB or RAWG to check"}</dd>{/if}
            <dt>Info from</dt><dd>{sourceName[meta.source] ?? meta.source}{#if d.row.system_id === "switch" && meta.source === "steamgriddb" && meta.description} (art), Switch title database (details){/if}{#if meta.chosen}<span class="faint"> · picked by you</span>{/if}
              <button class="more-link" onclick={chooseAgain} title="Look the game up afresh and pick the right match in Match review">Wrong match? Choose again</button></dd>
          </dl>
        </section>
      {:else if d.row.meta_state === "review"}
        <section class="note">Several possible matches were found for this game. <button class="more-link" onclick={() => (app.view = "review")}>Pick the right one</button></section>
      {:else}
        <section class="note">No game info yet. <button class="more-link" onclick={lookup}>Look it up</button></section>
      {/if}

      {#if d.files.length}
      <section>
        <div class="files-head">
          <h3>Files <span class="faint">{d.files.length} · {bytes(totalSize)}</span></h3>
          {#if where}<span class="faint small">in {where}</span>{/if}
        </div>
        <ul class="files">
          {#each files as r (r.key)}
            <li>
              <div class="frow">
                <span class="gpill {r.kind.tone}">{r.kind.label}</span>
                <div class="fbody">
                  <div class="fname selectable">{r.name}</div>
                  <div class="fmeta">
                    <span>{bytes(r.size)}</span>
                    {#if r.kind.label !== formatOf(r.f)}<span>{formatOf(r.f)}</span>{/if}
                    {#if r.parts > 1}<span>{r.parts} parts</span>{/if}
                    {#if r.version && r.version !== "0"}<span>v{r.version}</span>{/if}
                    {#if r.inside}<span>{r.inside} file{r.inside === 1 ? "" : "s"} inside</span>{/if}
                    {#if r.f.probe?.encrypted === true}<span class="warn">encrypted</span>{:else if r.f.probe?.encrypted === false}<span class="good">decrypted</span>{/if}
                    {#if r.f.dat_status === "verified"}<span class="good">verified</span>{:else if r.f.dat_status === "bad"}<span class="bad">bad dump</span>{/if}
                  </div>
                  {#if r.place !== where}<div class="fmeta"><span>in {r.place}</span></div>{/if}
                </div>
              </div>
              <div class="fbtns">
                <button class="btn small ghost" onclick={() => reveal(r.f.path)}>Show in folder</button>
                <button class="btn small ghost" onclick={() => copy(r.f.path, "the path")}>Copy path</button>
                {#if r.f.crc32 || r.f.sha1}<button class="btn small ghost" onclick={() => toggleHash(r.f.id)}>{openHashes.has(r.f.id) ? "Hide checksums" : "Checksums"}</button>{/if}
              </div>
              {#if openHashes.has(r.f.id)}
                <dl class="hashes selectable">
                  {#if r.f.crc32}<dt>CRC32</dt><dd>{r.f.crc32}</dd>{/if}
                  {#if r.f.sha1}<dt>SHA-1</dt><dd>{r.f.sha1}</dd>{/if}
                  {#if r.f.probe?.id}<dt>Header ID</dt><dd>{r.f.probe.id}</dd>{/if}
                </dl>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
      {/if}

    </div>
  {:else}
    <p class="faint pad">Loading…</p>
  {/if}
</aside>

{#if renaming && d}<RenameModal gameIds={[d.row.id]} onclose={() => (renaming = false)} />{/if}
{#if running}<ActionConfirm action={running.action} fileIds={running.ids} onclose={() => (running = null)} />{/if}
{#if adding && d}<AddToShelf system={d.row.system_id} release={likelyRelease ?? d.copies[0]?.release ?? null} onclose={() => (adding = false)} />{/if}

<style>
  .detail { position: relative; height: 100%; overflow-y: auto; border-left: 1px solid var(--seam); background: var(--panel); }

  /* ---- artwork header ---- */
  .hero { position: relative; height: 230px; overflow: hidden; background: #1c1c22; isolation: isolate; }
  .bg { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: center 30%; z-index: -2; }
  .hero.blurred .bg { filter: blur(18px) saturate(1.2) brightness(.7); transform: scale(1.25); }
  .hero.console .bg { filter: brightness(.5) saturate(.9); object-position: center 45%; }
  .shade { position: absolute; inset: 0; z-index: -1; background:
    linear-gradient(180deg, #00000070 0%, #00000000 38%, #00000000 55%, color-mix(in srgb, var(--panel) 85%, transparent) 88%, var(--panel) 100%); }
  .hero-top { display: flex; align-items: center; justify-content: space-between; padding: 10px 10px 0 12px; }
  .sys { display: inline-flex; align-items: center; gap: 7px; padding: 3px 10px 3px 8px; border-radius: 12px; background: #00000080; color: #fff; font-size: 13px; font-weight: 600; backdrop-filter: blur(6px); }
  .sys .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--maker); box-shadow: 0 0 0 2px #ffffff30; }
  .close { display: grid; place-items: center; width: 30px; height: 30px; border: 0; border-radius: 50%; background: #00000080; color: #fff; cursor: pointer; font-size: 14px; backdrop-filter: blur(6px); }
  .close:hover { background: #000000b0; }
  .hero-bottom { position: absolute; left: 16px; right: 16px; bottom: 10px; display: flex; align-items: flex-end; gap: 14px; }
  .cover { width: 92px; max-height: 136px; object-fit: cover; border-radius: var(--radius-s); box-shadow: 0 6px 18px #00000070, 0 0 0 1px #ffffff20; flex: none; }
  .logo { max-width: 60%; max-height: 74px; object-fit: contain; object-position: left bottom; margin-left: auto; filter: drop-shadow(0 2px 6px #000000a0); }

  /* ---- your copies ---- */
  .shelf { list-style: none; margin: 6px 0 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .copy { width: 100%; display: flex; gap: 10px; align-items: flex-start; text-align: left; border: 1px solid transparent; background: none; padding: 6px 8px; margin: 0 -8px; border-radius: var(--radius-s); cursor: pointer; color: var(--ink); }
  .copy:hover { background: var(--panel-2); border-color: var(--seam); }
  .copy .chip { flex: none; min-width: 64px; }
  .cbody { display: flex; flex-direction: column; min-width: 0; gap: 1px; }
  .crel { font-weight: 600; overflow-wrap: anywhere; }
  .cmeta { display: flex; flex-wrap: wrap; gap: 2px 12px; font-size: 13px; color: var(--ink-soft); }
  .cnote { font-size: 13px; color: var(--ink-faint); font-style: italic; }
  .editing { display: flex; flex-direction: column; gap: 10px; padding: 10px; border: 1px solid var(--seam); border-radius: var(--radius-s); background: var(--panel-2); }
  .cbtns { display: flex; gap: 8px; align-items: center; }
  .cbtns .spacer { flex: 1; }
  .btn.danger { color: var(--bad); }

  /* ---- body ---- */
  .body { padding: 12px 18px 30px; display: flex; flex-direction: column; gap: 16px; }
  h2 { font-size: 22px; line-height: 1.15; margin: 0; }
  .line { display: flex; flex-wrap: wrap; gap: 2px 0; margin: 4px 0 0; color: var(--ink-soft); font-size: 14px; }
  .line span + span::before { content: "·"; margin: 0 7px; color: var(--ink-faint); }
  .small { font-size: 13px; }
  .titles p { margin-top: 4px; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 8px; }
  .chips:empty { display: none; }
  .toolbar { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .tool { display: flex; flex-direction: column; align-items: flex-start; gap: 0; padding: 6px 10px; border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s); background: var(--panel); cursor: pointer; text-align: left; min-width: 0; }
  .tool:hover { background: var(--panel-2); }
  .tool b { font-size: 13.5px; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; }
  .tool span { font-size: 11.5px; color: var(--ink-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; }
  .tool.main { background: var(--accent); border-color: var(--accent); border-bottom-color: color-mix(in srgb, var(--accent) 70%, #000); }
  .tool.main b, .tool.main span { color: var(--accent-ink); }
  .tool.main span { opacity: .8; }
  .tool.main:hover { background: color-mix(in srgb, var(--accent) 88%, #000); }
  .acts { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 8px; }
  .act { display: flex; flex-direction: column; align-items: flex-start; gap: 1px; padding: 8px 12px; border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s); background: var(--panel); cursor: pointer; text-align: left; }
  .act:hover { background: var(--panel-2); }
  .act b::before { content: ""; display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--accent); margin-right: 7px; vertical-align: 1px; }
  .act b { color: var(--ink); }
  .act span { font-size: 12.5px; color: var(--ink-soft); }
  section h3 { margin: 0 0 8px; color: var(--ink-soft); font-size: 13.5px; font-weight: 600; }
  section h3 .faint { font-weight: 500; margin-left: 4px; }
  .desc { line-height: 1.5; margin: 0; }
  .desc.clamp { display: -webkit-box; -webkit-line-clamp: 5; line-clamp: 5; -webkit-box-orient: vertical; overflow: hidden; }
  .more-link { padding: 4px 0 0; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 14px; margin: 10px 0 0; font-size: 14px; }
  dt { color: var(--ink-faint); } dd { margin: 0; overflow-wrap: anywhere; }
  .mono, .hashes dd { font-family: ui-monospace, "JetBrains Mono", monospace; font-size: 12.5px; }
  .note { color: var(--ink-soft); padding: 10px 12px; border: 1px dashed var(--seam); border-radius: var(--radius-m); }
  .note .more-link { padding: 0; }

  /* ---- your play ---- */
  .play h3 { display: flex; align-items: center; gap: 6px; }
  .fav { border: 0; background: none; cursor: pointer; font-size: 17px; line-height: 1; color: var(--ink-faint); padding: 0 2px; }
  .fav.on { color: var(--signal); }
  /* one form: labels in a column, every control on the same edge */
  .form { display: grid; grid-template-columns: 76px minmax(0, 1fr); gap: 12px 12px; align-items: start; margin-top: 4px; }
  .lbl { padding-top: 6px; font-size: 13px; color: var(--ink-soft); }
  /* level with the inputs, below their small labels */
  .lbl.low { padding-top: 25px; }
  /* status keys, an even grid: the chosen one is pressed in and its lamp is lit */
  .statuses { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 5px; }
  .st { display: inline-flex; align-items: center; justify-content: center; gap: 6px; min-width: 0; height: 28px; padding: 0 6px; border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s);
    background: var(--panel); font: inherit; font-size: 13px; font-weight: 500; color: var(--ink-soft); white-space: nowrap; cursor: pointer; }
  .st::before { content: ""; flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--seam); }
  .st:hover { color: var(--ink); background: var(--panel-2); }
  .st.on { background: var(--panel-2); color: var(--ink); font-weight: 600; box-shadow: inset 0 1px 2px color-mix(in srgb, var(--ink) 14%, transparent); }
  .st.on.t-ok::before { background: var(--ok); } .st.on.t-accent::before { background: var(--accent); } .st.on.t-info::before { background: var(--info); }
  .st.on.t-warn::before { background: var(--warn); } .st.on.t-bad::before { background: var(--bad); } .st.on.t-plain::before { background: var(--ink-soft); }
  /* the score: one joined scale of ten keys; the ones up to your score are tinted, yours is lit */
  .scorerow { display: flex; align-items: center; gap: 10px; }
  .scale { flex: 1; display: grid; grid-template-columns: repeat(10, minmax(0, 1fr)); border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s); overflow: hidden; }
  .pip { height: 28px; padding: 0; border: 0; border-left: 1px solid var(--seam); background: var(--panel); font: inherit; font-size: 12.5px; font-weight: 600; color: var(--ink-faint); cursor: pointer; font-variant-numeric: tabular-nums; }
  .pip:first-child { border-left: 0; }
  .pip:hover { background: var(--panel-2); color: var(--ink); }
  .pip.lit { background: color-mix(in srgb, var(--signal) 18%, var(--panel)); color: var(--ink-soft); }
  .pip.at { background: var(--signal); color: #1c1c1a; }
  .pip:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .scoreval { min-width: 38px; text-align: right; font-size: 13px; font-weight: 600; color: var(--ink-soft); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .dates { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 72px; gap: 8px; }
  .dates label { display: flex; flex-direction: column; gap: 3px; min-width: 0; font-size: 12px; color: var(--ink-faint); }
  .dates input { width: 100%; min-width: 0; }
  .review { width: 100%; resize: vertical; }
  .incoll { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-height: 30px; }
  .cchip { border: 1px solid var(--seam); background: var(--panel-2); border-radius: var(--radius-s); padding: 3px 9px; font: inherit; font-size: 13px; font-weight: 600; color: var(--ink); cursor: pointer; }
  .cchip span { color: var(--ink-faint); font-weight: 500; }
  .incoll select { flex: 1; min-width: 160px; max-width: 240px; font-size: 13px; }

  /* ---- files ---- */
  .files-head { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; flex-wrap: wrap; }
  .files-head h3 { margin-bottom: 6px; }
  .files-head .small { overflow-wrap: anywhere; }
  .files { list-style: none; margin: 0; padding: 0; border: 1px solid var(--seam); border-radius: var(--radius-m); overflow: hidden; }
  .files > li { padding: 9px 12px 6px; background: var(--panel); }
  .files > li + li { border-top: 1px solid color-mix(in srgb, var(--seam) 60%, transparent); }
  .files > li:nth-child(even) { background: color-mix(in srgb, var(--panel-2) 45%, var(--panel)); }
  .frow { display: flex; gap: 10px; align-items: flex-start; }
  .frow .gpill { width: 78px; }
  .fbody { flex: 1; min-width: 0; }
  .fname { font-weight: 600; overflow-wrap: anywhere; line-height: 1.3; }
  .fmeta { display: flex; flex-wrap: wrap; gap: 0 12px; font-size: 12.5px; color: var(--ink-faint); margin-top: 2px; overflow-wrap: anywhere; }
  .fmeta .good { color: var(--ok); } .fmeta .warn { color: var(--warn); } .fmeta .bad { color: var(--bad); }
  .fbtns { display: flex; gap: 2px; margin: 4px 0 0 82px; flex-wrap: wrap; }
  .fbtns .btn { height: 22px; padding: 0 6px; font-size: 12.5px; color: var(--ink-soft); }
  .fbtns .btn:hover { color: var(--ink); }
  .hashes { margin: 6px 0 4px 88px; font-size: 12.5px; }
  .artnote { margin: 0; }
  .pad { padding: 18px; }
</style>
