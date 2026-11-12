<script lang="ts">
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { GameRow } from "../lib/types";
  import StatusChips from "./StatusChips.svelte";
  import Cover from "./Cover.svelte";

  let rows = $state<GameRow[]>([]);
  let total = $state(0);
  const LIMIT = 600;
  $effect(() => {
    void [JSON.stringify(app.filter), app.sort, app.revision];
    api.games(app.filter, app.sort, 0, LIMIT).then((p) => { rows = p.rows; total = p.total; }).catch((e) => app.fail(e));
  });
</script>

<div class="grid">
  {#each rows as r (r.id)}
    <button class="card" class:sel={app.selection.has(r.id)} onclick={(e) => { if (e.ctrlKey || e.metaKey) { const s = new Set(app.selection); s.has(r.id) ? s.delete(r.id) : s.add(r.id); app.selection = s; app.focusId = r.id; } else app.selectOnly(r.id); }}>
      <div class="art">
        {#if r.cover}<Cover path={r.cover} />
        {:else}
          <!-- no artwork yet: a perforated face like a Braun speaker grille, with the system on a nameplate -->
          <span class="blank" aria-hidden="true"><span class="grille"></span><span class="plate">{app.systemName(r.system_id)}</span></span>
        {/if}
      </div>
      <span class="name">{r.title}</span>
      <span class="chips">{#if r.shelf.copies}<span class="chip shelf" title="On your shelf{r.files ? '' : ' (no files)'}">{r.shelf.best ?? "Shelf"}</span>{/if}<StatusChips status={r.status} dat={r.dat} compact /></span>
    </button>
  {/each}
  {#if total > LIMIT}<p class="faint more">Showing the first {LIMIT} of {total.toLocaleString()}. Narrow the filter or switch to the list to see everything.</p>{/if}
</div>

<style>
  .grid { height: 100%; overflow: auto; display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 18px 14px; padding: 16px; align-content: start; }
  .card { border: 0; background: none; padding: 0; text-align: left; cursor: pointer; display: flex; flex-direction: column; gap: 6px; }
  .art { position: relative; aspect-ratio: 2 / 3; border-radius: var(--radius-s); overflow: hidden; background: var(--panel-2); border: 1px solid var(--seam); display: grid; place-items: center; }
  .card.sel .art { outline: 3px solid var(--accent); outline-offset: 1px; }
  .blank { position: absolute; inset: 0; display: flex; flex-direction: column; gap: 12px; padding: 14px 14px 12px; background: var(--panel); }
  /* an even field of perforations, set inside the face like the SK 4's grille */
  .grille { flex: 1; border-radius: 2px; background-image: radial-gradient(circle, color-mix(in srgb, var(--ink) 16%, transparent) 1.35px, transparent 1.6px); background-size: 8px 8px; background-position: center; }
  .plate { font-size: 11.5px; font-weight: 500; color: var(--ink-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .card:hover .grille { background-image: radial-gradient(circle, color-mix(in srgb, var(--ink) 24%, transparent) 1.35px, transparent 1.6px); }
  .art :global(img) { width: 100%; height: 100%; object-fit: cover; }
  .name { font-weight: 500; line-height: 1.2; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .chips { display: flex; gap: 4px; min-height: 20px; }
  .more { grid-column: 1 / -1; }
</style>
