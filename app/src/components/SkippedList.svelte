<script lang="ts">
  // What you've said no to on this page: closed by default, each one can be brought back.
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  import type { SkipKind, Skipped } from "../lib/types";

  let { kind, noun, onchange }: { kind: SkipKind; noun: { one: string; many: string }; onchange?: () => void } = $props();
  let items = $state<Skipped[]>([]);
  let open = $state(false);

  export async function reload() { try { items = (await api.skipped(kind)) ?? []; } catch { items = []; } }
  $effect(() => { void app.revision; reload(); });

  async function bringBack(s: Skipped) {
    try { await api.unskip(kind, s.key); items = items.filter((x) => x.key !== s.key); onchange?.(); }
    catch (e) { app.fail(e); }
  }
  const when = (t: number) => new Date(t * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
</script>

{#if items.length}
  <section class="skipped">
    <button class="more-link" aria-expanded={open} onclick={() => (open = !open)}>
      {open ? "Hide" : "Show"} {items.length} {items.length === 1 ? noun.one : noun.many} you skipped
    </button>
    {#if open}
      <ul>
        {#each items as s (s.key)}
          <li>
            <span class="label">{s.label}</span>
            <span class="faint">{s.system_id ? `${app.systemName(s.system_id)} · ` : ""}{when(s.at)}</span>
            <button class="btn small ghost" onclick={() => bringBack(s)}>Bring back</button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .skipped { margin: 18px 0 8px; }
  ul { list-style: none; margin: 8px 0 0; padding: 0; border-top: 1px solid var(--seam); }
  li { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 14px; align-items: center; padding: 6px 0; border-bottom: 1px solid var(--seam); font-size: 13.5px; }
  .label { overflow-wrap: anywhere; }
  .faint { font-size: 13px; white-space: nowrap; }
</style>
