<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { bytes } from "../lib/format";
  import { collapse, folderIn, setKey } from "../lib/files";
  import type { DupeGroup, Proof } from "../lib/types";
  import SkippedList from "../components/SkippedList.svelte";

  let groups = $state<DupeGroup[] | null>(null);
  let busy = $state(false);
  let filter = $state<"all" | "safe" | "region" | "decide">("all");
  // "archive anyway" needs a second click
  let arming = $state<string | null>(null);

  async function load() { try { groups = await api.dupes(null); } catch (e) { app.fail(e); } }
  onMount(load);
  // "Keep all": you've decided this group stays as it is. Remembered for these exact files —
  // a new copy turning up makes it a new group, which is suggested again.
  let skippedList = $state<SkippedList>();
  async function keepAll(g: DupeGroup) {
    try { await api.skip("dupe", g.skip_key, g.title, g.system_id); groups = (groups ?? []).filter((x) => x.skip_key !== g.skip_key); skippedList?.reload(); }
    catch (e) { app.fail(e); }
  }

  // "Unpack and compare" runs as a background job; the groups it covers reload when it ends
  let comparing = $state<{ job: number; keys: Set<string> }[]>([]);
  const gkey = (g: DupeGroup) => `${g.reason}:${g.keep}`;
  const isComparing = (g: DupeGroup) => comparing.some((c) => c.keys.has(gkey(g)));
  $effect(() => {
    const finished = comparing.filter((c) => { const j = app.jobs.find((x) => x.id === c.job); return j && !["queued", "running"].includes(j.state); });
    if (!finished.length) return;
    comparing = comparing.filter((c) => !finished.includes(c));
    for (const c of finished) {
      const j = app.jobs.find((x) => x.id === c.job);
      const failed = (j?.log ?? []).filter((l) => l.startsWith("✗"));
      if (j?.state === "failed") app.toast(`Compare failed: ${j.error ?? "unknown error"}`, "error");
      else if (failed.length) app.toast(`${failed.length} could not be unpacked: ${failed[0].slice(2)}`, "error");
    }
    load();
  });
  async function compare(gs: DupeGroup[]) {
    const ids = [...new Set(gs.flatMap((g) => g.files))];
    try {
      const job = await api.compareContents(ids);
      comparing = [...comparing, { job, keys: new Set(gs.map(gkey)) }];
      app.toast(`Unpacking and comparing ${gs.length} group${gs.length === 1 ? "" : "s"} — progress is in the jobs bar`);
    } catch (e) { app.fail(e); }
  }
  const uncheckedGroups = $derived((groups ?? []).filter((g) => g.proofs.some((p, i) => p === "Unchecked" && isExtra(g, i)) && g.can_unpack?.some(Boolean)));

  const reasons: Record<DupeGroup["reason"], string> = {
    SameHash: "The same file twice", EncDec: "Encrypted and decrypted copies", SameTitleDiffFormat: "The same game in different formats",
    SameTitle: "The same game more than once", SameTitleDiffRegion: "Released in several regions — keeping one (1G1R)", OlderUpdate: "An older version beside a newer one",
  };
  // what each file's status means, in plain words
  const verdict: Record<Proof, { pill: string; tone: string; why: string }> = {
    Kept: { pill: "Keep", tone: "ok", why: "" },
    Identical: { pill: "Copy", tone: "info", why: "Byte-for-byte the same as the kept file" },
    SameContent: { pill: "Copy", tone: "info", why: "Same content as the kept files, just packed differently" },
    Superseded: { pill: "Old version", tone: "info", why: "The kept newer version replaces it" },
    OtherRegion: { pill: "Other region", tone: "info", why: "One copy per game (1G1R) keeps the release at the top — turn that off in Settings to keep every region" },
    Different: { pill: "Unique", tone: "warn", why: "Holds data the kept files don't — only archive it if you don't want it" },
    Unchecked: { pill: "Not checked", tone: "plain", why: "Not compared yet — fingerprint this system to check" },
  };
  const SAFE: Proof[] = ["Identical", "SameContent", "Superseded"];
  const roots = $derived(app.settings?.library_roots ?? []);
  const keepers = $derived(new Set((groups ?? []).flatMap((g) => [g.keep, ...(g.keep_also ?? [])])));
  const isExtra = (g: DupeGroup, i: number) => g.proofs[i] !== "Kept" && !keepers.has(g.files[i]);
  const safeIds = (g: DupeGroup) => g.files.filter((_, i) => isExtra(g, i) && SAFE.includes(g.proofs[i]));
  const regionIds = (g: DupeGroup) => g.files.filter((_, i) => isExtra(g, i) && g.proofs[i] === "OtherRegion");
  const riskyIds = (g: DupeGroup) => g.files.filter((_, i) => isExtra(g, i) && !SAFE.includes(g.proofs[i]) && g.proofs[i] !== "OtherRegion");
  // rows, not files: the parts of a multi-part archive count once
  const count = (g: DupeGroup, ids: number[]) => new Set(ids.map((id) => setKey(g.names[g.files.indexOf(id)]))).size;
  const sizeOf = (g: DupeGroup, ids: number[]) => ids.reduce((a, id) => a + (g.sizes[g.files.indexOf(id)] ?? 0), 0);

  const all = $derived(groups ?? []);
  const safeGroups = $derived(all.filter((g) => safeIds(g).length && !riskyIds(g).length && !regionIds(g).length));
  const regionGroups = $derived(all.filter((g) => regionIds(g).length));
  const decideGroups = $derived(all.filter((g) => riskyIds(g).length));
  const shown = $derived(filter === "safe" ? safeGroups : filter === "region" ? regionGroups : filter === "decide" ? decideGroups : all);
  const safeTotal = $derived(all.reduce((a, g) => a + sizeOf(g, safeIds(g)), 0));
  const regionTotal = $derived(all.reduce((a, g) => a + sizeOf(g, regionIds(g)), 0));
  // a unique file that is the same game and version as the kept one: a different build or dump
  function whyFor(g: DupeGroup, i: number, p: Proof): string {
    if (p === "Identical" && g.unpacked?.[i]) return "Unpacked and compared: every file inside is byte-for-byte the same as a file in the kept copy (notes like .log or .txt aside)";
    if (p === "Different" && g.unpacked?.some((u, j) => u && (j === i || g.files[j] === g.keep))) return "Unpacked and compared: the files inside are not the same as the kept copy's — see what each one holds";
    if (p === "Different") {
      const k = g.files.indexOf(g.keep);
      if (g.notes?.[i] && g.notes[i] === g.notes?.[k]) return "Same game and version as the kept file, but a different build (for example a cart dump next to an eShop one) — keeping both loses nothing";
      return "Holds data the kept file doesn't — only archive it if you don't want it";
    }
    if (p === "Unchecked" && g.can_unpack?.some(Boolean)) return "Not compared yet — use Unpack and compare to look inside and check";
    return verdict[p].why;
  }

  function rows(g: DupeGroup) {
    const idx = g.files.map((_, i) => i);
    return collapse(idx, (i) => g.names[i], (i) => g.sizes[i] ?? 0).map((r) => {
      // the part that carries the archive's details (the first volume)
      const i = r.items.find((x) => g.inside?.[x]) ?? r.items[0];
      return { ...r, i, proof: g.proofs[i] as Proof, note: g.notes?.[i] ?? "", folder: folderIn(g.names[i], roots), extra: isExtra(g, i), inside: g.inside?.[i] ?? "", unpacked: g.unpacked?.[i] ?? false };
    }).sort((a, b) => Number(a.extra) - Number(b.extra));
  }

  async function archive(ids: number[]) {
    if (!ids.length) return;
    busy = true;
    try {
      const failed = await api.archiveFiles(ids, "duplicate");
      app.toast(`Moved ${ids.length - failed.length} file${ids.length - failed.length === 1 ? "" : "s"} to the archive — restorable for ${app.settings?.archive_hold_days ?? 7} days`);
      if (failed.length) app.toast(`${failed.length} could not be archived: ${failed[0][1]}`, "error");
      await load();
      app.refresh();
    } catch (e) { app.fail(e); } finally { busy = false; arming = null; }
  }
  function anyway(g: DupeGroup) {
    const k = `${g.reason}:${g.keep}`;
    if (arming !== k) { arming = k; return; }
    archive([...safeIds(g), ...riskyIds(g)]);
  }
</script>

<div class="page">
  <div class="page-head">
    <div>
      <h1>Duplicates</h1>
      <p>Each group shows which file stays and how every other file compares with it. <b>Copies</b> and <b>old versions</b> hold nothing you'd lose and can be archived. <b>Unique</b> files hold data the kept file doesn't, so they're only archived if you say so. Archived files can be restored for {app.settings?.archive_hold_days ?? 7} days.</p>
    </div>
    <span class="spacer"></span>
    <div class="bulk">
      {#if uncheckedGroups.length}<button class="btn" disabled={uncheckedGroups.every(isComparing)} onclick={() => compare(uncheckedGroups.filter((g) => !isComparing(g)))} title="Read every file inside the archives (nothing is written to disk) and compare them with the kept copies">Unpack and compare all ({uncheckedGroups.length})</button>{/if}
      {#if regionTotal > 0}<button class="btn" disabled={busy} onclick={() => archive(all.flatMap(regionIds))} title="One copy per game (1G1R): archive every other-region release">Archive other-region releases ({bytes(regionTotal)})</button>{/if}
      {#if safeTotal > 0}<button class="btn primary" disabled={busy} onclick={() => archive(all.flatMap(safeIds))}>Archive all copies ({bytes(safeTotal)})</button>{/if}
    </div>
  </div>

  {#if groups === null}<p class="faint">Looking for duplicates…</p>
  {:else if all.length === 0}<div class="empty"><h2>No duplicates found</h2><p>Fingerprint a system to also catch identical files stored under different names.</p></div>
  {:else}
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={filter === "all"} class:on={filter === "all"} onclick={() => (filter = "all")}>All <span>{all.length}</span></button>
      <button role="tab" aria-selected={filter === "safe"} class:on={filter === "safe"} onclick={() => (filter = "safe")}>Only copies <span>{safeGroups.length}</span></button>
      {#if regionGroups.length}<button role="tab" aria-selected={filter === "region"} class:on={filter === "region"} onclick={() => (filter = "region")}>Other regions <span>{regionGroups.length}</span></button>{/if}
      <button role="tab" aria-selected={filter === "decide"} class:on={filter === "decide"} onclick={() => (filter = "decide")}>Need a decision <span>{decideGroups.length}</span></button>
    </div>

    {#each shown as g (g.reason + g.keep)}
      {@const safe = safeIds(g)}
      {@const risky = riskyIds(g)}
      {@const region = regionIds(g)}
      {@const unchecked = risky.every((id) => g.proofs[g.files.indexOf(id)] === "Unchecked")}
      <section class="card">
        <header>
          <div class="ttl">
            <h3>{g.title}</h3>
            <span class="sys">{app.systemName(g.system_id)}</span>
          </div>
          <p class="reason">{reasons[g.reason]}</p>
        </header>
        <ul class="files">
          {#each rows(g) as r (r.key)}
            {@const v = verdict[r.extra ? r.proof : "Kept"]}
            {@const why = r.extra ? whyFor(g, r.i, r.proof) : ""}
            <li class:kept={!r.extra}>
              <span class="gpill {v.tone}">{v.pill}</span>
              <div class="body">
                <div class="name">{r.name}{#if r.parts > 1}<span class="parts"> · {r.parts} parts</span>{/if}</div>
                <div class="meta">
                  {#if r.note}<span>{r.note}</span>{/if}
                  <span>{bytes(r.size)}</span>
                  <span class="where">{r.folder ? `in ${r.folder}/` : "loose in the system folder"}</span>
                </div>
                {#if r.inside}<div class="inside"><span class="lbl">{r.unpacked ? "Inside (unpacked and fingerprinted)" : "Inside"}</span> {r.inside}</div>{/if}
                {#if why}<div class="why {v.tone}">{why}</div>{/if}
              </div>
            </li>
          {/each}
        </ul>
          <footer>
            <button class="btn small ghost" disabled={busy} onclick={() => keepAll(g)} title="Keep every file in this group and stop suggesting it — bring it back from the list at the bottom">Keep all</button>
            {#if g.proofs.some((p, i) => p === "Unchecked" && isExtra(g, i)) && g.can_unpack?.some(Boolean)}
              <button class="btn small" disabled={busy || isComparing(g)} onclick={() => compare([g])} title="Read every file inside the archives (nothing is written to disk) and compare them with the kept copy">{isComparing(g) ? "Comparing…" : "Unpack and compare"}</button>
            {/if}
            <span class="spacer"></span>
            {#if risky.length}
              {@const n = count(g, risky)}
              <button class="btn small" class:danger={arming === `${g.reason}:${g.keep}`} disabled={busy} onclick={() => anyway(g)}>
                {arming === `${g.reason}:${g.keep}` ? "Click again to archive" : unchecked ? `Archive ${n === 1 ? "it" : `${n} files`} without checking (${bytes(sizeOf(g, risky))})` : `Archive ${n === 1 ? "the unique file" : `${n} unique files`} anyway (${bytes(sizeOf(g, risky))})`}
              </button>
            {/if}
            {#if region.length}<button class="btn small" disabled={busy} onclick={() => archive([...region, ...safe])}>Archive {count(g, region) === 1 ? "the other release" : `${count(g, region)} other releases`} ({bytes(sizeOf(g, region))})</button>{/if}
            {#if safe.length}<button class="btn small primary" disabled={busy} onclick={() => archive(safe)}>Archive {count(g, safe) === 1 ? "the copy" : `${count(g, safe)} copies`} ({bytes(sizeOf(g, safe))})</button>{/if}
          </footer>
      </section>
    {/each}
  {/if}
  {#if groups !== null}<SkippedList bind:this={skippedList} kind="dupe" noun={{ one: "group", many: "groups" }} onchange={load} />{/if}
</div>

<style>
  .tabs { display: flex; gap: 4px; margin-bottom: 14px; border-bottom: 1px solid var(--seam); }
  .tabs button { border: 0; background: none; padding: 6px 12px; cursor: pointer; color: var(--ink-soft); font-weight: 600; border-bottom: 2px solid transparent; margin-bottom: -1px; }
  .tabs button.on { color: var(--ink); border-bottom-color: var(--accent); }
  .tabs span { color: var(--ink-faint); font-weight: 500; margin-left: 4px; }
  .card { border: 1px solid var(--seam); border-radius: var(--radius-m); background: var(--panel); margin-bottom: 12px; overflow: hidden; }
  header { padding: 10px 14px 6px; }
  .ttl { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  h3 { margin: 0; font-size: 16px; }
  .sys { font-size: 13px; color: var(--ink-faint); }
  .reason { margin: 2px 0 0; color: var(--ink-soft); font-size: 14px; }
  .files { list-style: none; margin: 0; padding: 0 14px; }
  .files li { display: flex; gap: 12px; align-items: flex-start; padding: 8px 0; border-top: 1px solid color-mix(in srgb, var(--seam) 60%, transparent); }
  .files .gpill { flex: none; width: 104px; padding-top: 1px; }
  .bulk { display: flex; gap: 8px; flex-wrap: wrap; justify-content: flex-end; }
  .body { min-width: 0; flex: 1; }
  .name { font-weight: 500; overflow-wrap: anywhere; color: var(--ink-soft); }
  li.kept .name { color: var(--ink); font-weight: 600; }
  .parts { color: var(--ink-faint); font-weight: 400; }
  .meta { display: flex; flex-wrap: wrap; gap: 4px 14px; font-size: 13px; color: var(--ink-faint); margin-top: 2px; }
  .where { overflow-wrap: anywhere; }
  .why { font-size: 13px; margin-top: 3px; }
  .why.info { color: var(--info); }
  .why.warn { color: var(--warn); }
  .why.plain { color: var(--ink-soft); }
  footer { display: flex; gap: 8px; justify-content: flex-end; align-items: center; padding: 8px 14px 10px; }
  footer .spacer { flex: 1; }
  .inside { font-size: 13px; color: var(--ink-soft); margin-top: 3px; overflow-wrap: anywhere; }
  .inside .lbl { color: var(--ink-faint); }
</style>
