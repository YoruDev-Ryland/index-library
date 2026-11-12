// Dev-only: run the UI in a plain browser with mocked Tauri IPC, using JSON snapshots from
// `lo --json ...` placed in public/preview-data/. Open http://localhost:1420/preview.html
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
mockWindows("main");
// local preview: "file paths" are plain URLs served by vite
(window as any).__TAURI_INTERNALS__.convertFileSrc = (p: string) => p;
// Headless screenshots fire before web fonts arrive; load them synchronously for the preview.
for (const w of [400, 500, 600]) {
  const x = new XMLHttpRequest();
  x.open("GET", `/node_modules/@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-${w}-normal.woff2`, false);
  x.overrideMimeType("text/plain; charset=x-user-defined");
  x.send();
  const b = new Uint8Array(x.responseText.length);
  for (let i = 0; i < b.length; i++) b[i] = x.responseText.charCodeAt(i) & 0xff;
  document.fonts.add(new FontFace("IBM Plex Sans", b.buffer, { weight: String(w) }));
}

import games from "../public/preview-data/games.json";
import stats from "../public/preview-data/stats.json";
import dupes from "../public/preview-data/dupes.json";
import junk from "../public/preview-data/junk.json";
import layout from "../public/preview-data/layout.json";
import review from "../public/preview-data/review.json";
import archiveList from "../public/preview-data/archive.json";
import historyList from "../public/preview-data/history.json";
import details from "../public/preview-data/details.json";
import colls from "../public/preview-data/collections.json";
import collsMany from "../public/preview-data/collections-many.json";
const rows: any[] = (games as any)?.rows ?? [];
const names: Record<string, string> = Object.fromEntries(((stats as any)?.systems ?? []).map((s: any) => [s.id, s.name]));
const params = new URLSearchParams(location.search);

mockIPC((cmd, args: any) => {
  switch (cmd) {
    case "startup": return { expired: [], expired_bytes: 0, synced: 0, needs_setup: params.has("firstrun") };
    case "get_settings": return { library_roots: ["/mnt/nas/Games"], folder_overrides: {}, ignored: ["Emulator Software", "hearto", "3DS-Decrypt", "Decryptor (Old)", "PSPDecrypter", "_archive", ".index-library"], archive_hold_days: 7, temp_dir: null, python: "python3", tools: {}, keys: { steamgriddb: "", igdb_client_id: "", igdb_client_secret: "", screenscraper_user: "", screenscraper_password: "", screenscraper_devid: "", screenscraper_devpassword: "", rawg: "" }, scan_concurrency: 16, region_order: ["USA", "World", "Europe", "Japan"], hash_level: "background", favorite_systems: ["n3ds", "switch", "psvita"], folder_layouts: {}, one_game_one_rom: true, theme: (params.get("theme") ?? "system") as any };
    case "settings_path": return "~/.config/index-library/settings.toml";
    case "all_systems": return Object.entries(names);
    case "systems": return ((stats as any)?.systems ?? []).map((s: any) => ({ id: s.id, name: s.name, games: s.games, files: s.files, bytes: s.bytes }));
    case "hash_state": return { level: "background", paused: false };
    case "jobs_list": return params.has("jobs") ? [{ id: 1, kind: "hash", label: "Look up 15 games", state: "running", done: 6, total: 15, message: "Pokemon Colosseum", error: null, log: ["✓ ATV - Quad Power Racing 2 → ATV: Quad Power Racing 2 (steamgriddb)", "✓ Final Fantasy Crystal Chronicles → Final Fantasy: Crystal Chronicles (steamgriddb)", "? Medal of Honor - European Assault: 4 possible matches — added to Match review", "✓ Medal of Honor - Frontline → Medal of Honor: Frontline (steamgriddb)", "– Some Homebrew: no match found", "✓ Metroid Prime → Metroid Prime (steamgriddb)"], started_at: 0, finished_at: null }] : [];
    case "games": {
      if (params.get("focus") && !rows.some((g) => g.id === Number(params.get("focus")))) { const r = (details as any[]).find((x) => x.row.id === Number(params.get("focus"))); if (r) rows.unshift(r.row); }
      const f = args.filter ?? {};
      let r = rows.filter((g) => (!f.system || g.system_id === f.system) && (!f.search || g.title.toLowerCase().includes(String(f.search).toLowerCase())) && (!f.status_key || g.status[f.status_key] === f.status_value));
      return { rows: r.slice(args.offset, args.offset + args.limit), total: r.length, offset: args.offset };
    }
    case "game_ids_for": return rows.map((g) => g.id);
    case "fetch_game_art": return {};
    case "collections": return params.has("many") ? (collsMany as any).collections : (colls as any).collections;
    case "play_summary": { const it = (colls as any).detail[0].items; const e = (i: any) => ({ game_id: i.game_id, title: i.title, system: i.system, cover: i.cover, status: i.play?.status ?? null, score: i.play?.score ?? null, started: i.play?.started ?? null, finished: i.play?.finished ?? null, hours: i.play?.hours ?? null });
      return { counts: { playing: 2, finished: 14, completed: 3, want: 22, skip: 4, dropped: 1 }, favorites: 6, average_score: 7.8, hours: 412, playing: it.filter((i: any) => i.play?.status === "playing").map(e).concat([e({ ...it[6], play: { status: "playing", started: "2026-11-05" } })]), finished: it.filter((i: any) => i.play?.status === "finished").map(e) }; }
    case "collection": return (colls as any).detail[0];
    case "set_play": return args.play;
    case "add_to_collection": case "set_collection_items": case "update_collection": return 1;
    case "create_collection": return "c1";
    case "reveal": return null;
    case "sources_status": return [];
    case "game_detail": {
      const real = (details as any[]).find((x) => x.row.id === args.id);
      if (real) return real;
      const row = rows.find((g) => g.id === args.id);
      return { row, parsed: { title: row.title, regions: row.regions, langs: [], rev: null, version: null, disc: null, tags: row.tags, title_id: null, serial: null, flags: [] }, canonical: null,
        files: [{ id: 1, path: `/mnt/nas/Games/${names[row.system_id]}/${row.title}.${row.exts[0]}`, name: `${row.title}.${row.exts[0]}`, ext: row.exts[0], size: row.size, crc32: null, sha1: null, dat_status: null, dat_game: null, probe: row.status.decrypted ? { kind: "N3ds", id: "0004000000030800", encrypted: row.status.decrypted === "false", extra: null } : null }],
        meta: null, art: {}, actions: row.status.decrypted === "false" ? [{ action: { id: "n3ds.decrypt_3ds", name: "Decrypt .3ds", description: "Decrypt a .3ds/.cci with decrypt_3ds.py.", systems: ["n3ds"], sets: ["decrypted"] }, file_ids: [1] }] : [] };
    }
    case "actions_for_games": return [];
    case "stats": return stats;
    case "system_images": return Object.fromEntries(Object.keys(names).map((id) => [id, `/preview-data/systems/${id}.${["atari5200","atari7800","genesis","lynx","ngpc","pc","sega32x","sg1000","vectrex"].includes(id) ? "jpg" : "png"}`]));
    case "dupes": return dupes ?? [];
    case "junk": return junk ?? [];
    case "layout": return layout ?? [];
    case "archive_list": return archiveList ?? [];
    case "history": return historyList ?? [];
    case "review_list": return review;
    case "candidate_preview": { const h = [...String(args.candidate.source_id)].reduce((a, c) => a + c.charCodeAt(0), 0) % 360; return `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="60" height="90"><rect width="60" height="90" fill="hsl(${h} 45% 45%)"/></svg>`)}`; }
    case "actions": return [];
    case "requirements": return [];
    case "plan_renames": return [];
    case "plugin:event|listen": return 1;
    default: return null;
  }
});
import "./main";
import { app } from "./lib/state.svelte";
if (params.get("view")) app.view = params.get("view") as any;
if (params.get("coll")) app.collectionId = params.get("coll");
if (params.get("system")) app.filter = { system: params.get("system") };
if (params.get("filter")) app.filter = { ...app.filter, ...JSON.parse(params.get("filter")!) };
if (params.get("focus")) app.focusId = Number(params.get("focus"));
if (params.has("drawer")) app.jobsOpen = true;
if (params.has("grid")) app.grid = true;
if (params.get("sel")) app.selection = new Set(params.get("sel")!.split(",").map(Number));
