<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { partsLabel, conditionLabel, money } from "../lib/shelf";
  import type { CsvMapping, ImportRow } from "../lib/types";
  let { onclose }: { onclose: () => void } = $props();

  let text = $state("");
  let file = $state("");
  let headers = $state<string[]>([]);
  let mapping = $state<CsvMapping | null>(null);
  let rows = $state<ImportRow[] | null>(null);
  let busy = $state(false);
  /** the console for rows that don't name one */
  let rowConsole = $state<string | null>(app.filter.system ?? null);

  const FIELDS: [keyof CsvMapping, string][] = [
    ["title", "Title"], ["console", "Console"], ["region", "Region"], ["edition", "Edition"], ["parts", "Parts (CIB, loose…)"],
    ["condition", "Condition"], ["location", "Where it's kept"], ["price", "Price paid"], ["date", "Date bought"], ["notes", "Notes"],
  ];

  async function pick() {
    const p = await open({ filters: [{ name: "Spreadsheet (CSV)", extensions: ["csv", "tsv", "txt"] }] });
    if (typeof p !== "string") return;
    busy = true;
    try {
      text = await api.readCsvFile(p);
      file = p.split(/[\\/]/).pop() ?? p;
      [headers, mapping] = await api.shelfCsvColumns(text);
      await preview();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function preview() {
    busy = true;
    try { rows = await api.shelfCsvPreview(text, $state.snapshot(mapping), rowConsole); } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const ready = $derived((rows ?? []).filter((r) => r.how !== "none" && !r.duplicate && r.system && r.release));
  async function run() {
    busy = true;
    try {
      const n = await api.shelfCsvImport($state.snapshot(ready));
      app.toast(`${n.toLocaleString()} ${n === 1 ? "copy" : "copies"} added to your shelf`);
      app.refresh();
      onclose();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const HOW: Record<ImportRow["how"], { tone: string; label: string }> = {
    exact: { tone: "ok", label: "Exact" }, close: { tone: "info", label: "By title" }, typed: { tone: "plain", label: "As written" }, none: { tone: "bad", label: "Not found" },
  };
</script>

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="si-title">
    <header>
      <h2 id="si-title">Import a spreadsheet</h2>
      <p>{rows ? `${file}: ${rows.length.toLocaleString()} rows, ${ready.length.toLocaleString()} ready to add. Check which column is which; rows already on your shelf are skipped.` : "A CSV export from a spreadsheet or another tracker, one copy per row. Columns are recognised by their headings; you can correct them before importing."}</p>
    </header>
    <div class="body">
      {#if !rows}
        <div class="start"><button class="btn primary" disabled={busy} onclick={pick}>Choose a CSV file…</button></div>
      {:else if mapping}
        <label class="console"><span>Rows without a console are for</span>
          <select value={rowConsole ?? ""} onchange={(e) => { rowConsole = (e.currentTarget as HTMLSelectElement).value || null; preview(); }}>
            <option value="">No console (skip them)</option>
            {#each app.allSystems as [id, name] (id)}<option value={id}>{name}</option>{/each}
          </select>
        </label>
        <div class="map">
          {#each FIELDS as [k, label] (k)}
            <label><span>{label}</span>
              <select value={mapping[k] ?? ""} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; mapping![k] = v === "" ? null : Number(v); preview(); }}>
                <option value="">Not in the file</option>
                {#each headers as h, i}<option value={i}>{h || `Column ${i + 1}`}</option>{/each}
              </select>
            </label>
          {/each}
        </div>
        <ol class="rows">
          {#each rows as r (r.line)}
            <li class:off={r.how === "none" || r.duplicate || !r.system}>
              <span class="chip {r.duplicate ? 'plain' : HOW[r.how].tone}">{r.duplicate ? "On the shelf" : !r.system ? "No console" : HOW[r.how].label}</span>
              <div class="what">
                <div class="t">{r.title || "(no title)"} <span class="sys">{r.system ? app.systemName(r.system) : ""}</span></div>
                <div class="m">
                  {#if r.release && r.release !== r.title}<span>{r.release}</span>{/if}
                  <span>{partsLabel(r.fields)}</span>
                  {#if r.fields.condition}<span>{conditionLabel(r.fields.condition)}</span>{/if}
                  {#if r.fields.edition}<span>{r.fields.edition}</span>{/if}
                  {#if r.fields.location}<span>{r.fields.location}</span>{/if}
                  {#if r.fields.price_cents != null}<span>{money(r.fields.price_cents, app.settings?.currency)}</span>{/if}
                  {#if r.fields.bought}<span>{r.fields.bought}</span>{/if}
                </div>
              </div>
            </li>
          {/each}
        </ol>
      {/if}
    </div>
    <footer>
      {#if rows}<button class="btn ghost left" disabled={busy} onclick={pick}>Choose another file…</button>{/if}
      <button class="btn" disabled={busy} onclick={onclose}>Cancel</button>
      {#if rows}<button class="btn primary" disabled={busy || !ready.length} onclick={run}>Add {ready.length.toLocaleString()} {ready.length === 1 ? "copy" : "copies"}</button>{/if}
    </footer>
  </div>
</div>

<style>
  .start { padding: 24px 0 30px; }
  .console { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; font-size: 13px; color: var(--ink-soft); font-weight: 600; }
  .map { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 8px 14px; margin-bottom: 14px; }
  .map label { display: flex; flex-direction: column; gap: 3px; font-size: 12.5px; color: var(--ink-soft); font-weight: 600; }
  .rows { list-style: none; margin: 0 0 14px; padding: 0; }
  .rows li { display: flex; gap: 12px; align-items: flex-start; padding: 7px 0; border-top: 1px solid color-mix(in srgb, var(--seam) 60%, transparent); }
  .rows li.off { opacity: .6; }
  .rows .chip { flex: none; width: 104px; }
  .what { min-width: 0; }
  .t { font-weight: 600; }
  .sys { font-weight: 400; color: var(--ink-faint); font-size: 13px; margin-left: 6px; }
  .m { display: flex; flex-wrap: wrap; gap: 2px 12px; font-size: 13px; color: var(--ink-faint); }
</style>
