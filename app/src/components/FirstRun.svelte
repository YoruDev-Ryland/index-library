<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { app } from "../lib/state.svelte";
  let { ondone }: { ondone: () => void } = $props();
  let path = $state("");
  let busy = $state(false);

  async function browse() {
    const p = await open({ directory: true, title: "Choose your game library folder" });
    if (typeof p === "string") path = p;
  }
  async function start() {
    if (!path.trim()) return;
    busy = true;
    try {
      const s = await api.settings();
      const base = structuredClone(s);
      s.library_roots = [path.trim()];
      await api.saveSettings(s, base);
      app.settings = s;
      ondone();
    } catch (e) { app.fail(e); } finally { busy = false; }
  }
</script>

<div class="first">
  <div class="card">
    <h1>Where is your game library?</h1>
    <p class="muted">Pick the folder that holds one subfolder per system, such as “Nintendo - 3DS” or “Sony - PS1”. A network share works too. The first scan only reads folder listings, so it takes seconds rather than hours.</p>
    <div class="row">
      <input type="text" bind:value={path} placeholder="/mnt/nas/Games" />
      <button class="btn" onclick={browse}>Browse…</button>
    </div>
    <button class="btn primary" disabled={!path.trim() || busy} onclick={start}>Scan this library</button>
  </div>
</div>

<style>
  .first { height: 100%; display: grid; place-items: center; padding: 24px; }
  .card { width: min(560px, 100%); display: flex; flex-direction: column; gap: 14px; }
  .row { display: flex; gap: 8px; }
  .row input { flex: 1; }
  .card > .btn { align-self: flex-start; }
</style>
