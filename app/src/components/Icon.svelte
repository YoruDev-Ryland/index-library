<script lang="ts">
  // A small drawn icon set: one 16px grid, one 1.6px stroke, round joins — so every icon sits on
  // the same baseline and weight as the text beside it.
  type Name = "close" | "edit" | "up" | "down" | "grip" | "star" | "star-filled" | "plus" | "chevron" | "win-min" | "win-max" | "win-restore";
  let { name, size = 16, label = "" }: { name: Name; size?: number; label?: string } = $props();
</script>

<svg class="icon" width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"
  role={label ? "img" : undefined} aria-label={label || undefined} aria-hidden={label ? undefined : "true"}>
  {#if name === "close"}<path d="M4 4l8 8M12 4l-8 8" />
  {:else if name === "edit"}<path d="M10.2 3.2l2.6 2.6L6 12.6 3 13l.4-3z" /><path d="M9 4.4l2.6 2.6" />
  {:else if name === "up"}<path d="M8 13V3.5M4 7.2l4-4 4 4" />
  {:else if name === "down"}<path d="M8 3v9.5M4 8.8l4 4 4-4" />
  {:else if name === "grip"}{#each [4, 8, 12] as y}<circle cx="6" cy={y} r="1.1" fill="currentColor" stroke="none" /><circle cx="10" cy={y} r="1.1" fill="currentColor" stroke="none" />{/each}
  {:else if name === "star"}<path d="M8 2l1.8 3.8 4.2.5-3.1 2.9.8 4.1L8 11.3l-3.7 2 .8-4.1L2 6.3l4.2-.5z" />
  {:else if name === "star-filled"}<path d="M8 2l1.8 3.8 4.2.5-3.1 2.9.8 4.1L8 11.3l-3.7 2 .8-4.1L2 6.3l4.2-.5z" fill="currentColor" />
  {:else if name === "plus"}<path d="M8 3.5v9M3.5 8h9" />
  {:else if name === "win-min"}<path d="M3.5 8h9" />
  {:else if name === "win-max"}<rect x="3.5" y="3.5" width="9" height="9" rx="1.2" />
  {:else if name === "win-restore"}<rect x="3.5" y="5.5" width="7" height="7" rx="1.1" /><path d="M5.5 3.5h6a1 1 0 0 1 1 1v6" />
  {:else if name === "chevron"}<path d="M4.5 6.2L8 9.8l3.5-3.6" />
  {/if}
</svg>

<style>
  .icon { display: inline-block; flex: none; vertical-align: -0.18em; }
</style>
