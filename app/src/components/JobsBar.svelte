<script lang="ts">
  import { ask } from "@tauri-apps/plugin-dialog";
  import { openLink } from "../lib/links";
  import Icon from "./Icon.svelte";
  import MarkAnim from "./MarkAnim.svelte";
  import { app } from "../lib/state.svelte";
  import { api } from "../lib/api";
  import type { HashLevel, JobInfo } from "../lib/types";

  let picked = $state<number | null>(null);
  const running = $derived(app.runningJobs);
  const current = $derived(running.find((j) => j.state === "running") ?? running[0]);
  // Follow the running job unless one was picked explicitly.
  const shown = $derived(app.jobs.find((j) => j.id === picked) ?? current ?? app.jobs[0]);
  // The most recent finished job, so a failure stays visible on the bar after it ends.
  const last = $derived(app.jobs.filter((j) => j.state !== "running" && j.state !== "queued").at(-1));
  const finished = $derived(app.jobs.length - running.length);
  let logEl = $state<HTMLPreElement | null>(null);
  let toggleEl = $state<HTMLButtonElement | null>(null);
  let stick = true;
  $effect(() => {
    void shown?.log.length;
    if (logEl && stick) logEl.scrollTop = logEl.scrollHeight;
  });
  const hashing = $derived(app.jobs.some((j) => j.kind === "hash" && (j.state === "running" || j.state === "queued")));
  const levels: { v: HashLevel; label: string }[] = [
    { v: "background", label: "Background" }, { v: "normal", label: "Normal" }, { v: "fast", label: "Fast" }, { v: "maximum", label: "Full speed" },
  ];
  async function setLevel(v: HashLevel) { await api.setHashLevel(v); app.hash = { ...app.hash, level: v }; }
  async function togglePause() { await api.pauseHashing(!app.hash.paused); app.hash = { ...app.hash, paused: !app.hash.paused }; }
  async function clearFinished() { await api.clearJobs(); app.jobs = app.jobs.filter((j) => j.state === "running" || j.state === "queued"); picked = null; }
  const pct = (d: number, t: number) => (t > 0 ? Math.min(100, Math.round((d / t) * 100)) : null);

  const lamp = (j: JobInfo) => ({ running: "info", queued: "", done: "ok", failed: "bad", cancelled: "warn" })[j.state];
  function stateText(j: JobInfo) {
    const p = pct(j.done, j.total);
    switch (j.state) {
      case "running": return p !== null ? `${p}%` : "Running";
      case "queued": return "Waiting";
      case "done": return "Done";
      case "failed": return "Failed";
      case "cancelled": return "Cancelled";
    }
  }

  // panel height: drag the top edge; remembered on this PC
  const MIN = 160;
  let dragging = $state(false);
  let height = $state(load());
  function load() { try { return Math.max(MIN, Number(localStorage.getItem("lo.activityHeight")) || 280); } catch { return 280; } }
  function resize(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const y0 = e.clientY, h0 = height;
    const max = () => Math.max(MIN, window.innerHeight * 0.7);
    const move = (ev: PointerEvent) => { height = Math.round(Math.min(max(), Math.max(MIN, h0 + (y0 - ev.clientY)))); };
    dragging = true;
    const up = () => { dragging = false; el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); try { localStorage.setItem("lo.activityHeight", String(height)); } catch {} };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
  function resizeKey(e: KeyboardEvent) {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    height = Math.min(Math.max(MIN, window.innerHeight * 0.7), Math.max(MIN, height + (e.key === "ArrowUp" ? 24 : -24)));
    try { localStorage.setItem("lo.activityHeight", String(height)); } catch {}
  }
  function close() { app.jobsOpen = false; toggleEl?.focus(); }
  // a new version, downloaded: install it and start again (asking first if work is under way)
  async function restartToUpdate() {
    const busy = app.runningJobs.length;
    if (busy && !(await ask(`${busy === 1 ? "A job is" : `${busy} jobs are`} still running. Restarting stops ${busy === 1 ? "it" : "them"}; it can be run again after the update.`, { title: "Restart to update?", kind: "warning", okLabel: "Restart", cancelLabel: "Not now" }))) return;
    try { await api.updateInstall(); } catch (e) { app.fail(e); }
  }
  const openUrl = (u: string) => openLink(u);
</script>

<!-- The panel slides up from behind the bar; the Activity tab rides its top edge, tucked into the
     bar when closed and standing above the panel like a folder tab when open. -->
<!-- Esc anywhere inside closes it -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="activity" class:open={app.jobsOpen} class:dragging style:height="{app.jobsOpen ? height : 0}px"
  onkeydown={(e) => { if (e.key === "Escape" && app.jobsOpen) close(); }}>
  <button class="tab" bind:this={toggleEl} onclick={() => (app.jobsOpen = !app.jobsOpen)} aria-expanded={app.jobsOpen} aria-controls="activity-panel"
    title={app.jobsOpen ? "Hide activity (Esc)" : "Show activity: progress and logs of background work"}>
    Activity
    {#if app.jobs.length}<span class="count">{app.jobs.length}</span>{/if}
    <span class="chev"><Icon name="chevron" /></span>
  </button>
  <div class="clip">
  <section class="panel" id="activity-panel" aria-label="Activity" inert={!app.jobsOpen} style:height="{height}px">
    <!-- a focusable window splitter: arrow keys resize -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div class="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the activity panel" aria-valuenow={height} aria-valuemin={MIN} tabindex="0" onpointerdown={resize} onkeydown={resizeKey}></div>
    <header class="head">
      <span class="faint">{running.length ? `${running.length} running or waiting` : "Nothing running"}{finished ? ` · ${finished} finished` : ""}</span>
      <span class="spacer"></span>
      {#if finished}<button class="btn small ghost" onclick={clearFinished}>Clear finished</button>{/if}
    </header>
    {#if app.jobs.length === 0}
      <p class="idle faint">Nothing has run since the app started. Scans, fingerprinting, lookups and actions show their progress and logs here.</p>
    {:else}
    <div class="body">
      <ul class="jobs">
        {#each [...app.jobs].reverse() as j (j.id)}
          {@const p = pct(j.done, j.total)}
          <li>
            <button class="job" class:on={shown?.id === j.id} onclick={() => (picked = j.id)} aria-current={shown?.id === j.id}>
              <span class="lamp {lamp(j)}" class:live={j.state === "running"}></span>
              <span class="jl">{j.label}</span>
              <span class="js" class:bad={j.state === "failed"}>{stateText(j)}</span>
              {#if j.state === "running" && p !== null}<span class="meter" aria-hidden="true"><span style:width="{p}%"></span></span>{/if}
            </button>
          </li>
        {/each}
      </ul>
      <div class="log">
        {#if shown}
          <div class="loghead">
            <h3>{shown.label}</h3>
            <span class="chip {lamp(shown)}">{stateText(shown)}</span>
            {#if shown.state === "running" && shown.message}<span class="faint msg">{shown.message}</span>{/if}
            <span class="spacer"></span>
            {#if shown.state === "running" || shown.state === "queued"}<button class="btn small danger" onclick={() => api.cancelJob(shown.id)}>Cancel</button>{/if}
          </div>
          {#if shown.error}<p class="err">{shown.error}</p>{/if}
          <pre bind:this={logEl} onscroll={(e) => { const el = e.currentTarget as HTMLPreElement; stick = el.scrollHeight - el.scrollTop - el.clientHeight < 24; }}>{shown.log.join("\n") || (shown.state === "running" ? "Working…" : "No output.")}</pre>
        {/if}
      </div>
    </div>
    {/if}
  </section>
  </div>
</div>

<footer class="jobsbar">
  <!-- a mouse shortcut; the Activity key is the keyboard path -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <div class="summary" role="status" onclick={() => (app.jobsOpen = !app.jobsOpen)}>
    {#if current}
      {@const p = pct(current.done, current.total)}
      <MarkAnim size={14} label="Working" />
      <span class="label">{current.label}</span>
      <span class="muted">{current.message}{p !== null ? ` · ${p}%` : ""}</span>
      {#if running.length > 1}<span class="chip plain">+{running.length - 1} waiting</span>{/if}
    {:else if last?.state === "failed"}
      <span class="lamp bad"></span>
      <span class="label">{last.label}</span>
      <span class="muted">failed — open Activity for details</span>
    {:else}
      <span class="lamp"></span>
      <span class="muted">Nothing running</span>
    {/if}
  </div>
  {#if hashing || app.hash.paused}
    <div class="hashctl" title="How hard fingerprinting works the NAS and this PC — can be changed while it runs">
      <span class="muted">Fingerprinting speed</span>
      <select aria-label="Fingerprinting speed" value={app.hash.level} onchange={(e) => setLevel((e.currentTarget as HTMLSelectElement).value as HashLevel)}>
        {#each levels as l}<option value={l.v}>{l.label}</option>{/each}
      </select>
      <button class="btn small" onclick={togglePause}>{app.hash.paused ? "Resume" : "Pause"}</button>
    </div>
  {/if}
  {#if app.update.status?.available}
    {@const u = app.update.status}
    <div class="update" role="status">
      <span class="lamp ok"></span>
      {#if u.ready}<span>Index {u.ready} is ready</span>
      {:else if app.update.downloading}<span class="muted">Downloading Index {u.available?.version}…</span>
      {:else if !u.can_install}<span>Index {u.available?.version} is out</span>
      {:else}<span class="muted">Index {u.available?.version} is out</span>{/if}
      <button class="link" onclick={() => openUrl(u.available!.url)}>What's new</button>
      {#if u.ready}<button class="btn small primary" onclick={restartToUpdate}>Restart to update</button>
      {:else if !u.can_install}<button class="btn small" onclick={() => openUrl(u.available!.url)}>Download</button>{/if}
    </div>
  {/if}
  <span class="tab-room" aria-hidden="true"></span>
</footer>

<style>
  .update { display: flex; align-items: center; gap: 10px; padding: 0 12px; border-left: 1px solid var(--seam); white-space: nowrap; }
  .update .link { border: 0; background: none; padding: 0; color: var(--ink-soft); text-decoration: underline; cursor: pointer; font: inherit; }
  .update .link:hover { color: var(--ink); }
  /* ---- the bar ---- */
  .jobsbar { display: flex; align-items: stretch; height: 36px; border-top: 1px solid var(--seam); background: var(--panel-2); font-size: 13.5px; }
  /* hovering the bar — or the closed tab resting in it — lights the whole bar, edge to edge */
  .jobsbar:hover, .activity:not(.open):has(.tab:hover) ~ .jobsbar { background: color-mix(in srgb, var(--ink) 4%, var(--panel-2)); }
  .summary { flex: 1; display: flex; align-items: center; gap: 10px; min-width: 0; padding: 0 14px; cursor: pointer; text-align: left; overflow: hidden; white-space: nowrap; }
  .summary .muted { overflow: hidden; text-overflow: ellipsis; }
  .label { font-weight: 600; }
  .hashctl { display: flex; align-items: center; gap: 8px; padding: 0 12px; }
  .hashctl select { height: 26px; padding: 0 6px; font-size: 13.5px; line-height: normal; }
  .tab-room { flex: none; width: calc(var(--tab-w) + 16px); }
  .count { min-width: 20px; height: 18px; padding: 0 5px; border-radius: 9px; background: color-mix(in srgb, var(--ink) 10%, transparent); color: var(--ink-soft); font-size: 12px; font-weight: 600; line-height: 18px; text-align: center; font-variant-numeric: tabular-nums; }
  .chev { display: inline-flex; color: var(--ink-soft); transform: rotate(180deg); }
  .open .chev { transform: none; }
  @media (prefers-reduced-motion: no-preference) { .chev { transition: transform .18s cubic-bezier(.2, .8, .2, 1); } }

  /* ---- indicator lamps ---- */
  .lamp { width: 8px; height: 8px; flex: none; border-radius: 50%; background: var(--ink-faint); box-shadow: 0 0 0 2px color-mix(in srgb, var(--ink-faint) 18%, transparent); }
  .lamp.info { background: var(--info); box-shadow: 0 0 0 2px var(--info-bg); }
  .lamp.ok { background: var(--ok); box-shadow: 0 0 0 2px var(--ok-bg); }
  .lamp.bad { background: var(--bad); box-shadow: 0 0 0 2px var(--bad-bg); }
  .lamp.warn { background: var(--warn); box-shadow: 0 0 0 2px var(--warn-bg); }
  @media (prefers-reduced-motion: no-preference) {
    .lamp.live { animation: lamp 1.6s ease-in-out infinite; }
    @keyframes lamp { 50% { opacity: .35; } }
  }

  /* ---- the panel: part of the layout, directly above the bar ---- */
  :global(:root) { --tab-w: 128px; }
  /* the drawer's height animates; its bottom edge is the bar's top */
  .activity { position: relative; z-index: 3; min-height: 0; max-height: 70vh; }
  /* the panel keeps its full height and is revealed from the top edge as the drawer rises */
  .clip { position: absolute; inset: 0; overflow: hidden; }
  .panel { position: absolute; left: 0; right: 0; top: 0; display: grid; grid-template-rows: auto minmax(0, 1fr); max-height: 70vh; border-top: 1px solid var(--seam); background: var(--panel); }

  /* the tab rides the drawer's top edge: tucked into the bar when closed, a folder tab when open */
  .tab { position: absolute; right: 16px; bottom: 100%; z-index: 1; display: flex; align-items: center; justify-content: center; gap: 8px; width: var(--tab-w); height: 31px;
    border: 1px solid transparent; border-bottom: 0; border-radius: 7px 7px 0 0; background: transparent; color: var(--ink); font: inherit; font-weight: 600; font-size: 13.5px; cursor: pointer;
    transform: translateY(34px); }
  /* open: it stands on the panel, over the panel's top line, so the two read as one piece */
  .open .tab { transform: translateY(2px); background: var(--panel); border-color: var(--seam); }
  /* resizing: the panel's outline lights up — its top edge and up, over and down the tab — so the
     line goes around the tab rather than through it */
  .activity:has(.grip:hover) .panel, .activity:has(.grip:focus-visible) .panel, .activity.dragging .panel { border-top-color: var(--accent); box-shadow: inset 0 1px 0 var(--accent); }
  .open:has(.grip:hover) .tab, .open:has(.grip:focus-visible) .tab, .open.dragging .tab { border-color: var(--accent); box-shadow: inset 1px 1px 0 var(--accent), inset -1px 0 0 var(--accent); }
  .tab:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  @media (prefers-reduced-motion: no-preference) {
    .activity { transition: height .34s cubic-bezier(.2, .8, .2, 1); }
    .tab { transition: transform .34s cubic-bezier(.2, .8, .2, 1), background-color .2s, border-color .15s, box-shadow .15s; }
    .panel { transition: border-color .15s, box-shadow .15s; }
    .activity.dragging { transition: none; }
  }
  .grip { position: absolute; left: 0; right: 0; top: -4px; height: 8px; cursor: ns-resize; z-index: 1; }
  .grip:focus-visible { outline: none; }
  .head { display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 8px 0 16px; border-bottom: 1px solid var(--seam); }
  .head .faint { font-size: 13px; }
  .spacer { flex: 1; }
  .body { display: grid; grid-template-columns: minmax(240px, 320px) minmax(0, 1fr); min-height: 0; }

  .jobs { min-height: 0; overflow: auto; margin: 0; padding: 4px 0; list-style: none; border-right: 1px solid var(--seam); }
  .job { display: grid; grid-template-columns: 8px minmax(0, 1fr) auto; column-gap: 10px; align-items: center; width: 100%; padding: 7px 14px 7px 16px; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; text-align: left; }
  .job:hover { background: color-mix(in srgb, var(--ink) 4%, transparent); }
  .job.on { background: var(--panel-2); }
  .jl { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .job.on .jl { font-weight: 600; }
  .js { font-size: 12.5px; color: var(--ink-soft); font-variant-numeric: tabular-nums; }
  .js.bad { color: var(--bad); font-weight: 600; }
  .meter { grid-column: 2 / -1; height: 2px; margin-top: 5px; border-radius: 1px; background: color-mix(in srgb, var(--ink) 10%, transparent); overflow: hidden; }
  .meter span { display: block; height: 100%; background: var(--info); }
  .idle { margin: 0; padding: 14px 16px; max-width: 70ch; }

  .log { display: flex; flex-direction: column; min-width: 0; min-height: 0; padding: 10px 16px 12px; background: var(--plastic); }
  .loghead { display: flex; align-items: center; gap: 12px; min-height: 28px; margin-bottom: 8px; }
  .loghead h3 { margin: 0; font-size: 14px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .msg { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .err { margin: 0 0 8px; color: var(--bad); white-space: pre-wrap; }
  pre { flex: 1; min-height: 0; margin: 0; overflow: auto; overflow-wrap: anywhere; white-space: pre-wrap; font: 12.5px/1.5 ui-monospace, "Cascadia Mono", "DejaVu Sans Mono", monospace; color: var(--ink-soft); }
</style>
