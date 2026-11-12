<script lang="ts">
  // Fetches a candidate's preview image only once it scrolls into view (previews cost a
  // rate-limited API call each for SteamGridDB results).
  import { api } from "../lib/api";
  import type { MetaCandidate } from "../lib/types";
  let { candidate }: { candidate: MetaCandidate } = $props();
  let url = $state<string | null>(null);
  let phase = $state<"idle" | "loading" | "done">("idle");
  let el: HTMLDivElement;

  $effect(() => {
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting) && phase === "idle") {
        phase = "loading";
        api.candidatePreview(candidate).then((u) => (url = u)).catch(() => {}).finally(() => (phase = "done"));
        io.disconnect();
      }
    }, { rootMargin: "200px" });
    io.observe(el);
    return () => io.disconnect();
  });
</script>

<div class="thumb" bind:this={el}>
  {#if url}<img src={url} alt="" referrerpolicy="no-referrer" />
  {:else}<span class="faint">{phase === "done" ? "No image" : ""}</span>{/if}
</div>

<style>
  .thumb { width: 100%; aspect-ratio: 2 / 3; border-radius: var(--radius-s); background: var(--panel-2); border: 1px solid var(--seam); overflow: hidden; display: grid; place-items: center; font-size: 12.5px; }
  img { width: 100%; height: 100%; object-fit: cover; }
</style>
