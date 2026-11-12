<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { basename, bytes, dirname } from "../lib/format";
  import { collapse, folderIn, placeOf, systemFolder } from "../lib/files";
  import type { JunkItem, Layout, LayoutItem, SystemLayout } from "../lib/types";
  import SkippedList from "../components/SkippedList.svelte";

  let items = $state<JunkItem[] | null>(null);
  let layouts = $state<SystemLayout[] | null>(null);
  let busy = $state(false);
  let hidden = $state<Set<string>>(new Set());
  let checking = $state(false);
  let openSets = $state<Set<string>>(new Set());
  const roots = $derived(app.settings?.library_roots ?? []);
  const hold = $derived(app.settings?.archive_hold_days ?? 7);

  // Listing every system folder touches the NAS, so it runs when the page opens and when asked —
  // not after every change. Acting on an item updates the list in place.
  async function check() {
    checking = true;
    try {
      const [j, l] = await Promise.all([api.junk(), api.layout()]);
      items = j; layouts = l; hidden = new Set();
    } catch (e) { app.fail(e); } finally { checking = false; }
  }
  async function recheckLayout() { try { layouts = await api.layout(); } catch (e) { app.fail(e); } }
  onMount(check);

  // what you skip here is remembered, and listed at the bottom of the page to bring back
  let skippedJunk = $state<SkippedList>();
  let skippedLayout = $state<SkippedList>();
  async function ignore(i: JunkItem) {
    try { await api.skip("junk", i.path, basename(i.path), i.system_id); hidden = new Set([...hidden, i.path]); skippedJunk?.reload(); } catch (e) { app.fail(e); }
  }
  async function leave(s: SystemLayout, i: LayoutItem) {
    try {
      await api.skip("layout", i.skip_key, i.title, s.system_id);
      layouts = (layouts ?? []).map((x) => (x === s ? { ...x, items: x.items.filter((y) => y !== i) } : x));
      skippedLayout?.reload();
    } catch (e) { app.fail(e); }
  }

  // ---- folder layout ----
  const layoutNames: Record<Layout, string> = { loose: "Files directly in the system folder", "per-game": "One folder per game", grouped: "Shared folders (Games, DLC, Updates…)", leave: "Leave as is" };
  const problemText: Record<LayoutItem["problem"], string> = {
    Split: "Its files are in more than one place", NeedsFolder: "Loose in the system folder, but this system keeps each game in its own folder",
    InOwnFolder: "In a folder of its own, but this system keeps games loose", NotFiled: "Loose in the system folder, but this system files games into shared folders",
  };
  const withItems = $derived((layouts ?? []).filter((s) => s.items.length));
  const fixable = (s: SystemLayout) => s.items.filter((i) => i.moves.length || i.duplicates.length);
  const where = (p: string) => { const f = folderIn(p, roots); return f ? `${f}/` : "the system folder"; };
  function plan(i: LayoutItem) {
    const moves = collapse(i.moves, (m) => m.src, (m) => m.size).map((r) => ({
      ...r, from: where(r.items[0].src),
      renamed: basename(r.items[0].dst) !== basename(r.items[0].src) ? basename(r.items[0].dst) : null,
    }));
    const spares = collapse(i.duplicates, (d) => d.src, (d) => d.size).map((r) => ({
      ...r, from: where(r.items[0].src), note: r.items[0].note,
      why: r.items[0].proof === "Identical" ? `Byte-for-byte the same as ${basename(r.items[0].same_as)}, which stays`
        : r.items[0].note.startsWith("Archive with") ? "Everything inside it is already there as unpacked files, which stay"
        : `Same content as ${basename(r.items[0].same_as)}, just packed differently — that file stays`,
    }));
    return { moves, spares };
  }
  async function chooseLayout(sys: SystemLayout, v: string) {
    await app.setFolderLayout(sys.system_id, v === "auto" ? null : v);
    await recheckLayout();
  }
  async function tidy(list: LayoutItem[]) {
    busy = true;
    try {
      const r = await api.fixLayout(list.map((i) => i.game_id));
      if (r.failed.length) app.toast(`${r.failed.length} step${r.failed.length === 1 ? "" : "s"} failed: ${r.failed[0][1]}`, "error");
      else app.toast(`Tidied ${list.length} game${list.length === 1 ? "" : "s"} — undo from Change history`);
      api.scan(false);
      await recheckLayout();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }

  // ---- things that don't belong ----
  type Kind = JunkItem["kind"];
  const sections: { kind: Kind; title: string; hint: string; pill: string; tone: string }[] = [
    { kind: "RomInToolFolder", title: "Games inside tool folders", pill: "Stray game", tone: "warn", hint: "A game file sitting in a tool's folder, where scans don't see it. Move it back among the games." },
    { kind: "ToolFolder", title: "Program folders among the games", pill: "Program", tone: "plain", hint: "A program's own folder. It moves into the system's _Tools folder as a whole, so the program keeps all its files." },
    { kind: "ToolFiles", title: "Scripts and the files they use", pill: "Scripts", tone: "plain", hint: "Scripts loose in a system folder. They move into _Tools together with the lists and notes beside them." },
    { kind: "Executable", title: "Programs inside game folders", pill: "Program", tone: "plain", hint: "A program in a folder of games. Only the program moves into _Tools; the games stay where they are." },
    { kind: "LooseToolFolder", title: "Tool folders outside _Tools", pill: "Tool folder", tone: "plain", hint: "Tool folders that scans already skip, sitting among the games. Moving them keeps every tool for a system in one place. A folder that still holds game-sized files is refused." },
    { kind: "TempLeftover", title: "Leftovers from interrupted copies", pill: "Leftover", tone: "plain", hint: "Partial files from a copy or download that didn't finish. Safe to remove." },
    { kind: "UrlShortcut", title: "Web shortcuts", pill: "Shortcut", tone: "plain", hint: "Links to websites saved next to the games." },
    { kind: "EmptyDir", title: "Empty folders", pill: "Empty", tone: "plain", hint: "Folders with nothing in them. Removing them can be undone." },
    { kind: "ForeignSystem", title: "Files that don't match the system", pill: "Wrong type", tone: "warn", hint: "File types this system doesn't use. Check them yourself — they may belong to another system." },
  ];
  const TOOLISH: Kind[] = ["ToolFolder", "ToolFiles", "LooseToolFolder", "Executable"];
  const canArchive = (k: Kind) => !["ToolFolder", "ToolFiles", "LooseToolFolder"].includes(k);
  const visible = $derived((items ?? []).filter((i) => !hidden.has(i.path)));
  const bySection = $derived(sections.map((s) => ({ ...s, list: visible.filter((i) => i.kind === s.kind) })).filter((s) => s.list.length));
  const isProgram = (p: string) => /\.(exe|bat|cmd|dll|py|sh|msi|apk|jar)$/i.test(p);
  const toolsOf = (p: string) => `${systemFolder(p, roots) || "system"} › _Tools`;

  async function archive(list: JunkItem[]) {
    busy = true;
    try {
      const failed = await api.archivePaths(list.map((i) => i.path), "cleanup");
      const ok = list.length - failed.length;
      if (ok) app.toast(`Cleaned up ${ok} item${ok === 1 ? "" : "s"}`);
      if (failed.length) app.toast(`${failed.length} failed: ${failed[0][1]}`, "error");
      hidden = new Set([...hidden, ...list.map((i) => i.path).filter((p) => !failed.some(([f]) => f === p))]);
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function moveOut(i: JunkItem) {
    busy = true;
    try {
      const r = await api.moveFile(i.path, dirname(dirname(i.path)));
      if (r.failed.length) app.toast(r.failed[0][1], "error"); else { app.toast(`Moved ${basename(i.path)}`); hidden = new Set([...hidden, i.path]); api.scan(false); }
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function toTools(list: JunkItem[]) {
    busy = true;
    try {
      const r = await api.moveToTools(list.flatMap((i) => (i.members?.length ? i.members : [i.path])));
      const failedPaths = new Set(r.failed.map(([op]) => op.src));
      if (r.failed.length) app.toast(`${r.failed.length} not moved: ${r.failed[0][1]}`, "error");
      const moved = list.filter((i) => !(i.members?.length ? i.members : [i.path]).some((p) => failedPaths.has(p)));
      if (moved.length) app.toast(`Moved ${moved.length} item${moved.length === 1 ? "" : "s"} into _Tools — undo from Change history`);
      hidden = new Set([...hidden, ...moved.map((i) => i.path)]);
      api.scan(false);
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const toggle = (k: string) => { const s = new Set(openSets); s.has(k) ? s.delete(k) : s.add(k); openSets = s; };
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Cleanup</h1>
      <p>Things that are out of place in your system folders. Tools move into each system's <b>_Tools</b> folder, which is never scanned as games. Other files go to the archive for {hold} days before you're asked to delete them. Every change can be undone from Change history.</p>
    </div>
    <span class="spacer"></span>
    <button class="btn" disabled={checking} onclick={check}>{checking ? "Checking…" : "Check again"}</button>
  </div>

  {#if items === null}<p class="faint">Checking folders…</p>
  {:else if !bySection.length && !withItems.length}<div class="empty"><h2>Nothing to clean up</h2><p>Every system folder only holds game files, laid out the way the rest of its games are.</p></div>
  {/if}

  {#if withItems.length}
    <h2 class="sec">Folder layout</h2>
    <p class="muted sec-hint">Each system follows the layout most of its games already use, unless you pick one. Tidying gathers a game's files into one place. A file only goes to the archive when it's proven to be a copy of one that stays; anything else is moved in, never thrown away.</p>
    {#each withItems as s (s.system_id)}
      <div class="sys-head">
        <h3>{app.systemName(s.system_id)}</h3>
        <label class="faint lay">Layout
          <select value={s.chosen ? s.layout : "auto"} onchange={(e) => chooseLayout(s, (e.currentTarget as HTMLSelectElement).value)} aria-label="Layout for {app.systemName(s.system_id)}">
            <option value="auto">{layoutNames[s.detected]} (what most games use)</option>
            {#each Object.entries(layoutNames) as [k, label]}{#if k !== s.detected || s.chosen}<option value={k}>{label}</option>{/if}{/each}
          </select>
        </label>
        <span class="spacer"></span>
        {#if fixable(s).length > 1}<button class="btn small" disabled={busy} onclick={() => tidy(fixable(s))}>Tidy all {fixable(s).length}</button>{/if}
      </div>
      {#each s.items as i (i.game_id)}
        {@const p = plan(i)}
        <article class="gcard">
          <div class="gcard-head">
            <div class="grow">
              <h3>{i.title}</h3>
              <p class="sub">{problemText[i.problem]}</p>
              {#if i.target}<p class="sub dest">Everything goes into <b>{basename(i.target)}/</b>{#if i.new_folder} <span class="faint">(a new folder)</span>{/if}</p>{/if}
            </div>
            <div class="actions">
              <button class="btn small ghost" disabled={busy} onclick={() => leave(s, i)} title="Don't suggest this again — bring it back from the list at the bottom">Leave as is</button>
              {#if p.moves.length || p.spares.length}<button class="btn small primary" disabled={busy} onclick={() => tidy([i])}>Tidy</button>{/if}
            </div>
          </div>
          <ul class="grows">
            {#each p.moves as r (r.key)}
              <li>
                <span class="gpill ok">Move in</span>
                <div class="grow-body">
                  <div class="grow-name">{r.name}{#if r.parts > 1}<span class="faint"> · {r.parts} parts</span>{/if}</div>
                  <div class="grow-meta"><span>{bytes(r.size)}</span><span>from {r.from}</span></div>
                  {#if r.renamed}<div class="grow-note warn">A different file already has this name there, so this one becomes {r.renamed}</div>{/if}
                </div>
              </li>
            {/each}
            {#each p.spares as r (r.key)}
              <li>
                <span class="gpill info">Archive</span>
                <div class="grow-body">
                  <div class="grow-name">{r.name}{#if r.parts > 1}<span class="faint"> · {r.parts} parts</span>{/if}</div>
                  <div class="grow-meta">{#if r.note}<span>{r.note}</span>{/if}<span>{bytes(r.size)}</span><span>in {r.from}</span></div>
                  <div class="grow-note info">{r.why}</div>
                </div>
              </li>
            {/each}
            {#if !i.target}
              {#each i.left as [path, why] (path)}
                <li><span class="gpill">No fix</span><div class="grow-body"><div class="grow-name">{basename(path)}</div><div class="grow-note">{why}</div></div></li>
              {/each}
            {/if}
          </ul>
          {#if p.moves.length || p.spares.length}<p class="foot faint">Folders this leaves empty are removed. Archived copies can be restored for {hold} days.</p>{/if}
        </article>
      {/each}
    {/each}
  {/if}

  {#each bySection as sec (sec.kind)}
    <section class="gcard">
      <div class="gcard-head">
        <div class="grow">
          <h3>{sec.title} <span class="count">{sec.list.length}</span></h3>
          <p class="sub">{sec.hint}</p>
        </div>
        <div class="actions">
          {#if TOOLISH.includes(sec.kind)}<button class="btn small primary" disabled={busy} onclick={() => toTools(sec.list)}>Move all to _Tools</button>
          {:else if sec.kind === "EmptyDir"}<button class="btn small" disabled={busy} onclick={() => archive(sec.list)}>Remove all</button>
          {:else if sec.kind === "TempLeftover" || sec.kind === "UrlShortcut"}<button class="btn small" disabled={busy} onclick={() => archive(sec.list)}>Archive all</button>{/if}
        </div>
      </div>
      <ul class="grows">
        {#each sec.list as i (i.path)}
          {@const scripts = (i.members ?? []).filter(isProgram)}
          {@const others = (i.members ?? []).filter((m) => !isProgram(m))}
          <li>
            <span class="gpill {sec.tone}">{sec.pill}</span>
            <div class="grow-body">
              <div class="grow-name">{scripts.length ? scripts.map(basename).join(", ") : basename(i.path)}</div>
              <div class="grow-meta">
                <span>in {placeOf(i.path, roots)}</span>
                {#if i.size}<span>{bytes(i.size)}</span>{/if}
                {#if TOOLISH.includes(sec.kind)}<span>→ {toolsOf(i.path)}</span>{/if}
                {#if sec.kind === "RomInToolFolder"}<span>→ {systemFolder(i.path, roots)}</span>{/if}
              </div>
              {#if others.length}
                <div class="grow-note">Moves with {others.length} file{others.length === 1 ? "" : "s"} beside {scripts.length === 1 ? "it" : "them"}
                  <button class="more-link" onclick={() => toggle(i.path)}>{openSets.has(i.path) ? "hide" : "show"}</button>
                </div>
                {#if openSets.has(i.path)}<ul class="members">{#each others as m (m)}<li>{basename(m)}</li>{/each}</ul>{/if}
              {/if}
            </div>
            <div class="grow-actions">
              {#if sec.kind === "RomInToolFolder"}<button class="btn small primary" disabled={busy} onclick={() => moveOut(i)}>Move to {app.systemName(i.system_id)}</button>{/if}
              {#if TOOLISH.includes(sec.kind)}<button class="btn small primary" disabled={busy} onclick={() => toTools([i])}>Move to _Tools</button>{/if}
              {#if canArchive(sec.kind)}<button class="btn small" disabled={busy} onclick={() => archive([i])}>{sec.kind === "EmptyDir" ? "Remove" : "Archive"}</button>{/if}
              <button class="btn small ghost" onclick={() => ignore(i)} title="Don't suggest this again — bring it back from the list at the bottom">Ignore</button>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#if items !== null}
    <SkippedList bind:this={skippedLayout} kind="layout" noun={{ one: "game folder", many: "game folders" }} onchange={recheckLayout} />
    <SkippedList bind:this={skippedJunk} kind="junk" noun={{ one: "item", many: "items" }} onchange={check} />
  {/if}
</div>

<style>
  .spacer { flex: 1; }
  h2.sec { margin: 4px 0 2px; font-size: 18px; }
  .sec-hint { margin: 0 0 6px; max-width: 90ch; }
  .sys-head { display: flex; align-items: center; gap: 12px; margin: 14px 0 8px; }
  .sys-head h3 { margin: 0; font-size: 16px; }
  .lay { display: flex; align-items: center; gap: 6px; font-size: 13px; }
  select { max-width: 340px; }
  .dest { color: var(--ink); }
  .foot { font-size: 12.5px; margin: 0; padding: 6px 14px 10px; }
  .members { list-style: none; margin: 4px 0 0; padding: 0; columns: 2 220px; font-size: 13px; color: var(--ink-soft); }
  .members li { overflow-wrap: anywhere; break-inside: avoid; padding: 1px 0; }
  .more-link { padding: 0 0 0 6px; }
</style>
