<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { basename } from "../lib/format";
  import { diffNames } from "../lib/diff";
  import SkippedList from "./SkippedList.svelte";
  import type { BatchResult, Problem, RenameItem } from "../lib/types";
  let { gameIds, onclose }: { gameIds: number[]; onclose: () => void } = $props();

  let items = $state<RenameItem[] | null>(null);
  let chosen = $state<Set<number>>(new Set());
  let result = $state<BatchResult | null>(null);
  let busy = $state(false);
  let showClean = $state(false);

  // games you chose to name by their similarity match; the plan is rebuilt with them so
  // name clashes are checked against the names that would really be used
  let useMatched = $state<Set<number>>(new Set());
  let first = true;
  async function plan() {
    try {
      const p = await api.planRenames(gameIds, [...useMatched]);
      const keep = first ? null : chosen;
      items = p.filter((i) => i.problem?.kind !== "Unchanged" || i.suggestion);
      const ok = items.filter((i) => !i.problem).map((i) => i.file_id);
      chosen = new Set(keep ? ok.filter((id) => keep.has(id)) : ok);
      first = false;
    } catch (e) { app.fail(e); onclose(); }
  }
  $effect(() => { void gameIds; plan(); });
  async function toggleMatched(gameId: number) {
    const s = new Set(useMatched);
    s.has(gameId) ? s.delete(gameId) : s.add(gameId);
    useMatched = s;
    await plan();
    // choosing a name means you want this file renamed
    const ids = (items ?? []).filter((i) => i.game_id === gameId && !i.problem).map((i) => i.file_id);
    chosen = new Set([...chosen, ...ids]);
  }
  // the suggestion shows once per game, under its first file
  const firstOfGame = $derived(new Set(Object.values(Object.fromEntries([...(items ?? [])].reverse().map((i) => [i.game_id, i.file_id])))));
  const ready = $derived((items ?? []).filter((i) => !i.problem));
  const blocked = $derived((items ?? []).filter((i) => i.problem && i.problem.kind !== "Unchanged"));
  // names that stay the same unless you pick their suggested name: kept out of the list, one per game
  const cleanWithSuggestion = $derived((items ?? []).filter((i) => i.problem?.kind === "Unchanged" && i.suggestion && firstOfGame.has(i.file_id)));

  // The preview as a tree: each game, then its files by kind (base game, updates, DLC).
  const KINDS = [{ role: "game", label: "Base game" }, { role: "disc", label: "Discs" }, { role: "update", label: "Updates" }, { role: "dlc", label: "DLC" }] as const;
  // where a new name comes from, in words; names rebuilt from the file itself (the usual case) say nothing
  const SOURCE: Record<string, string> = { dat: "DAT name", official: "Official title", matched: "Your chosen match", header: "ID read from the file" };
  const SOURCE_WHY: Record<string, string> = {
    dat: "The file is verified against a No-Intro/Redump list, so it gets that list's name",
    official: "Matched exactly by title ID or fingerprint, so the title is the official one",
    matched: "The name you chose from a match by similarity",
    header: "The title ID and version are read from the file's header",
  };
  type Group = { id: number; title: string; system: string; files: RenameItem[]; kinds: { label: string | null; files: RenameItem[] }[]; sources: string[]; suggestion: RenameItem | null };
  const groups = $derived.by(() => {
    const by = new Map<number, RenameItem[]>();
    for (const i of [...ready, ...blocked]) (by.get(i.game_id) ?? by.set(i.game_id, []).get(i.game_id)!).push(i);
    return [...by.entries()].map(([id, files]): Group => {
      files.sort((a, b) => a.role_order - b.role_order || basename(a.src).localeCompare(basename(b.src)));
      const present = KINDS.filter((k) => files.some((f) => f.role === k.role));
      // one kind of file needs no heading; a lone game file is just the game
      const kinds = present.map((k) => ({ label: present.length > 1 ? k.label : k.role === "game" ? null : k.label, files: files.filter((f) => f.role === k.role) }));
      return { id, title: files[0].game_title || basename(files[0].src), system: app.systemName(files[0].system_id), files, kinds,
        sources: [...new Set(files.map((f) => SOURCE[f.source]).filter(Boolean))], suggestion: files.find((f) => f.suggestion && firstOfGame.has(f.file_id)) ?? null };
    });
  });
  const gameCount = $derived(groups.length);
  function pickGame(g: Group) {
    const ids = g.files.filter((f) => !f.problem).map((f) => f.file_id);
    const all = ids.every((id) => chosen.has(id));
    const s = new Set(chosen);
    for (const id of ids) all ? s.delete(id) : s.add(id);
    chosen = s;
  }
  // "Keep names": these files keep their names, and aren't offered again unless brought back
  async function keepNames(g: Group) {
    try {
      for (const f of g.files) await api.skip("rename", f.skip_key, basename(f.src), f.system_id);
      await plan();
    } catch (e) { app.fail(e); }
  }
  const pickFile = (id: number) => { const s = new Set(chosen); s.has(id) ? s.delete(id) : s.add(id); chosen = s; };

  function why(p: Problem): string {
    switch (p.kind) {
      case "Collision": return "Two files would get this name";
      case "TargetExists": return "A file with this name already exists";
      case "IllegalOnWindows": return `Not a valid file name on Windows: ${p.detail}`;
      default: return "";
    }
  }
  async function apply() {
    busy = true;
    try {
      result = await api.applyRenames(ready.filter((i) => chosen.has(i.file_id)));
      app.refresh();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function undo() {
    if (!result) return;
    busy = true;
    try { const u = await api.undo(result.batch_id); app.toast(`Undid ${u.done.length} renames`); app.refresh(); onclose(); }
    catch (e) { app.fail(e); } finally { busy = false; }
  }
</script>

{#snippet fromName(src: string, dst: string)}
  <span class="name from">{#each diffNames(basename(src), basename(dst)).from as r}{#if r.kind === "same"}{r.text}{:else}<del>{r.text}</del>{/if}{/each}</span>
{/snippet}
{#snippet toName(src: string, dst: string)}
  <span class="name">{#each diffNames(basename(src), basename(dst)).to as r}{#if r.kind === "same"}{r.text}{:else}<ins>{r.text}</ins>{/if}{/each}</span>
{/snippet}

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="rn-title">
    <header>
      <h2 id="rn-title">Clean up names</h2>
      <p>Verified games get their No-Intro/Redump name, and games matched exactly by title ID or fingerprint get their official title. Everything else is rebuilt from the file's own title, region and tags. Titles matched only by name are offered below a file for you to choose, since a name match can be a different release. Nothing changes until you apply, and every rename can be undone.</p>
    </header>
    <div class="body">
      {#if items === null}<p class="faint">Building the preview…</p>
      {:else if result}
        <p><b>Renamed {result.done.length} file{result.done.length === 1 ? "" : "s"}.</b>{#if result.failed.length} {result.failed.length} could not be renamed:{/if}</p>
        {#each result.failed as [op, err]}<p class="path">{basename(op.src)}: <span style="color: var(--bad)">{err}</span></p>{/each}
      {:else if ready.length + blocked.length + cleanWithSuggestion.length === 0}<p class="empty">These names are already clean.</p>
      {:else}
        <div class="tree" role="list">
          <div class="cols">
            <input type="checkbox" aria-label="Choose all" checked={chosen.size === ready.length && ready.length > 0} indeterminate={chosen.size > 0 && chosen.size < ready.length}
              onchange={() => (chosen = chosen.size === ready.length ? new Set() : new Set(ready.map((i) => i.file_id)))} />
            <span>Current name</span><span>New name</span>
          </div>
          {#each groups as g (g.id)}
            {@const ids = g.files.filter((f) => !f.problem).map((f) => f.file_id)}
            {@const on = ids.filter((id) => chosen.has(id)).length}
            <section class="game" role="listitem" aria-label={g.title}>
              <div class="ghead">
                {#if ids.length}<input type="checkbox" aria-label="Choose {g.title}" checked={on === ids.length} indeterminate={on > 0 && on < ids.length} onchange={() => pickGame(g)} />{:else}<span></span>{/if}
                <span class="gtitle">{g.title}</span>
                <span class="gmeta"><button class="more-link keep" onclick={() => keepNames(g)} title="Keep the current {g.files.length === 1 ? "name" : "names"} and don't offer {g.files.length === 1 ? "it" : "them"} again">Keep {g.files.length === 1 ? "name" : "names"}</button>{g.system}{#each g.sources as src}<span class="src" title={SOURCE_WHY[Object.keys(SOURCE).find((k) => SOURCE[k] === src) ?? ""]}>{src}</span>{/each}</span>
              </div>
              {#if g.suggestion}
                {@const i = g.suggestion}
                <label class="sugg"><input type="checkbox" checked={i.using_suggestion} onchange={() => toggleMatched(i.game_id)} />
                  <span>Name it “{i.suggestion}” <span class="faint">— matched by name on {i.suggestion_source}{i.suggestion_console === "match" ? ", checked to be for this console" : ", console not checked"}; make sure it's the same release (an edition, remaster or region can differ)</span></span></label>
              {/if}
              {#each g.kinds as k}
                {#if k.label}<div class="kind">{k.label}{#if k.files.length > 1}<span class="count">{k.files.length}</span>{/if}</div>{/if}
                {#each k.files as i (i.file_id)}
                  <div class="file" class:blocked={!!i.problem} class:nested={!!k.label}>
                    {#if i.problem}<span></span>{:else}<input type="checkbox" aria-label="Rename {basename(i.src)}" checked={chosen.has(i.file_id)} onchange={() => pickFile(i.file_id)} />{/if}
                    {@render fromName(i.src, i.dst)}
                    <span class="to">{@render toName(i.src, i.dst)}{#if i.problem}<span class="why">{why(i.problem)}</span>{/if}</span>
                  </div>
                {/each}
              {/each}
            </section>
          {/each}
        </div>
        {#if cleanWithSuggestion.length}
          <section class="clean">
            <button class="more-link" aria-expanded={showClean} onclick={() => (showClean = !showClean)}>
              {showClean ? "Hide" : "Show"} {cleanWithSuggestion.length} {cleanWithSuggestion.length === 1 ? "name that is" : "names that are"} already clean but {cleanWithSuggestion.length === 1 ? "has" : "have"} a suggested name
            </button>
            {#if showClean}
              <ul>
                {#each cleanWithSuggestion as i (i.file_id)}
                  <li>
                    <span class="name">{basename(i.src)}</span>
                    <label class="sugg"><input type="checkbox" checked={i.using_suggestion} onchange={() => toggleMatched(i.game_id)} />
                      <span>Name it “{i.suggestion}” <span class="faint">— matched by name on {i.suggestion_source}; check it's the same release</span></span></label>
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {/if}
      {/if}
      {#if items !== null && !result}<SkippedList kind="rename" noun={{ one: "name", many: "names" }} onchange={plan} />{/if}
    </div>
    <footer>
      {#if result}
        <button class="btn" disabled={busy || result.done.length === 0} onclick={undo}>Undo these renames</button>
        <button class="btn primary" onclick={onclose}>Done</button>
      {:else}
        <span class="left">{ready.length} {ready.length === 1 ? "file" : "files"} in {gameCount} {gameCount === 1 ? "game" : "games"}{blocked.length ? ` · ${blocked.length} need attention` : ""}</span>
        <button class="btn ghost" onclick={onclose}>Cancel</button>
        <button class="btn primary" disabled={busy || chosen.size === 0} onclick={apply}>Rename {chosen.size} file{chosen.size === 1 ? "" : "s"}</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  /* one grid for the whole tree, so current and new names line up across games */
  .tree { --cols: 22px minmax(0, 1fr) minmax(0, 1fr); }
  .cols, .ghead, .file { display: grid; grid-template-columns: var(--cols); column-gap: 14px; align-items: baseline; }
  .cols { position: sticky; top: 0; z-index: 1; padding: 8px 0; background: var(--panel); border-bottom: 1px solid var(--seam); font-size: 13px; font-weight: 600; color: var(--ink-soft); align-items: center; }
  .game { padding: 10px 0 12px; border-bottom: 1px solid var(--seam); }
  .ghead { grid-template-columns: 22px minmax(0, 1fr) auto; align-items: center; }
  .gtitle { font-weight: 600; font-size: 14.5px; overflow-wrap: anywhere; }
  .gmeta { display: flex; gap: 14px; font-size: 13px; color: var(--ink-soft); white-space: nowrap; }
  .src { color: var(--ink); font-weight: 600; }
  .keep { font-size: 13px; }
  .kind { margin: 8px 0 2px 48px; font-size: 12.5px; font-weight: 600; color: var(--ink-soft); }
  .count { margin-left: 6px; font-weight: 500; color: var(--ink-faint); }
  .file { padding: 4px 0; font-size: 13.5px; }
  .file > :global(.name.from) { padding-left: 14px; }
  .file.nested > :global(.name.from) { padding-left: 26px; }
  .file input { margin: 2px 0 0; align-self: start; }
  .file.blocked { color: var(--ink-faint); }
  .to { overflow-wrap: anywhere; }
  .why { display: block; color: var(--warn); font-size: 13px; }
  .sugg { margin-left: 36px; }
  /* exact names: double spaces stay visible, and only what changes is marked */
  .name { white-space: pre-wrap; }
  .from { color: var(--ink-soft); }
  del { text-decoration: line-through; text-decoration-color: var(--bad); background: var(--bad-bg); color: var(--ink); border-radius: 2px; }
  ins { text-decoration: none; background: var(--ok-bg); color: var(--ink); font-weight: 600; border-radius: 2px; box-shadow: inset 0 -1.5px 0 var(--ok); }
  .clean { margin: 14px 0 6px; }
  .clean ul { list-style: none; margin: 8px 0 0; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  .clean .name { color: var(--ink-soft); }
  .sugg { display: flex; gap: 8px; align-items: flex-start; margin-top: 6px; font-size: 13px; cursor: pointer; }
  .clean .sugg { margin-left: 0; }
  .sugg input { margin-top: 2px; }
</style>
