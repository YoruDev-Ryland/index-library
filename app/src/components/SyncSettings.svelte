<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { RootMap, SyncStatus } from "../lib/types";

  let st = $state<SyncStatus | null>(null);
  let url = $state("");
  let password = $state("");
  let busy = $state(false);
  /** the server's library folders, to be found on this PC (a restore) */
  let mapping = $state<RootMap[] | null>(null);

  const load = async () => { try { st = await api.syncStatus(); if (!url && st.url) url = st.url; } catch (e) { app.fail(e); } };
  onMount(load);
  // quicker while a sync is under way, so its progress moves
  let timer = setInterval(load, 1500);
  onDestroy(() => clearInterval(timer));

  /** Plain http to an address outside the home network sends the password and keys unencrypted. */
  const exposed = $derived.by(() => {
    const u = url.trim();
    if (!u || /^https:\/\//i.test(u)) return false;
    const host = u.replace(/^[a-z]+:\/\//i, "").split(/[/:]/)[0].toLowerCase();
    if (!host) return false;
    const m = host.match(/^(\d+)\.(\d+)\.\d+\.\d+$/);
    if (m) {
      const [a, b] = [Number(m[1]), Number(m[2])];
      return !(a === 10 || a === 127 || (a === 192 && b === 168) || (a === 172 && b >= 16 && b <= 31) || (a === 100 && b >= 64 && b <= 127));
    }
    return !(host === "localhost" || host.endsWith(".local") || host.endsWith(".lan") || host.endsWith(".ts.net") || !host.includes("."));
  });

  async function connect() {
    busy = true;
    try {
      const offer = await api.syncConnect(url, password);
      password = "";
      if (offer) mapping = prefill(offer);
      else app.toast("Connected — this library is being uploaded in the background");
      await load();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function now() {
    try { await api.syncNow(); await load(); } catch (e) { app.fail(e); await load(); }
  }
  async function disconnect() {
    busy = true;
    try { await api.syncDisconnect(); mapping = null; await load(); } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function choose(i: number) {
    const p = await open({ directory: true, multiple: false });
    if (typeof p === "string" && mapping) mapping[i].path = p;
  }
  async function restore() {
    if (!mapping) return;
    busy = true;
    try {
      await api.syncRestore($state.snapshot(mapping));
      mapping = null;
      app.toast("Restoring from the server — progress is in the jobs bar");
      app.settings = await api.settings();
      await load();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  /** A folder this PC already has is the likely place for the server's only one. */
  const prefill = (offer: RootMap[]) => offer.map((r) => ({ ...r, path: offer.length === 1 && app.settings?.library_roots.length === 1 ? app.settings.library_roots[0] : "" }));
  // connected and waiting for folders, e.g. after the app was closed mid-setup
  $effect(() => { if (st?.restore_pending && st.offer.length && !mapping) mapping = prefill(st.offer); });

  /** "12 play statuses, 3 matches": what moved, in words */
  const NOUN: Record<string, [string, string]> = {
    Game: ["game", "games"], Meta: ["match", "matches"], Fingerprint: ["fingerprint", "fingerprints"], Skip: ["skipped suggestion", "skipped suggestions"],
    Play: ["play status", "play statuses"], Collection: ["collection", "collections"], CollectionItems: ["collection's games", "collections' games"],
    Op: ["history entry", "history entries"], Settings: ["settings", "settings"],
  };
  const words = (m: Record<string, number>) => Object.entries(m).map(([k, n]) => `${n.toLocaleString()} ${(NOUN[k] ?? [k, k])[n === 1 ? 0 : 1]}`).join(", ");
  const ago = (ms: number) => {
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    return s < 60 ? "just now" : s < 3600 ? `${Math.round(s / 60)} min ago` : new Date(ms).toLocaleString();
  };
</script>

{#if st}
  {#if !st.connected}
    <div class="set">
      <div class="lab"><b>Server</b><span>The address of your Index sync server, such as http://192.168.1.10:8095 or https://index.example.com.</span></div>
      <div class="ctl stack">
        <input type="text" aria-label="Sync server address" placeholder="http://server:8095" bind:value={url} />
        <input type="password" aria-label="Sync server password" placeholder="Password" bind:value={password} onkeydown={(e) => { if (e.key === "Enter" && url && password) connect(); }} />
        {#if exposed}<p class="note">This address isn't on your home network and doesn't use https: the password and your API keys would cross the internet unencrypted.</p>{/if}
        <div><button class="btn small primary" disabled={busy || !url.trim() || !password} onclick={connect}>{busy ? "Connecting…" : "Connect"}</button></div>
      </div>
    </div>
  {:else}
    <div class="set">
      <div class="lab"><b>Server</b><span>Connected as this PC. Changes sync shortly after you make them, and every few minutes.</span></div>
      <div class="ctl stack">
        <code class="addr">{st.url}</code>
        <div class="status" aria-live="polite">
          {#if st.running}
            <p><span class="gpill info">Syncing</span> {st.running.message}</p>
            {#if st.running.total}<span class="meter" aria-hidden="true"><span style:width="{Math.min(100, (st.running.done / st.running.total) * 100)}%"></span></span>{/if}
          {:else if st.error}<p><span class="gpill warn">Sync failed {st.last_at ? ago(st.last_at) : ""}</span> {st.error}</p>
          {:else if st.last_at && st.pending}<p><span class="gpill warn">{st.pending.toLocaleString()} {st.pending === 1 ? "change" : "changes"} waiting to send</span> checked {ago(st.last_at)}</p>
          {:else if st.last_at}<p><span class="gpill ok">Up to date</span> checked {ago(st.last_at)}</p>
          {:else if !st.restore_pending}<p><span class="gpill">Not synced yet</span></p>{/if}
          {#if st.last?.waiting}<p class="quiet">{st.last.waiting.toLocaleString()} received {st.last.waiting === 1 ? "record waits" : "records wait"} for games not scanned on this PC.</p>{/if}
        </div>
        {#if st.last && Math.abs(st.last.clock_skew_ms) > 300_000}<p class="note">This PC's clock is {Math.round(Math.abs(st.last.clock_skew_ms) / 60000)} minutes {st.last.clock_skew_ms > 0 ? "ahead of" : "behind"} the server's. The newest change wins, so fix the clock to keep changes in the right order.</p>{/if}
        <div class="row">
          <button class="btn small" disabled={busy || st.restore_pending || !!st.running} onclick={now}>{st.running ? "Syncing…" : "Sync now"}</button>
          <button class="btn small ghost" disabled={busy} onclick={disconnect}>Disconnect</button>
        </div>
      </div>
    </div>
  {/if}
  {#if st.connected && st.activity.length}
    <div class="set">
      <div class="lab"><b>Recent activity</b><span>What went to the server from this PC, and what came from your other devices.</span></div>
      <ul class="activity">
        {#each st.activity.slice(0, 12) as a (a.at)}
          <li>
            <span class="when">{ago(a.at)}</span>
            <span class="what">
              {#if a.error}<span class="bad">Failed: {a.error}</span>{/if}
              {#if Object.keys(a.sent).length}<span>Sent {words(a.sent)}{a.covers_up ? `, ${a.covers_up.toLocaleString()} covers` : ""}</span>{/if}
              {#if Object.keys(a.received).length}<span>Received {words(a.received)}{a.covers_down ? `, ${a.covers_down.toLocaleString()} covers` : ""}</span>{/if}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
  {#if mapping}
    <div class="set">
      <div class="lab"><b>Restore this library</b><span>The server already has a library. Choose where each of its library folders is on this PC; matches, fingerprints, your play data and collections come back as the folders are scanned, without fingerprinting again.</span></div>
      <div class="ctl stack">
        {#each mapping as m, i (m.id)}
          <div class="map">
            <span class="origin">{m.origin || "Library folder"}</span>
            <div class="row"><input type="text" aria-label="Where {m.origin} is on this PC" placeholder="Folder on this PC" bind:value={m.path} /><button class="btn small" onclick={() => choose(i)}>Browse…</button></div>
          </div>
        {/each}
        <div><button class="btn small primary" disabled={busy || !mapping.some((m) => m.path.trim())} onclick={restore}>Restore</button></div>
      </div>
    </div>
  {/if}
{/if}

<style>
  .set { display: grid; grid-template-columns: minmax(0, 1fr) minmax(260px, 1.15fr); gap: 8px 28px; align-items: start; padding: 12px 0; }
  .set + .set { border-top: 1px solid color-mix(in srgb, var(--seam) 55%, transparent); }
  .lab { display: flex; flex-direction: column; gap: 3px; padding-top: 5px; }
  .lab b { font-weight: 600; }
  .lab span { font-size: 13px; color: var(--ink-soft); line-height: 1.45; }
  .ctl { min-width: 0; }
  .stack { display: flex; flex-direction: column; gap: 8px; }
  .stack > input { width: 100%; }
  .row { display: flex; gap: 8px; align-items: center; }
  .row input { flex: 1; min-width: 0; }
  .note { margin: 0; font-size: 13px; color: var(--warn); }
  .status { font-size: 13px; color: var(--ink-soft); display: flex; flex-direction: column; gap: 4px; }
  .status p { margin: 0; }
  .status .quiet { color: var(--ink-faint); }
  .meter { display: flex; height: 4px; border-radius: 2px; background: var(--panel-2); overflow: hidden; max-width: 320px; }
  .meter span { background: var(--accent); }
  .activity { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
  .activity li { display: grid; grid-template-columns: 110px minmax(0, 1fr); gap: 10px; }
  .activity .when { color: var(--ink-faint); font-variant-numeric: tabular-nums; }
  .activity .what { display: flex; flex-direction: column; color: var(--ink-soft); }
  .activity .bad { color: var(--bad); }
  .addr { align-self: flex-start; font-size: 12.5px; background: var(--panel-2); padding: 2px 6px; border-radius: 3px; word-break: break-all; }
  .map { display: flex; flex-direction: column; gap: 4px; }
  .origin { font-size: 12.5px; color: var(--ink-soft); word-break: break-all; }
  @media (max-width: 1100px) { .set { grid-template-columns: 1fr; } .lab { padding-top: 0; } }
</style>
