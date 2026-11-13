// Mirrors of the Rust serde types exposed by the Tauri commands.
export type Status = Record<string, string>;
export interface GameRow { id: number; system_id: string; title: string; regions: string[]; tags: string[]; exts: string[]; size: number; files: number; status: Status; dat: "verified" | "bad" | "unknown" | null; meta_state: string | null; cover: string | null; play: PlayStatus | null; score: number | null; favorite: boolean; shelf: ShelfSummary }
export interface Page<T> { rows: T[]; total: number; offset: number }
export interface GameFilter { system?: string | null; search?: string | null; region?: string | null; status_key?: string | null; status_value?: string | null; dat_status?: string | null; meta_state?: string | null; ext?: string | null; play?: string | null; owned?: "digital" | "physical" | "both" | null }
export type GameSort = "title" | "size" | "system" | "files" | "region";
export interface SystemSummary { id: string; name: string; games: number; files: number; bytes: number }
export interface ParsedName { title: string; regions: string[]; langs: string[]; rev: string | null; version: string | null; disc: number | null; tags: string[]; title_id: string | null; serial: string | null; flags: string[]; display_title?: string }
export interface Probe { kind: string; id: string | null; encrypted: boolean | null; extra: unknown }
export interface FileDetail { id: number; path: string; name: string; ext: string; size: number; crc32: string | null; sha1: string | null; dat_status: string | null; dat_game: string | null; probe: Probe | null }
export interface ActionInfo { id: string; name: string; description: string; systems: string[]; sets: string[] }
export interface ApplicableAction { action: ActionInfo; file_ids: number[] }
export interface ArtRef { kind: string; url: string }
export interface MetaCandidate { source: string; source_id: string; title: string; score: number; description: string | null; release: string | null; genres: string[]; developer: string | null; publisher: string | null; art: ArtRef[]; platforms: string[]; console: "match" | "other" | null; chosen?: boolean }
export interface GameDetail { row: GameRow; parsed: ParsedName | null; canonical: string | null; files: FileDetail[]; meta: (MetaCandidate & { candidates?: MetaCandidate[] }) | null; art: Record<string, string>; actions: ApplicableAction[]; play: PlayInfo | null; collections: [string, string, number][]; copies: ShelfCopy[] }
export type JobKind = "scan" | "hash" | "action" | "metadata" | "other";
export type JobState = "queued" | "running" | "done" | "failed" | "cancelled";
export interface JobInfo { id: number; kind: JobKind; label: string; state: JobState; done: number; total: number; message: string; error: string | null; log: string[]; started_at: number | null; finished_at: number | null }
export type JobEvent =
  | { type: "Queued"; id: number; kind: JobKind; label: string }
  | { type: "Started"; id: number }
  | { type: "Progress"; id: number; done: number; total: number; message: string }
  | { type: "Log"; id: number; line: string }
  | { type: "Finished"; id: number; ok: boolean; error: string | null };
export type Problem = { kind: "Collision" } | { kind: "TargetExists" } | { kind: "Unchanged" } | { kind: "IllegalOnWindows"; detail: string };
export interface RenameItem { file_id: number; game_id: number; src: string; dst: string; source: string; problem: Problem | null; suggestion: string | null; suggestion_source: string | null; suggestion_console: string | null; using_suggestion: boolean; game_title: string; system_id: string; role: "game" | "disc" | "update" | "dlc"; role_label: string; role_order: number; skip_key: string }
export interface Op { kind: string; src: string; dst: string | null; expect: [number, number] | null }
export interface BatchResult { batch_id: string; done: number[]; failed: [Op, string][] }
export interface OpItem { kind: string; src: string; dst: string | null; undone: boolean }
export interface BatchInfo { batch_id: string; label: string; created_at: number; ops: number; undone: number; items: OpItem[] }
export type HashLevel = "background" | "normal" | "fast" | "maximum";
export type HashScope = { kind: "All" } | { kind: "System"; value: string } | { kind: "Games"; value: number[] };
export interface HashState { level: HashLevel; paused: boolean }
export interface ArchiveEntry { id: number; system_id: string | null; path: string; original_path: string; archived_at: number; delete_after: number; reason: string | null; action: string | null; size: number }
export interface PurgeReport { deleted: number; bytes: number; failed: [string, string][] }
export interface StartupInfo { expired: ArchiveEntry[]; expired_bytes: number; synced: number; needs_setup: boolean }
export interface DupeGroup { skip_key: string; reason: "SameHash" | "EncDec" | "SameTitleDiffFormat" | "SameTitle" | "SameTitleDiffRegion" | "OlderUpdate"; system_id: string; title: string; files: number[]; names: string[]; sizes: number[]; keep: number; keep_also: number[]; reclaimable: number; proofs: Proof[]; proven_bytes: number; inside: string[]; unpacked: boolean[]; can_unpack: boolean[]; notes: string[] }
export type Proof = "Kept" | "Identical" | "SameContent" | "Superseded" | "OtherRegion" | "Different" | "Unchecked";
export type Layout = "loose" | "per-game" | "grouped" | "leave";
export interface LayoutMove { file_id: number; src: string; dst: string; size: number }
export interface LayoutSpare { file_id: number; src: string; size: number; same_as: string; proof: Proof; note: string }
export interface LayoutItem { game_id: number; title: string; problem: "Split" | "NeedsFolder" | "InOwnFolder" | "NotFiled"; target: string | null; new_folder: boolean; places: string[]; moves: LayoutMove[]; duplicates: LayoutSpare[]; left: [string, string][]; skip_key: string }
export interface SystemLayout { system_id: string; detected: Layout; layout: Layout; chosen: boolean; loose: number; per_game: number; grouped: number; split: number; items: LayoutItem[] }
export interface JunkItem { path: string; kind: "UrlShortcut" | "EmptyDir" | "RomInToolFolder" | "Executable" | "TempLeftover" | "ForeignSystem" | "ToolFolder" | "ToolFiles" | "LooseToolFolder"; system_id: string; suggestion: string; size: number; members: string[] }
export interface SystemStats { id: string; name: string; games: number; files: number; bytes: number; hashed: number; verified: number; status_counts: Record<string, number> }
export interface Stats { systems: SystemStats[]; archive_count: number; archive_bytes: number; dupe_bytes: number }
export interface Requirement { name: string; ok: boolean; hint: string }
export interface ReviewItem { game_id: number; system_id: string; title: string; file_name: string | null; candidates: MetaCandidate[]; skip_key: string }
/** suggestions you said no to: they stay gone until you bring them back */
export type SkipKind = "match" | "junk" | "dupe" | "layout" | "rename";
export interface Skipped { kind: SkipKind; key: string; label: string; system_id: string | null; at: number }
export interface ScanReport { dirs_listed: number; dirs_skipped: number; files_added: number; files_removed: number; files_changed: number; errors: [string, string][]; unknown_folders: string[] }
export type FolderMapping = { kind: "system"; system: string } | { kind: "ignored" };
export interface ApiKeys { steamgriddb: string; igdb_client_id: string; igdb_client_secret: string; screenscraper_user: string; screenscraper_password: string; screenscraper_devid: string; screenscraper_devpassword: string; rawg: string }
export interface Settings { library_roots: string[]; folder_overrides: Record<string, FolderMapping>; ignored: string[]; archive_hold_days: number; temp_dir: string | null; python: string; tools: Record<string, string>; keys: ApiKeys; scan_concurrency: number; region_order: string[]; hash_level: string; favorite_systems: string[]; folder_layouts: Record<string, string>; one_game_one_rom: boolean; theme: "system" | "light" | "dark"; currency: string; auto_update: boolean }

export interface CompletionOptions { regions: string[]; unlicensed: boolean; aftermarket: boolean; languages: string[]; source: "digital" | "physical" | "either" }
export type CompletionState = "have" | "unverified" | "missing";
export interface CompletionEntry { title: string; release: string; releases: string[]; regions: string[]; state: CompletionState; game_id: number | null; shelf: number }
export interface Completion { system: string; list: string | null; clone_list: boolean; total: number; have: number; unverified: number; missing: number; entries: CompletionEntry[] }
export interface CompletionSummary { system: string; name: string; total: number; have: number; unverified: number }
export interface RootMap { id: string; path: string; origin: string }
export interface SyncReport { pushed: number; pulled: number; applied: number; waiting: number; covers_up: number; covers_down: number; clock_skew_ms: number; settings_changed: boolean; sent: Record<string, number>; received: Record<string, number> }
export interface SyncActivity { at: number; sent: Record<string, number>; received: Record<string, number>; covers_up: number; covers_down: number; error: string | null }
export interface SyncProgress { message: string; done: number; total: number }
export interface SyncStatus { connected: boolean; url: string; device_id: string; restore_pending: boolean; offer: RootMap[]; roots: RootMap[]; last_at: number | null; last: SyncReport | null; error: string | null; running: SyncProgress | null; pending: number | null; activity: SyncActivity[] }

export interface SourceStatus { id: string; name: string; active: boolean; detail: string }

export type PlayStatus = "want" | "playing" | "finished" | "completed" | "onhold" | "dropped" | "skip";
export interface PlayInfo { status: PlayStatus | null; score: number | null; notes: string | null; started: string | null; finished: string | null; hours: number | null; favorite: boolean }
export interface CollectionSummary { id: string; name: string; notes: string | null; system: string | null; count: number; done: number; skipped: number; next: string | null }
export interface CollectionItem { pos: number; ref: string; title: string; system: string; note: string | null; game_id: number | null; cover: string | null; play: PlayInfo | null }
export interface CollectionDetail { id: string; name: string; notes: string | null; items: CollectionItem[] }
export interface PlayEntry { game_id: number | null; title: string; system: string; cover: string | null; status: PlayStatus | null; score: number | null; started: string | null; finished: string | null; hours: number | null }
export interface PlaySummary { counts: Record<string, number>; favorites: number; average_score: number | null; hours: number; playing: PlayEntry[]; finished: PlayEntry[] }
export interface ImportCandidate { game_id: number; title: string; system: string; regions: string[] }
export interface ImportEntry { title: string; system: string | null; note: string | null; how: "id" | "title" | "close" | "none"; matched: ImportCandidate | null; alternatives: ImportCandidate[] }
export interface ImportPreview { name: string; notes: string | null; entries: ImportEntry[] }

export type Condition = "mint" | "near_mint" | "very_good" | "good" | "fair" | "poor";
export interface CopyFields { edition: string | null; cart: boolean; box: boolean; manual: boolean; inserts: boolean; sealed: boolean; condition: Condition | null; location: string | null; price_cents: number | null; bought: string | null; notes: string | null }
export interface ShelfCopy extends CopyFields { id: string; game: { system: string; key: string; title: string; ids: string[] }; release: string; region: string | null; deleted: boolean }
export interface ShelfSummary { copies: number; best: string | null }
export interface ReleaseHit { release: string; title: string; regions: string[]; source: "list" | "titledb" | "catalog" }
export interface CsvMapping { title: number | null; console: number | null; region: number | null; edition: number | null; parts: number | null; condition: number | null; location: number | null; price: number | null; date: number | null; notes: number | null }
export interface ImportRow { line: number; system: string | null; title: string; release: string | null; how: "exact" | "close" | "typed" | "none"; region: string | null; fields: CopyFields; duplicate: boolean }

export interface UpdateAvailable { version: string; notes: string | null; url: string; via: "api" | "latest.json" }
export interface UpdateStatus { current: string; available: UpdateAvailable | null; can_install: boolean; ready: string | null }
