<script lang="ts" module>
  /** one pass of the wave, in ms */
  export const CYCLE_MS = 2200;
</script>

<script lang="ts">
  // The Index mark, working: a soft wave crosses the keypad on the diagonal, from the top-left key
  // to the orange home key, which takes it with a small press and settles — so every pass lands on
  // the still mark. Keys on one diagonal move together; each eases down and back with the next
  // already starting, so the motion is one continuous ripple, not steps. Only transform and
  // opacity animate. `tile` draws the black housing (the app icon); the screen and the jobs bar
  // use the bare keys on the page.
  let { size = 64, running = true, tile = false, label = "" }: { size?: number; running?: boolean; tile?: boolean; label?: string } = $props();
  const keys = [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => ({ i: r * 3 + c, d: r + c, x: 512 + (c - 1) * 196, y: 512 + (r - 1) * 196 })));
  const box = $derived(tile ? "64 64 896 896" : "242 242 540 540");
</script>

<svg class="mark-anim" class:running class:tile width={size} height={size} viewBox={box} style:--cycle="{CYCLE_MS}ms"
  role={label ? "img" : undefined} aria-label={label || undefined} aria-hidden={label ? undefined : "true"}>
  {#if tile}<rect x="64" y="64" width="896" height="896" rx="204" class="housing" />{/if}
  {#each keys as k (k.i)}
    <circle cx={k.x} cy={k.y} r="74" class="key" class:home={k.i === 8} style:--d={k.d} />
  {/each}
</svg>

<style>
  .mark-anim { display: block; flex: none; overflow: visible; --spread: 0.13s; }
  .housing { fill: #1f1f1d; }
  .key { fill: var(--mark-key, var(--ink-faint)); transform-box: fill-box; transform-origin: center; }
  .key.home { fill: var(--accent); }
  .tile .key { fill: #d3cec4; }
  .tile .key.home { fill: #ee6b2c; }

  @media (prefers-reduced-motion: no-preference) {
    .running .key {
      animation: wave var(--cycle) cubic-bezier(.45, 0, .25, 1) infinite;
      /* negative: every key starts mid-cycle in its place, so the first frame is already moving */
      animation-delay: calc(var(--d) * var(--spread) - var(--cycle));
    }
    .running .key.home { animation-name: land; }
    @keyframes wave {
      0% { transform: scale(1); opacity: 1; }
      20% { transform: scale(.58); opacity: .45; }
      44%, 100% { transform: scale(1); opacity: 1; }
    }
    /* the home key takes the wave: a press, a small rebound past its size, and rest */
    @keyframes land {
      0% { transform: scale(1); }
      18% { transform: scale(.66); }
      34% { transform: scale(1.14); }
      50%, 100% { transform: scale(1); }
    }
  }
</style>
