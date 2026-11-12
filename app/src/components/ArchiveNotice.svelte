<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { basename, bytes } from "../lib/format";
  import type { ArchiveEntry } from "../lib/types";
  let { entries, bytes: total, onclose }: { entries: ArchiveEntry[]; bytes: number; onclose: () => void } = $props();
  let busy = $state(false);
  const hold = $derived(app.settings?.archive_hold_days ?? 7);

  async function del() {
    busy = true;
    try {
      const r = await api.archivePurgeExpired(entries.map((e) => e.id));
      app.toast(r.deleted < entries.length ? `Deleted ${r.deleted}; ${entries.length - r.deleted - r.failed.length} were kept longer or restored on another PC` : `Deleted ${r.deleted} archived originals, freed ${bytes(r.bytes)}`);
      if (r.failed.length) app.toast(`${r.failed.length} could not be deleted: ${r.failed[0][1]}`, "error");
      onclose();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
  async function keep() {
    busy = true;
    try { await api.archiveExtend(entries.map((e) => e.id), hold); app.toast(`Kept for ${hold} more days`); onclose(); }
    catch (e) { app.fail(e); } finally { busy = false; }
  }
</script>

<div class="modal-backdrop">
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="an-title" style="width: min(620px, calc(100vw - 48px))">
    <header>
      <h2 id="an-title">{entries.length} archived original{entries.length === 1 ? " is" : "s are"} past {entries.length === 1 ? "its" : "their"} hold</h2>
      <p>These were replaced by decrypted or converted copies and have been kept for at least {hold} days. Deleting them frees {bytes(total)}.</p>
    </header>
    <div class="body">
      <ul>
        {#each entries.slice(0, 12) as e}<li><span>{basename(e.path)}</span><span class="faint">{bytes(e.size)}</span></li>{/each}
        {#if entries.length > 12}<li class="faint">and {entries.length - 12} more</li>{/if}
      </ul>
    </div>
    <footer>
      <button class="btn ghost" disabled={busy} onclick={onclose}>Remind me later</button>
      <button class="btn" disabled={busy} onclick={keep}>Keep {hold} more days</button>
      <button class="btn primary" disabled={busy} onclick={del}>Delete now</button>
    </footer>
  </div>
</div>

<style>
  ul { list-style: none; margin: 0 0 12px; padding: 0; }
  li { display: flex; justify-content: space-between; gap: 12px; padding: 4px 0; border-bottom: 1px solid color-mix(in srgb, var(--seam) 50%, transparent); }
</style>
