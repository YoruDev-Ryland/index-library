<script lang="ts">
  // Local artwork is served over Tauri's asset:// protocol. WebKitGTK only lets an http(s) page
  // display those images when they are requested in CORS mode, hence crossorigin="anonymous".
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  let { path, alt = "", fallback = "", class: cls = "" }: { path: string | null; alt?: string; fallback?: string; class?: string } = $props();
  let failed = $state(false);
  $effect(() => { void path; failed = false; });
  function onerror() {
    failed = true;
    invoke("ui_log", { line: `cover failed to load: ${path}` }).catch(() => {});
  }
</script>

{#if path && !failed}
  <img class={cls} src={convertFileSrc(path)} {alt} crossorigin="anonymous" loading="lazy" {onerror} />
{:else if fallback}
  <span class="ph">{fallback}</span>
{/if}

<style>
  .ph { padding: 12px; color: var(--ink-faint); font-weight: 600; text-align: center; }
</style>
