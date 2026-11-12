<script lang="ts">
  // The Index mark: a Braun-black tile with nine keys; the last one is the signal orange,
  // like the "=" key on the ET 66 — the one entry you picked out of the index.
  let { size = 18 }: { size?: number } = $props();
  const keys = [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => ({ x: 512 + (c - 1) * 196, y: 512 + (r - 1) * 196, last: r === 2 && c === 2 })));
</script>

<svg class="mark" width={size} height={size} viewBox="64 64 896 896" aria-hidden="true">
  <rect x="64" y="64" width="896" height="896" rx="204" fill="#1f1f1d" />
  {#each keys as k}<circle cx={k.x} cy={k.y} r="74" fill={k.last ? "#ee6b2c" : "#d3cec4"} />{/each}
</svg>

<style>
  .mark { flex: none; display: block; }
</style>
