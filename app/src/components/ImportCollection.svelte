<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { aiPrompt } from "../lib/collectionFormat";
  import type { ImportCandidate, ImportPreview } from "../lib/types";
  let { onclose }: { onclose: () => void } = $props();

  let text = $state("");
  let preview = $state<ImportPreview | null>(null);
  let choices = $state<(number | null)[]>([]);
  let busy = $state(false);
  let error = $state("");

  async function check() {
    error = "";
    busy = true;
    try {
      preview = await api.collectionPreview(text);
      choices = preview.entries.map((e) => e.matched?.game_id ?? null);
    } catch (e) { error = String(e); } finally { busy = false; }
  }
  async function openFile() {
    const p = await open({ filters: [{ name: "Collection", extensions: ["json", "txt"] }] });
    if (typeof p !== "string") return;
    try { text = await api.readTextFile(p); await check(); } catch (e) { error = String(e); }
  }
  async function copyPrompt() {
    try { await navigator.clipboard.writeText(aiPrompt(app.allSystems)); app.toast("Copied — paste it into an AI, describe the collection you want, then paste its reply here"); }
    catch { app.toast("Couldn't reach the clipboard", "error"); }
  }
  async function create() {
    busy = true;
    try {
      const id = await api.collectionImport(text, choices);
      await app.loadCollections();
      app.openCollection(id);
      const missing = choices.filter((c) => c === null).length;
      app.toast(`Created “${preview!.name}”${missing ? ` — ${missing} game${missing === 1 ? " isn't" : "s aren't"} in your library yet and will be found when added` : ""}`);
      onclose();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  const label = (c: ImportCandidate) => `${c.title}${c.regions.length ? ` (${c.regions.join(", ")})` : ""} · ${app.systemName(c.system)}`;
  const howText = { id: "Exact match", title: "Same title", close: "Close title — check it", none: "Not in your library" } as const;
  const howTone = { id: "ok", title: "ok", close: "warn", none: "" } as const;
  const counts = $derived(preview ? { found: choices.filter((c) => c !== null).length, total: preview.entries.length } : null);
</script>

<div class="modal-backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && !busy && onclose()}>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="ic-title">
    <header>
      <h2 id="ic-title">{preview ? `Import “${preview.name}”` : "Import a collection"}</h2>
      {#if !preview}
        <p>Paste a collection file — or an AI's reply containing one — or open a .json file. To have an AI make one, copy the prompt, paste it into the AI with what you want, and paste its reply here.</p>
      {:else}
        <p>{counts!.found} of {counts!.total} games found in your library. Check the close matches; games you don't have stay in the collection by name and are found once you add them.</p>
      {/if}
    </header>
    <div class="body">
      {#if !preview}
        <textarea class="paste" rows="14" bind:value={text} placeholder={'{\n  "name": "Atelier, in release order",\n  "games": [\n    { "title": "Atelier Rorona: The Alchemist of Arland DX", "system": "Switch" }\n  ]\n}'} aria-label="Collection JSON"></textarea>
        {#if error}<p class="err">{error}</p>{/if}
      {:else}
        {#if preview.notes}<p class="notes">{preview.notes}</p>{/if}
        <ol class="entries">
          {#each preview.entries as e, i}
            <li>
              <span class="n">{i + 1}</span>
              <div class="what">
                <div class="t">{e.title}</div>
                <div class="sub">{e.system ? app.systemName(e.system) : "System not recognised"}{e.note ? ` · ${e.note}` : ""}</div>
              </div>
              <div class="pick">
                <span class="gpill {howTone[e.how]}">{choices[i] === null ? "Not in your library" : choices[i] === e.matched?.game_id ? howText[e.how] : "Your choice"}</span>
                {#if e.matched || e.alternatives.length}
                  <select aria-label="Match for {e.title}" value={choices[i] ?? ""} onchange={(ev) => { const v = (ev.currentTarget as HTMLSelectElement).value; choices[i] = v ? Number(v) : null; }}>
                    {#each [e.matched, ...e.alternatives].filter(Boolean) as c (c!.game_id)}<option value={c!.game_id}>{label(c!)}</option>{/each}
                    <option value="">Not this — keep it by name</option>
                  </select>
                {/if}
              </div>
            </li>
          {/each}
        </ol>
      {/if}
    </div>
    <footer>
      {#if !preview}
        <button class="btn ghost left" onclick={copyPrompt}>Copy AI prompt</button>
        <button class="btn" onclick={openFile}>Open file…</button>
        <button class="btn ghost" onclick={onclose}>Cancel</button>
        <button class="btn primary" disabled={busy || !text.trim()} onclick={check}>{busy ? "Reading…" : "Check matches"}</button>
      {:else}
        <button class="btn ghost left" onclick={() => (preview = null)}>Back</button>
        <button class="btn ghost" onclick={onclose}>Cancel</button>
        <button class="btn primary" disabled={busy} onclick={create}>Create collection</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .paste { width: 100%; font-family: ui-monospace, "JetBrains Mono", monospace; font-size: 12.5px; line-height: 1.5; margin: 4px 0 8px; }
  .err { color: var(--bad); margin: 0 0 8px; }
  .notes { color: var(--ink-soft); margin: 0 0 10px; }
  .entries { list-style: none; margin: 0 0 12px; padding: 0; }
  .entries li { display: grid; grid-template-columns: 28px minmax(0, 1fr) minmax(240px, 420px); gap: 12px; align-items: center; padding: 8px 0; border-top: 1px solid color-mix(in srgb, var(--seam) 55%, transparent); }
  .n { color: var(--ink-faint); font-weight: 600; text-align: right; font-variant-numeric: tabular-nums; }
  .t { font-weight: 600; overflow-wrap: anywhere; }
  .sub { font-size: 13px; color: var(--ink-soft); overflow-wrap: anywhere; }
  .pick { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .pick select { width: 100%; font-size: 13px; }
  footer .left { margin-right: auto; }
  :global(.modal):has(.entries) { width: min(1080px, calc(100vw - 48px)); }
</style>
