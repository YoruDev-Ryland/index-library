<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { CONDITIONS, EDITIONS, partsLabel } from "../lib/shelf";
  import type { CopyFields } from "../lib/types";

  /** The copy being filled in; changed in place. */
  let { fields = $bindable() }: { fields: CopyFields } = $props();
  let locations = $state<string[]>([]);
  onMount(() => { api.shelfLocations().then((v) => (locations = v)).catch(() => {}); });
  const uid = Math.random().toString(36).slice(2, 8);

  // the price as typed, in the currency's units
  let price = $state(fields.price_cents != null ? (fields.price_cents / 100).toFixed(2) : "");
  $effect(() => { const n = Number(price.replace(",", ".")); fields.price_cents = price.trim() && Number.isFinite(n) && n >= 0 ? Math.round(n * 100) : null; });
  const text = (k: "edition" | "location" | "notes", v: string) => { fields[k] = v.trim() ? v : null; };
</script>

<div class="copyform">
  <span class="lbl">What you have</span>
  <div class="parts">
    <label><input type="checkbox" bind:checked={fields.cart} /> Cart or disc</label>
    <label><input type="checkbox" bind:checked={fields.box} /> Box</label>
    <label><input type="checkbox" bind:checked={fields.manual} /> Manual</label>
    <label><input type="checkbox" bind:checked={fields.inserts} /> Inserts</label>
    <label><input type="checkbox" bind:checked={fields.sealed} /> Sealed</label>
    <span class="chip plain">{partsLabel(fields)}</span>
  </div>

  <label class="lbl" for="ed-{uid}">Edition</label>
  <input id="ed-{uid}" type="text" list="eds-{uid}" placeholder="Original release" value={fields.edition ?? ""} oninput={(e) => text("edition", (e.currentTarget as HTMLInputElement).value)} />
  <datalist id="eds-{uid}">{#each EDITIONS as e}<option value={e}></option>{/each}</datalist>

  <label class="lbl" for="cd-{uid}">Condition</label>
  <select id="cd-{uid}" bind:value={fields.condition}>
    <option value={null}>Not set</option>
    {#each CONDITIONS as c}<option value={c.id}>{c.label}</option>{/each}
  </select>

  <label class="lbl" for="lo-{uid}">Where it's kept</label>
  <input id="lo-{uid}" type="text" list="los-{uid}" placeholder="Shelf 3, the blue box…" value={fields.location ?? ""} oninput={(e) => text("location", (e.currentTarget as HTMLInputElement).value)} />
  <datalist id="los-{uid}">{#each locations as l}<option value={l}></option>{/each}</datalist>

  <span class="lbl">Paid</span>
  <div class="paid">
    <label class="money"><input type="text" inputmode="decimal" placeholder="0.00" bind:value={price} aria-label="Price paid" /><span>{app.settings?.currency ?? "USD"}</span></label>
    <label class="on"><span>on</span><input type="date" value={fields.bought ?? ""} onchange={(e) => (fields.bought = (e.currentTarget as HTMLInputElement).value || null)} aria-label="Date bought" /></label>
  </div>

  <label class="lbl" for="no-{uid}">Notes</label>
  <textarea id="no-{uid}" rows="2" placeholder="Label wear, the seller, a missing insert…" value={fields.notes ?? ""} oninput={(e) => text("notes", (e.currentTarget as HTMLTextAreaElement).value)}></textarea>
</div>

<style>
  .copyform { display: grid; grid-template-columns: 120px minmax(0, 1fr); gap: 10px 14px; align-items: center; }
  .lbl { font-size: 13px; color: var(--ink-soft); font-weight: 600; }
  .parts { display: flex; flex-wrap: wrap; gap: 6px 14px; align-items: center; }
  .parts label { display: inline-flex; gap: 6px; align-items: center; cursor: pointer; }
  .paid { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; }
  .money { display: inline-flex; align-items: center; gap: 6px; }
  .money input { width: 110px; text-align: right; font-variant-numeric: tabular-nums; }
  .money span, .on span { color: var(--ink-soft); font-size: 13px; }
  .on { display: inline-flex; align-items: center; gap: 6px; }
  textarea { resize: vertical; }
</style>
