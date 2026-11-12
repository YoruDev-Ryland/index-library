<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { basename } from "../lib/format";
  import { placeOf } from "../lib/files";
  import type { BatchInfo, OpItem } from "../lib/types";

  let list = $state<BatchInfo[] | null>(null);
  let busy = $state(false);
  let open = $state<Set<string>>(new Set());
  const load = () => api.history().then((h) => (list = h)).catch((e) => app.fail(e));
  $effect(() => { void app.revision; load(); });
  const roots = $derived(app.settings?.library_roots ?? []);

  // what a batch was, in words (old batches carry their raw label)
  function title(label: string, n: number): string {
    const s = n === 1 ? "" : "s";
    const known: Record<string, string> = {
      "archive: duplicate": `Archived ${n} duplicate${s}`, "archive: cleanup": `Archived ${n} item${s} during cleanup`,
      "Remove empty folder": `Removed ${n} empty folder${s}`, "Tidy folders": `Tidied ${n} game folder${s}`,
      "Move file": `Moved ${n} file${s}`, "Move to _Tools": `Moved ${n} tool${s} into _Tools`,
    };
    return known[label] ?? (n > 1 ? `${label} (${n})` : label);
  }
  const verb: Record<string, { pill: string; tone: string }> = {
    rename: { pill: "Renamed", tone: "info" }, move: { pill: "Moved", tone: "ok" }, archive: { pill: "Archived", tone: "info" },
    restore: { pill: "Restored", tone: "ok" }, delete: { pill: "Deleted", tone: "bad" }, writefile: { pill: "Edited", tone: "info" }, rmdir: { pill: "Removed", tone: "plain" },
  };
  const v = (k: string) => verb[k] ?? { pill: k, tone: "plain" };

  // batches of the same kind made within a few minutes of each other read as one change
  const groups = $derived.by(() => {
    const out: { key: string; label: string; at: number; batches: BatchInfo[] }[] = [];
    for (const b of list ?? []) {
      const last = out[out.length - 1];
      if (last && last.label === b.label && Math.abs(last.at - b.created_at) < 300) { last.batches.push(b); continue; }
      out.push({ key: b.batch_id, label: b.label, at: b.created_at, batches: [b] });
    }
    return out;
  });
  const count = (g: { batches: BatchInfo[] }) => g.batches.reduce((a, b) => a + b.ops, 0);
  const allUndone = (g: { batches: BatchInfo[] }) => g.batches.every((b) => b.undone >= b.ops);
  const rows = (g: { batches: BatchInfo[] }) => g.batches.flatMap((b) => b.items.map((it) => ({ it, b })));
  function when(ts: number) {
    const d = new Date(ts * 1000), today = new Date();
    const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
    if (d.toDateString() === today.toDateString()) return `Today, ${time}`;
    const y = new Date(today); y.setDate(today.getDate() - 1);
    if (d.toDateString() === y.toDateString()) return `Yesterday, ${time}`;
    return `${d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: d.getFullYear() === today.getFullYear() ? undefined : "numeric" })}, ${time}`;
  }
  function detail(it: OpItem): string {
    if (!it.dst) return `in ${placeOf(it.src, roots)}`;
    if (it.dst.includes("/_archive/") || it.dst.includes("\\_archive\\")) return `from ${placeOf(it.src, roots)}`;
    const from = placeOf(it.src, roots), to = placeOf(it.dst, roots);
    if (from === to) return `in ${from} → ${basename(it.dst)}`;
    return `${from} → ${to}`;
  }

  async function undo(batches: BatchInfo[]) {
    busy = true;
    let done = 0; const failed: string[] = [];
    try {
      for (const b of batches.filter((b) => b.undone < b.ops)) {
        const r = await api.undo(b.batch_id);
        done += r.done.length;
        failed.push(...r.failed.map(([, e]) => e));
      }
      if (done) app.toast(`Undid ${done} change${done === 1 ? "" : "s"}`);
      if (failed.length) app.toast(`${failed.length} couldn't be undone: ${failed[0]}`, "error");
      await load(); app.refresh();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const toggle = (k: string) => { const s = new Set(open); s.has(k) ? s.delete(k) : s.add(k); open = s; };
</script>

<div class="page">
  <div class="page-head"><div><h1>Change history</h1><p>Every rename, move and archive this PC made, newest first. Undo puts files back where they were, as long as nothing else has moved them since.</p></div></div>
  {#if list === null}<p class="faint">Loading…</p>
  {:else if list.length === 0}<div class="empty"><h2>No changes yet</h2></div>
  {:else}
    {#each groups as g (g.key)}
      {@const all = rows(g)}
      {@const shown = open.has(g.key) ? all : all.slice(0, 5)}
      <section class="gcard" class:undone={allUndone(g)}>
        <div class="gcard-head">
          <div class="grow">
            <h3>{title(g.label, count(g))} <span class="count">{when(g.at)}</span></h3>
          </div>
          <div class="actions">
            {#if allUndone(g)}<span class="faint">Undone</span>
            {:else}<button class="btn small" disabled={busy} onclick={() => undo(g.batches)}>{g.batches.length > 1 || count(g) > 1 ? "Undo all" : "Undo"}</button>{/if}
          </div>
        </div>
        <ul class="grows">
          {#each shown as { it, b }, n (b.batch_id + n)}
            <li class:gone={it.undone}>
              <span class="gpill {it.undone ? '' : v(it.kind).tone}">{it.undone ? "Undone" : v(it.kind).pill}</span>
              <div class="grow-body">
                <div class="grow-name">{basename(it.src)}</div>
                <div class="grow-meta"><span>{detail(it)}</span></div>
              </div>
              {#if g.batches.length > 1 && b.ops === 1 && !it.undone}
                <div class="grow-actions"><button class="btn small ghost" disabled={busy} onclick={() => undo([b])}>Undo</button></div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if all.length > 5}<div class="more"><button class="more-link" onclick={() => toggle(g.key)}>{open.has(g.key) ? "Show fewer" : `Show all ${all.length}`}</button></div>{/if}
        {#if count(g) > all.length}<p class="faint trunc">Only the first changes of large batches are listed.</p>{/if}
      </section>
    {/each}
  {/if}
</div>

<style>
  .undone { opacity: .7; }
  /* one Undo per group is enough at a glance; a single change's Undo shows on its row */
  .grows li .grow-actions { opacity: 0; }
  .grows li:hover .grow-actions, .grows li:focus-within .grow-actions { opacity: 1; }
  li.gone .grow-name { text-decoration: line-through; color: var(--ink-faint); }
  .more { padding: 0 14px 4px; }
  .trunc { font-size: 12.5px; margin: 0; padding: 0 14px 10px; }
</style>
