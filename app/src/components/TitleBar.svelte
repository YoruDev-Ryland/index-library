<script lang="ts">
  // The window's own frame: the system title bar is off (tauri.conf.json "decorations": false).
  // The left part continues the sidebar; the rest is a drag area; window keys on the right.
  // Invisible edges resize the window, which Linux needs once the system frame is gone.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import Mark from "./Mark.svelte";
  import { app } from "../lib/state.svelte";

  // null outside Tauri (the UI preview), so the frame never takes the app down with it
  const win = (() => { try { return getCurrentWindow(); } catch { return null; } })();
  let maximized = $state(false);

  onMount(() => {
    let off: (() => void) | undefined;
    const sync = async () => { try { maximized = (await win?.isMaximized()) ?? false; } catch {} };
    sync();
    win?.onResized(sync).then((u) => (off = u)).catch(() => {});
    return () => off?.();
  });

  type Dir = "North" | "South" | "East" | "West" | "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest";
  const edges: { d: Dir; c: string }[] = [
    { d: "North", c: "n" }, { d: "South", c: "s" }, { d: "East", c: "e" }, { d: "West", c: "w" },
    { d: "NorthEast", c: "ne" }, { d: "NorthWest", c: "nw" }, { d: "SouthEast", c: "se" }, { d: "SouthWest", c: "sw" },
  ];
  const resize = (e: PointerEvent, d: Dir) => { if (e.button === 0) win?.startResizeDragging(d as any).catch(() => {}); };
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <button class="logo" onclick={() => app.splashReplay++} aria-label="Play the startup animation" title="Index"><Mark size={18} /></button>
    <span data-tauri-drag-region>Index</span>
  </div>
  <div class="rest" data-tauri-drag-region></div>
  <div class="keys">
    <button onclick={() => win?.minimize()} aria-label="Minimize" title="Minimize"><Icon name="win-min" size={14} /></button>
    <button onclick={() => win?.toggleMaximize()} aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"}><Icon name={maximized ? "win-restore" : "win-max"} size={14} /></button>
    <button class="close" onclick={() => win?.close()} aria-label="Close" title="Close"><Icon name="close" size={14} /></button>
  </div>
</header>

{#if !maximized}
  {#each edges as e (e.c)}
    <div class="edge {e.c}" aria-hidden="true" onpointerdown={(ev) => resize(ev, e.d)}></div>
  {/each}
{/if}

<style>
  .titlebar { display: grid; grid-template-columns: 248px minmax(0, 1fr) auto; height: 34px; flex: none; background: var(--plastic); border-bottom: 1px solid var(--seam); }
  .brand { display: flex; align-items: center; gap: 9px; padding: 0 14px; background: var(--panel-2); border-right: 1px solid var(--seam); font-weight: 700; font-size: 14.5px; letter-spacing: .01em; }
  .logo { display: grid; place-items: center; margin: -4px; padding: 4px; border: 0; border-radius: var(--radius-s); background: none; cursor: default; }
  .logo:hover { background: color-mix(in srgb, var(--ink) 7%, transparent); }
  .logo:focus-visible { outline: 2px solid var(--accent); outline-offset: 0; }
  .keys { display: flex; }
  .keys button { display: grid; place-items: center; width: 44px; height: 100%; border: 0; background: none; color: var(--ink-soft); cursor: default; }
  .keys button:hover { background: var(--panel-2); color: var(--ink); }
  .keys button:active { background: color-mix(in srgb, var(--ink) 12%, transparent); }
  .keys .close:hover { background: var(--bad); color: #fff; }
  .keys button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }

  .edge { position: fixed; z-index: 1000; }
  .n, .s { left: 8px; right: 8px; height: 4px; cursor: ns-resize; }
  .n { top: 0; } .s { bottom: 0; }
  .e, .w { top: 8px; bottom: 8px; width: 4px; cursor: ew-resize; }
  .e { right: 0; } .w { left: 0; }
  .ne, .nw, .se, .sw { width: 8px; height: 8px; }
  .ne { top: 0; right: 0; cursor: nesw-resize; } .sw { bottom: 0; left: 0; cursor: nesw-resize; }
  .nw { top: 0; left: 0; cursor: nwse-resize; } .se { bottom: 0; right: 0; cursor: nwse-resize; }
</style>
