import { mount } from "svelte";
import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-sans/400-italic.css";
import "./app.css";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app")! });

// The window starts hidden (tauri.conf.json) so it never flashes white; show it once the app has
// painted. Outside Tauri (the UI preview) there's no window to show.
requestAnimationFrame(() => requestAnimationFrame(async () => {
  try { const { getCurrentWindow } = await import("@tauri-apps/api/window"); await getCurrentWindow().show(); } catch {}
}));

