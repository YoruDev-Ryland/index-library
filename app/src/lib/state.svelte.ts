import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import type { CollectionSummary, GameFilter, GameSort, HashState, JobEvent, JobInfo, Settings, SystemSummary } from "./types";
import { errText } from "./format";

export type View = "library" | "dashboard" | "completion" | "duplicates" | "cleanup" | "archive" | "review" | "history" | "settings" | "collection";

function readGrid(): boolean {
  try { return localStorage.getItem("lo.view") === "covers"; } catch { return false; }
}

class AppState {
  view = $state<View>("library");
  systems = $state<SystemSummary[]>([]);
  allSystems = $state<[string, string][]>([]);
  settings = $state<Settings | null>(null);
  filter = $state<GameFilter>({});
  sort = $state<GameSort>("title");
  selection = $state<Set<number>>(new Set());
  focusId = $state<number | null>(null);
  /** List or Covers, remembered on this PC between restarts */
  grid = $state(readGrid());
  setGrid(on: boolean) {
    this.grid = on;
    try { localStorage.setItem("lo.view", on ? "covers" : "list"); } catch { /* storage unavailable: just this session */ }
  }
  jobsOpen = $state(false);
  /** bumped to play the startup screen again (the mark in the title bar) */
  splashReplay = $state(0);
  systemImages = $state<Record<string, string>>({});
  collections = $state<CollectionSummary[]>([]);
  /** the collection open in the "collection" view */
  collectionId = $state<string | null>(null);
  private fetchedImages = false;
  jobs = $state<JobInfo[]>([]);
  hash = $state<HashState>({ level: "background", paused: false });
  /** bumps whenever library contents may have changed; views re-query on change */
  revision = $state(0);
  toasts = $state<{ id: number; text: string; kind: "ok" | "error" }[]>([]);
  private toastId = 0;

  get systemId() { return this.filter.system ?? null; }
  systemName(id: string) { return this.allSystems.find(([i]) => i === id)?.[1] ?? id; }

  toast(text: string, kind: "ok" | "error" = "ok") {
    const id = ++this.toastId;
    this.toasts = [...this.toasts, { id, text, kind }];
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === "error" ? 9000 : 4000);
  }
  fail(e: unknown) { this.toast(errText(e), "error"); }

  async refresh() {
    try {
      [this.systems, this.hash, this.systemImages, this.collections] = await Promise.all([api.systems(), api.hashState(), api.systemImages(), api.collections()]);
      // download sidebar images for systems that don't have one yet (once per session)
      if (!this.fetchedImages && this.systems.some((s) => !this.systemImages[s.id])) { this.fetchedImages = true; api.fetchSystemImages().catch(() => {}); }
      this.revision++;
    } catch (e) { this.fail(e); }
  }

  private unlisten: Promise<UnlistenFn> | null = null;

  /** Light or dark as chosen in Settings; "system" follows the OS. */
  applyTheme(theme: string | undefined) {
    const el = document.documentElement;
    if (theme === "light" || theme === "dark") el.dataset.theme = theme; else delete el.dataset.theme;
    // remembered for index.html, which sets the ground colour before the app loads
    try { localStorage.setItem("lo.theme", theme ?? "system"); } catch {}
  }

  async init() {
    this.allSystems = await api.allSystems();
    this.settings = await api.settings();
    this.applyTheme(this.settings?.theme);
    this.jobs = await api.jobs();
    // Register the job-event listener exactly once, even if the app remounts (e.g. a dev reload);
    // a second listener would apply every event twice and duplicate jobs in the list.
    if (!this.unlisten) this.unlisten = listen<JobEvent>("job", (ev) => this.onJob(ev.payload));
    await this.unlisten;
    await this.refresh();
    // other PCs' play status and collection changes arrive through the shared journal: pick
    // them up when you come back to the window (at most once a minute)
    let last = Date.now();
    window.addEventListener("focus", () => {
      if (Date.now() - last < 60_000) return;
      last = Date.now();
      api.syncShared().then((n) => { if (n > 0) this.refresh(); }).catch(() => {});
    });
  }

  async loadCollections() { try { this.collections = await api.collections(); } catch (e) { this.fail(e); } }
  openCollection(id: string) { this.collectionId = id; this.view = "collection"; this.focusId = null; }

  private onJob(e: JobEvent) {
    const i = this.jobs.findIndex((j) => j.id === e.id);
    if (e.type === "Queued") {
      if (i >= 0) return; // already known (e.g. from jobs_list at startup)
      this.jobs = [{ id: e.id, kind: e.kind, label: e.label, state: "queued", done: 0, total: 0, message: "", error: null, log: [], started_at: null, finished_at: null }, ...this.jobs];
      return;
    }
    if (i < 0) return;
    const j = { ...this.jobs[i] };
    if (e.type === "Started") j.state = "running";
    if (e.type === "Progress") { j.done = e.done; j.total = e.total; j.message = e.message; }
    if (e.type === "Log") j.log = [...j.log.slice(-499), e.line];
    if (e.type === "Finished") {
      j.state = e.ok ? "done" : e.error === "cancelled" ? "cancelled" : "failed";
      j.error = e.error;
      if (e.ok) this.toast(`${j.label} finished`);
      else if (j.state === "failed") this.toast(`${j.label}: ${e.error}`, "error");
      this.refresh();
    }
    this.jobs[i] = j;
  }

  get runningJobs() { return this.jobs.filter((j) => j.state === "running" || j.state === "queued"); }

  isFavorite(id: string) { return this.settings?.favorite_systems?.includes(id) ?? false; }
  async toggleFavorite(id: string) {
    if (!this.settings) return;
    const favs = this.settings.favorite_systems ?? [];
    const base = $state.snapshot(this.settings);
    const next = { ...base, favorite_systems: favs.includes(id) ? favs.filter((x) => x !== id) : [...favs, id] };
    try { await api.saveSettings(next, base); this.settings = next; } catch (e) { this.fail(e); }
  }

  async setFolderLayout(system: string, layout: string | null) {
    if (!this.settings) return;
    const cur = { ...($state.snapshot(this.settings).folder_layouts ?? {}) };
    if (layout) cur[system] = layout; else delete cur[system];
    const base = $state.snapshot(this.settings);
    const next = { ...base, folder_layouts: cur };
    try { await api.saveSettings(next, base); this.settings = next; } catch (e) { this.fail(e); }
  }

  selectOnly(id: number) { this.selection = new Set([id]); this.focusId = id; }
}

export const app = new AppState();

// In development, drop the listener when this module is hot-replaced.
if (import.meta.hot) import.meta.hot.dispose(() => { app["unlisten"]?.then((f) => f()); });
