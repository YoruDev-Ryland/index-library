import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig(({ command }) => ({
  // In dev, each component's CSS ships inside its own compiled module. With separate CSS
  // modules, the Tauri webview could reuse a cached component module that the freshly started
  // dev server never compiled; the stylesheet request then found no compiled CSS and the
  // plugin served the raw .svelte file instead, so App's layout grid silently disappeared.
  plugins: [svelte({ emitCss: command === "build" })],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
}));
