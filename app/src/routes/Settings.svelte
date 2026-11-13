<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import SyncSettings from "../components/SyncSettings.svelte";
  import ShelfImport from "../components/ShelfImport.svelte";
  let importing = $state(false);
  const CURRENCIES = ["USD", "EUR", "GBP", "CAD", "AUD", "JPY", "NZD", "SEK", "NOK", "DKK", "CHF", "PLN", "BRL", "MXN"];
  import type { ActionInfo, Requirement, Settings, SourceStatus } from "../lib/types";
  let s = $state<Settings | null>(null);
  /** what the page started from: only what's changed from it is saved */
  let base: Settings | null = null;
  let path = $state("");
  let reqs = $state<Record<string, Requirement[]>>({});
  let actions = $state<ActionInfo[]>([]);
  let datSystem = $state("");
  let dirty = $state(false);
  let saveState = $state<"" | "saving" | "saved">("");
  let saveTimer: ReturnType<typeof setTimeout>;

  const toolKeys: { key: string; label: string; dir?: boolean }[] = [
    { key: "3ds-decrypt", label: "3DS-Decrypt scripts folder", dir: true },
    { key: "pkg2zip", label: "pkg2zip" }, { key: "maxcso", label: "maxcso" },
    { key: "chdman", label: "chdman" }, { key: "dolphin-tool", label: "dolphin-tool" },
  ];

  async function load() {
    s = structuredClone($state.snapshot(app.settings)) ?? (await api.settings());
    base = structuredClone($state.snapshot(s));
    path = await api.settingsPath();
    actions = await api.actions();
    checkTools();
  }
  async function checkTools() {
    const out: Record<string, Requirement[]> = {};
    for (const a of actions) out[a.id] = await api.requirements(a.id);
    reqs = out;
  }
  onMount(load);

  // which game info sources are in use; refreshed after saving, and when a download ends
  let sources = $state<SourceStatus[] | null>(null);
  const loadSources = () => api.sourcesStatus().then((x) => (sources = x)).catch(() => {});
  onMount(loadSources);
  let titledbJob = $state<number | null>(null);
  async function downloadSwitch() {
    try { titledbJob = await api.downloadSwitchTitledb(); app.toast("Downloading the Switch title database — progress is in the jobs bar"); } catch (e) { app.fail(e); }
  }
  $effect(() => {
    if (titledbJob === null) return;
    const j = app.jobs.find((x) => x.id === titledbJob);
    if (j && !["queued", "running"].includes(j.state)) {
      titledbJob = null;
      if (j.state === "failed") app.toast(`Download failed: ${j.error ?? "unknown error"}`, "error");
      loadSources();
    }
  });

  // Changes save on their own shortly after you stop typing (and when you leave the page).
  async function save() {
    if (!s || !dirty) return;
    clearTimeout(saveTimer);
    saveState = "saving";
    try { const snap = structuredClone($state.snapshot(s)); await api.saveSettings(snap, base ?? undefined); base = structuredClone(snap); app.settings = snap; dirty = false; saveState = "saved"; checkTools(); loadSources(); }
    catch (e) { saveState = ""; app.fail(e); }
  }
  onDestroy(() => { if (dirty) save(); });
  async function pick(dir: boolean): Promise<string | null> {
    const p = await open({ directory: dir, multiple: false });
    return typeof p === "string" ? p : null;
  }
  async function addRoot() { const p = await pick(true); if (p && s) { s.library_roots = [...s.library_roots, p]; mark(); } }
  async function setTool(k: string, dir: boolean) { const p = await pick(dir); if (p && s) { s.tools = { ...s.tools, [k]: p }; mark(); } }
  async function importDat() {
    if (!datSystem) return;
    const p = await open({ filters: [{ name: "DAT", extensions: ["dat", "xml"] }] });
    if (typeof p !== "string") return;
    try { const r = await api.importDat(datSystem, p); const v = await api.verify(datSystem); app.toast(`${r.name}: ${r.games} games. Verified ${v.verified}, bad ${v.bad}, not in DAT ${v.unknown}. Hash the system to verify more.`); app.refresh(); }
    catch (e) { app.fail(e); }
  }
  async function importTitles(kind: string) {
    const p = await open({ filters: [{ name: kind === "psp" ? "TSV" : "JSON", extensions: [kind === "psp" ? "tsv" : "json"] }] });
    if (typeof p !== "string") return;
    try { const n = await api.importTitledb(kind, p); app.toast(`Imported ${n.toLocaleString()} titles`); loadSources(); } catch (e) { app.fail(e); }
  }
  async function chooseSystemImage(id: string) {
    const p = await open({ filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
    if (typeof p !== "string") return;
    try { await api.setSystemImage(id, p); await app.refresh(); } catch (e) { app.fail(e); }
  }
  async function resetSystemImage(id: string) {
    try { await api.setSystemImage(id, null); await api.fetchSystemImages(); await app.refresh(); } catch (e) { app.fail(e); }
  }
  const mark = () => { dirty = true; saveState = ""; clearTimeout(saveTimer); saveTimer = setTimeout(save, 700); };

  function setTheme(t: Settings["theme"]) {
    if (!s) return;
    s.theme = t;
    app.applyTheme(t);
    mark();
  }

  // the section list on the left follows where you are on the page
  const SECTIONS = [
    { id: "appearance", label: "Appearance" }, { id: "library", label: "Library" }, { id: "duplicates", label: "Duplicates" },
    { id: "archive", label: "Scanning & archive" }, { id: "info", label: "Game info" }, { id: "tools", label: "Tools" },
    { id: "verification", label: "Verification" }, { id: "images", label: "System images" }, { id: "shelf", label: "Shelf" }, { id: "sync", label: "Sync" }, { id: "updates", label: "Updates" },
  ];
  let current = $state("appearance");
  let scroller = $state<HTMLElement | null>(null);
  $effect(() => {
    if (!scroller || !s) return;
    const io = new IntersectionObserver((entries) => {
      const vis = entries.filter((e) => e.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
      if (vis[0]) current = vis[0].target.id.replace("s-", "");
    }, { root: scroller, rootMargin: "0px 0px -65% 0px" });
    for (const x of SECTIONS) { const el = document.getElementById(`s-${x.id}`); if (el) io.observe(el); }
    return () => io.disconnect();
  });
  const go = (id: string) => { document.getElementById(`s-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" }); current = id; };
  const keySet = (v: string) => !!v?.trim();
  async function checkConsoles() {
    if (dirty) await save();
    try { await api.checkConsoles(); app.toast("Checking matched games against their consoles — progress is in the jobs bar"); } catch (e) { app.fail(e); }
  }
  async function downloadDats() {
    try { await api.downloadDats(); app.toast("Downloading game lists — progress is in the jobs bar"); } catch (e) { app.fail(e); }
  }
</script>

{#if importing}<ShelfImport onclose={() => (importing = false)} />{/if}
<div class="settings">
  <nav class="side" aria-label="Settings sections">
    <h1>Settings</h1>
    {#each SECTIONS as x (x.id)}<button class:on={current === x.id} aria-current={current === x.id ? "true" : undefined} onclick={() => go(x.id)}>{x.label}</button>{/each}
    <p class="saved" aria-live="polite">{saveState === "saving" ? "Saving…" : saveState === "saved" ? "Saved" : dirty ? "Unsaved changes" : "Changes save automatically"}</p>
  </nav>

  <div class="scroll" bind:this={scroller}>
    {#if s}
    <div class="content">
      <section id="s-appearance">
        <header><h2>Appearance</h2></header>
        <div class="set">
          <div class="lab"><b>Theme</b><span>Follow your system's light or dark setting, or choose one for this PC.</span></div>
          <div class="ctl">
            <div class="seg" role="radiogroup" aria-label="Theme">
              {#each [["system", "System"], ["light", "Light"], ["dark", "Dark"]] as [v, l]}
                <button role="radio" aria-checked={s.theme === v} class:on={s.theme === v} onclick={() => setTheme(v as Settings["theme"])}>{l}</button>
              {/each}
            </div>
          </div>
        </div>
      </section>

      <section id="s-library">
        <header><h2>Library</h2><p>The folders that hold your systems. Each system is a subfolder, such as “Nintendo - Switch”.</p></header>
        <div class="set">
          <div class="lab"><b>Library folders</b><span>Network shares work too.</span></div>
          <div class="ctl stack">
            {#each s.library_roots as r, i}
              <div class="row"><input type="text" bind:value={s.library_roots[i]} oninput={mark} aria-label="Library folder {i + 1}" /><button class="btn small ghost" onclick={() => { s!.library_roots = s!.library_roots.filter((_, j) => j !== i); mark(); }}>Remove</button></div>
            {/each}
            <div><button class="btn small" onclick={addRoot}>Add folder…</button></div>
          </div>
        </div>
        <div class="set">
          <div class="lab"><b>Folders to skip</b><span>One name per line. Tool and emulator folders inside system folders; scans and Cleanup leave them alone. Every system's _Tools folder is always skipped.</span></div>
          <div class="ctl"><textarea rows="5" aria-label="Folders to skip" value={s.ignored.join("\n")} oninput={(e) => { s!.ignored = (e.currentTarget as HTMLTextAreaElement).value.split("\n").map((x) => x.trim()).filter(Boolean); mark(); }}></textarea></div>
        </div>
      </section>

      <section id="s-duplicates">
        <header><h2>Duplicates</h2></header>
        <div class="set">
          <div class="lab"><b>Preferred regions</b><span>Best first, separated by commas. Decides which copy is kept.</span></div>
          <div class="ctl"><input type="text" aria-label="Preferred regions" value={s.region_order.join(", ")} oninput={(e) => { s!.region_order = (e.currentTarget as HTMLInputElement).value.split(",").map((x) => x.trim()).filter(Boolean); mark(); }} /></div>
        </div>
        <div class="set">
          <div class="lab"><b>One copy per game (1G1R)</b><span>When a game is here from several regions, keep the best one and list the others as duplicates. Hacks, translations and BIOS files are always kept.</span></div>
          <div class="ctl"><label class="switch"><input type="checkbox" bind:checked={s.one_game_one_rom} onchange={mark} /><span>{s.one_game_one_rom ? "On" : "Off"}</span></label></div>
        </div>
      </section>

      <section id="s-archive">
        <header><h2>Scanning & archive</h2></header>
        <div class="set">
          <div class="lab"><b>Keep archived files for</b><span>Removed and replaced files wait in each system's _archive folder this long before you're asked to delete them.</span></div>
          <div class="ctl short"><input type="number" min="1" aria-label="Days to keep archived files" bind:value={s.archive_hold_days} oninput={mark} /><span class="unit">days</span></div>
        </div>
        <div class="set">
          <div class="lab"><b>Folders listed at once</b><span>How many folders a scan reads in parallel. Lower it if the NAS struggles.</span></div>
          <div class="ctl short"><input type="number" min="1" max="64" aria-label="Folders listed at once" bind:value={s.scan_concurrency} oninput={mark} /></div>
        </div>
        <div class="set">
          <div class="lab"><b>Working folder</b><span>Where actions make temporary copies. Empty uses the system's temp folder.</span></div>
          <div class="ctl"><input type="text" aria-label="Working folder" value={s.temp_dir ?? ""} placeholder="System temp folder" oninput={(e) => { s!.temp_dir = (e.currentTarget as HTMLInputElement).value || null; mark(); }} /></div>
        </div>
      </section>

      <section id="s-info">
        <header><h2>Game info</h2><p>Where names, details and artwork come from. Title databases match exactly by ID; online sources search by file fingerprint or name.</p></header>
        {#if sources}
          <ul class="sources">
            {#each sources as src (src.id)}
              <li>
                <span class="gpill {src.active ? 'ok' : ''}">{src.name}</span>
                <span class="sdetail">{src.detail}</span>
                {#if src.id === "titledb-switch"}<button class="btn small" class:primary={!src.active} onclick={downloadSwitch}>{src.active ? "Update" : "Download"} · 90 MB</button>{/if}
              </li>
            {/each}
          </ul>
        {/if}
        <div class="set">
          <div class="lab"><b>SteamGridDB</b><span>Covers, banners and logos. Free key from steamgriddb.com → Preferences → API.</span></div>
          <div class="ctl"><input type="password" aria-label="SteamGridDB API key" placeholder="API key" bind:value={s.keys.steamgriddb} oninput={mark} /></div>
        </div>
        <div class="set">
          <div class="lab"><b>IGDB</b><span>Descriptions, dates, genres, and which consoles each game came out on. Free: create an application at dev.twitch.tv/console (category “Application Integration”, any redirect URL such as http://localhost), then copy its client ID and generate a secret.</span></div>
          <div class="ctl stack">
            <input type="text" aria-label="IGDB client ID" placeholder="Client ID" bind:value={s.keys.igdb_client_id} oninput={mark} />
            <input type="password" aria-label="IGDB client secret" placeholder="Client secret" bind:value={s.keys.igdb_client_secret} oninput={mark} />
          </div>
        </div>
        <div class="set">
          <div class="lab"><b>RAWG</b><span>Names, dates, genres, wide artwork, and which consoles each game came out on. Free key: sign up at rawg.io, then rawg.io/apidocs → Get API key.</span></div>
          <div class="ctl"><input type="password" aria-label="RAWG API key" placeholder="API key" bind:value={s.keys.rawg} oninput={mark} /></div>
        </div>
        <div class="set">
          <div class="lab"><b>Check matches against consoles</b><span>SteamGridDB doesn't say which console a game is for. With IGDB or RAWG set up, every match is checked; this checks the games matched before. A match for another console goes back to Match review.</span></div>
          <div class="ctl"><button class="btn small" disabled={!(s.keys.igdb_client_id && s.keys.igdb_client_secret) && !s.keys.rawg} onclick={checkConsoles}>Check consoles</button>{#if !(s.keys.igdb_client_id && s.keys.igdb_client_secret) && !s.keys.rawg}<p class="note quiet">Set up IGDB or RAWG first.</p>{/if}</div>
        </div>
        <div class="set">
          <div class="lab"><b>ScreenScraper</b><span>Exact matches by fingerprint. Needs your account and a developer ID for this app.</span></div>
          <div class="ctl stack">
            <div class="pair"><input type="text" aria-label="ScreenScraper user" placeholder="User" bind:value={s.keys.screenscraper_user} oninput={mark} /><input type="password" aria-label="ScreenScraper password" placeholder="Password" bind:value={s.keys.screenscraper_password} oninput={mark} /></div>
            <div class="pair"><input type="text" aria-label="ScreenScraper developer ID" placeholder="Developer ID" bind:value={s.keys.screenscraper_devid} oninput={mark} /><input type="password" aria-label="ScreenScraper developer password" placeholder="Developer password" bind:value={s.keys.screenscraper_devpassword} oninput={mark} /></div>
            {#if keySet(s.keys.screenscraper_user) && !keySet(s.keys.screenscraper_devid)}<p class="note">Your account alone isn't enough: ScreenScraper refuses requests without a developer ID.</p>{/if}
          </div>
        </div>
        <div class="set">
          <div class="lab"><b>Title lists from files</b><span>Import NoPayStation's PSP list or a titledb JSON you already have.</span></div>
          <div class="ctl row"><button class="btn small" onclick={() => importTitles("psp")}>PSP list (TSV)…</button><button class="btn small" onclick={() => importTitles("switch")}>Switch titledb (JSON)…</button></div>
        </div>
      </section>

      <section id="s-tools">
        <header><h2>Tools</h2><p>Programs that actions run. Leave a path empty to use the one on your PATH.</p></header>
        <div class="set">
          <div class="lab"><b>Python</b></div>
          <div class="ctl"><input type="text" aria-label="Python" bind:value={s.python} oninput={mark} /></div>
        </div>
        {#each toolKeys as t}
          <div class="set">
            <div class="lab"><b>{t.label}</b></div>
            <div class="ctl row"><input type="text" aria-label={t.label} value={s.tools[t.key] ?? ""} placeholder={t.dir ? "<library>/Nintendo - 3DS/3DS-Decrypt" : t.key} oninput={(e) => { const v = (e.currentTarget as HTMLInputElement).value; const n = { ...s!.tools }; if (v) n[t.key] = v; else delete n[t.key]; s!.tools = n; mark(); }} /><button class="btn small" onclick={() => setTool(t.key, !!t.dir)}>Browse…</button></div>
          </div>
        {/each}
        <div class="set">
          <div class="lab"><b>Ready to run</b><span>What each action needs, and whether it was found.</span></div>
          <div class="ctl stack">
            {#each actions as a}
              {@const rs = reqs[a.id] ?? []}
              <div class="act">
                <span class="gpill {rs.length && rs.every((r) => r.ok) ? 'ok' : rs.some((r) => !r.ok) ? 'warn' : ''}">{a.name}</span>
                {#each rs.filter((r) => !r.ok) as r}<span class="miss">Missing {r.name} — {r.hint}</span>{/each}
              </div>
            {/each}
          </div>
        </div>
      </section>

      <section id="s-verification">
        <header><h2>Verification</h2><p>No-Intro and Redump list every known good dump of each game with its official name. With a list and fingerprints, files are verified, bad dumps are flagged, and verified files take their official name when you clean up names.</p></header>
        <div class="set">
          <div class="lab"><b>Game lists from libretro</b><span>Download the No-Intro / Redump lists for every system in your library (open data, no key). Then fingerprint a system to match its files.</span></div>
          <div class="ctl"><button class="btn small primary" onclick={downloadDats}>Download game lists</button></div>
        </div>
        <div class="set">
          <div class="lab"><b>Import a DAT file</b><span>A list you downloaded yourself, for example from datomatic.no-intro.org or redump.org.</span></div>
          <div class="ctl row">
            <select aria-label="System for the DAT" bind:value={datSystem}><option value="">Choose a system</option>{#each app.allSystems as [id, name]}<option value={id}>{name}</option>{/each}</select>
            <button class="btn small" disabled={!datSystem} onclick={importDat}>Choose DAT file…</button>
          </div>
        </div>
      </section>

      <section id="s-images">
        <header><h2>System images</h2><p>The sidebar shows each console's photo from Wikipedia. Pick your own picture for any system.</p></header>
        <div class="sysimgs">
          {#each app.systems as sys (sys.id)}
            {@const img = app.systemImages[sys.id]}
            <div class="sysimg">
              <div class="prev">{#if img}<img src={convertFileSrc(img)} alt="" crossorigin="anonymous" />{/if}<span>{sys.name}</span></div>
              <button class="btn small" onclick={() => chooseSystemImage(sys.id)}>Choose…</button>
              {#if img && img.includes("custom-")}<button class="btn small ghost" onclick={() => resetSystemImage(sys.id)}>Default</button>{/if}
            </div>
          {/each}
        </div>
      </section>

      <section id="s-shelf">
        <header><h2>Shelf</h2><p>Your physical games: one entry per copy you own, with its parts, condition and where it's kept. Add copies from a game's details, from Completion, or with Add to shelf in the library.</p></header>
        <div class="set">
          <div class="lab"><b>Currency</b><span>What the prices you paid are in.</span></div>
          <div class="ctl short">
            <select aria-label="Currency" value={s.currency ?? "USD"} onchange={(e) => { s!.currency = (e.currentTarget as HTMLSelectElement).value; mark(); }}>
              {#each CURRENCIES.includes(s.currency ?? "USD") ? CURRENCIES : [s.currency, ...CURRENCIES] as c}<option value={c}>{c}</option>{/each}
            </select>
          </div>
        </div>
        <div class="set">
          <div class="lab"><b>Import a spreadsheet</b><span>A CSV of your physical games from another tracker or your own sheet, one copy per row. Each row is matched to a release before anything is added.</span></div>
          <div class="ctl"><button class="btn small" onclick={() => (importing = true)}>Import CSV…</button></div>
        </div>
      </section>

      <section id="s-sync">
        <header><h2>Sync</h2><p>Optional. Connect to your own Index sync server to see and edit your library from other devices, and to restore it on a new PC. Everything works the same without one.</p></header>
        <SyncSettings />
      </section>

      <section id="s-updates">
        <header><h2>Updates</h2><p>New versions of Index come from its GitHub releases. They download in the background, and you choose when to restart into them.</p></header>
        <div class="set">
          <div class="lab"><b>Version</b><span>{app.update.status ? `Index ${app.update.status.current}` : "Checking…"}{app.update.status && !app.update.status.can_install ? ". Installed by your system's package manager: new versions are offered as a download." : ""}</span></div>
          <div class="ctl stack">
            <div class="row"><button class="btn small" disabled={app.update.checking || app.update.downloading} onclick={() => app.checkUpdate(true)}>{app.update.checking ? "Checking…" : app.update.downloading ? "Downloading…" : "Check now"}</button></div>
            <p class="note quiet" aria-live="polite">
              {#if app.update.error}Couldn't check: {app.update.error}
              {:else if app.update.status?.ready}Index {app.update.status.ready} is downloaded: restart to update, from the bar at the bottom.
              {:else if app.update.status?.available}Index {app.update.status.available.version} is out.
              {:else if app.update.checkedAt}Up to date, checked {new Date(app.update.checkedAt).toLocaleTimeString()}.{/if}
            </p>
          </div>
        </div>
        <div class="set">
          <div class="lab"><b>Check automatically</b><span>When Index starts and every 6 hours. This PC only.</span></div>
          <div class="ctl"><label class="switch"><input type="checkbox" bind:checked={s.auto_update} onchange={mark} /><span>{s.auto_update ? "On" : "Off"}</span></label></div>
        </div>
      </section>

      <p class="file">Stored on this PC in <code>{path}</code>. API keys stay in this file, and go to your sync server only when one is connected.</p>
    </div>
    {/if}
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 210px minmax(0, 1fr); height: 100%; }
  .side { display: flex; flex-direction: column; gap: 1px; padding: 22px 12px 16px 20px; border-right: 1px solid var(--seam); }
  .side h1 { margin: 0 0 14px 8px; }
  .side button { border: 0; background: none; text-align: left; padding: 6px 8px; border-radius: var(--radius-s); color: var(--ink-soft); font-weight: 500; cursor: pointer; }
  .side button:hover { color: var(--ink); background: var(--panel); }
  .side button.on { color: var(--ink); background: var(--panel); font-weight: 600; box-shadow: inset 0 0 0 1px var(--seam); }
  .saved { margin: auto 8px 0; font-size: 12.5px; color: var(--ink-faint); }
  .scroll { overflow-y: auto; min-height: 0; }
  .content { max-width: 860px; padding: 22px 32px 60px; }
  section { padding: 4px 0 26px; scroll-margin-top: 16px; }
  section + section { border-top: 1px solid var(--seam); padding-top: 22px; }
  section header { margin-bottom: 8px; }
  section h2 { font-size: 17px; }
  section header p { margin: 4px 0 0; color: var(--ink-soft); max-width: 68ch; }
  /* one setting per row: what it is on the left, the control on the right */
  .set { display: grid; grid-template-columns: minmax(0, 1fr) minmax(260px, 1.15fr); gap: 8px 28px; align-items: start; padding: 12px 0; }
  .set + .set { border-top: 1px solid color-mix(in srgb, var(--seam) 55%, transparent); }
  .lab { display: flex; flex-direction: column; gap: 3px; padding-top: 5px; }
  .lab b { font-weight: 600; }
  .lab span { font-size: 13px; color: var(--ink-soft); line-height: 1.45; }
  .ctl { min-width: 0; }
  .ctl > input, .ctl > textarea, .ctl > select { width: 100%; }
  .stack { display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; gap: 8px; align-items: center; }
  .row input, .row select { flex: 1; min-width: 0; }
  .pair { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .short { display: flex; align-items: center; gap: 8px; }
  .short input { width: 96px; }
  .unit { color: var(--ink-soft); }
  .note { margin: 0; font-size: 13px; color: var(--warn); }
  .note.quiet { color: var(--ink-soft); margin-top: 6px; }
  .seg { display: inline-flex; border: 1px solid var(--seam); border-bottom-color: var(--key-edge); border-radius: var(--radius-s); overflow: hidden; }
  .seg button { border: 0; background: var(--panel); padding: 0 16px; height: 30px; cursor: pointer; color: var(--ink-soft); font-weight: 500; }
  .seg button + button { border-left: 1px solid var(--seam); }
  .seg button.on { background: var(--ink); color: var(--plastic); font-weight: 600; }
  .switch { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 500; }
  .switch input { width: 16px; height: 16px; }
  .sources { list-style: none; margin: 6px 0 10px; padding: 10px 14px; display: flex; flex-direction: column; gap: 8px; border: 1px solid var(--seam); border-radius: var(--radius-m); background: var(--panel); }
  .sources li { display: grid; grid-template-columns: 190px minmax(0, 1fr) auto; gap: 12px; align-items: baseline; }
  .sdetail { font-size: 13px; color: var(--ink-soft); }
  .act { display: flex; flex-direction: column; gap: 2px; }
  .miss { font-size: 12.5px; color: var(--ink-soft); padding-left: 13px; }
  .sysimgs { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 8px 14px; }
  .sysimg { display: flex; align-items: center; gap: 8px; }
  .prev { position: relative; flex: 1; height: 32px; border-radius: var(--radius-s); background: #2b2b31; overflow: hidden; display: flex; align-items: center; padding: 0 10px; color: #fff; font-weight: 600; }
  .prev img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; filter: brightness(.45); }
  .prev span { position: relative; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .file { margin-top: 10px; font-size: 13px; color: var(--ink-faint); }
  code { font-size: 12.5px; background: var(--panel-2); padding: 0 4px; border-radius: 3px; word-break: break-all; }
  @media (max-width: 1100px) { .set { grid-template-columns: 1fr; } .lab { padding-top: 0; } }
</style>
