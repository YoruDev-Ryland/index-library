//! The one object front ends (Tauri app, CLI) talk to. Thread-safe; long work runs as jobs.
use crate::actions::{self, ActionInfo, Requirement};
use crate::archive::{self, ArchiveEntry, PurgeReport};
use crate::config::{machine_id, project_dirs, Settings};
use crate::db::Db;
use crate::dupes::DupeGroup;
use crate::fsx::{LocalFs, Transport};
use crate::hash::{HashLevel, Throttle};
use crate::jobs::{JobId, JobKind, JobManager, Sink};
use crate::junk::JunkItem;
use crate::metadata::{self, http::Http, MatchResult, MetaCandidate};
use crate::ops::{BatchInfo, BatchResult};
use crate::parse::ParsedName;
use crate::rename::RenameItem;
use crate::scan::{ScanOpts, ScanReport};
use crate::shared::journal::Journal;
use crate::stats::Stats;
use crate::systems::registry;
use crate::util::p2s;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameFilter {
    pub system: Option<String>,
    pub search: Option<String>,
    pub region: Option<String>,
    pub status_key: Option<String>,
    /// with status_key: required value; "!x" = anything but x (including unset)
    pub status_value: Option<String>,
    pub dat_status: Option<String>,
    pub meta_state: Option<String>,
    pub ext: Option<String>,
    /// your play status ("none" = not set, "favorite" = marked favourite)
    #[serde(default)]
    pub play: Option<String>,
    /// what you have of it: "digital" (files), "physical" (copies on the shelf), "both"
    #[serde(default)]
    pub owned: Option<String>,
}

/// A game's copies on the shelf, for the list.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ShelfSummary {
    pub copies: i64,
    /// the best of them: Sealed, CIB, Boxed, Loose, Box only, Manual only
    pub best: Option<String>,
}

/// A release to pick when adding a copy.
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseHit {
    pub release: String,
    pub title: String,
    pub regions: Vec<String>,
    /// where it came from: "list" (No-Intro / Redump), "titledb", "catalog" (IGDB)
    pub source: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameSort { #[default] Title, Size, System, Files, Region }

#[derive(Debug, Clone, Serialize)]
pub struct GameRow {
    pub id: i64,
    pub system_id: String,
    pub title: String,
    pub regions: Vec<String>,
    pub tags: Vec<String>,
    pub exts: Vec<String>,
    pub size: i64,
    pub files: i64,
    pub status: BTreeMap<String, String>,
    pub dat: Option<String>,
    pub meta_state: Option<String>,
    pub cover: Option<String>,
    /// your play status and score
    pub play: Option<String>,
    pub score: Option<i64>,
    pub favorite: bool,
    /// your physical copies of it
    pub shelf: ShelfSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page<T> { pub rows: Vec<T>, pub total: i64, pub offset: i64 }

#[derive(Debug, Clone, Serialize)]
pub struct FileDetail { pub id: i64, pub path: String, pub name: String, pub ext: String, pub size: i64, pub crc32: Option<String>, pub sha1: Option<String>, pub dat_status: Option<String>, pub dat_game: Option<String>, pub probe: Option<serde_json::Value> }

#[derive(Debug, Clone, Serialize)]
pub struct ApplicableAction { pub action: ActionInfo, pub file_ids: Vec<i64> }

#[derive(Debug, Clone, Serialize)]
pub struct GameDetail {
    pub row: GameRow,
    pub parsed: Option<ParsedName>,
    pub canonical: Option<String>,
    pub files: Vec<FileDetail>,
    pub meta: Option<serde_json::Value>,
    pub art: BTreeMap<String, String>,
    pub actions: Vec<ApplicableAction>,
    /// your play status, and the collections it's in (id, name, position)
    pub play: Option<crate::userdata::PlayInfo>,
    pub collections: Vec<(String, String, i64)>,
    /// your physical copies of it
    pub copies: Vec<crate::shelf::Copy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemSummary { pub id: String, pub name: String, pub games: i64, pub files: i64, pub bytes: i64 }

#[derive(Debug, Clone, Serialize)]
pub struct StartupInfo { pub expired: Vec<ArchiveEntry>, pub expired_bytes: i64, pub synced: usize, pub needs_setup: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum HashScope { All, System(String), Games(Vec<i64>) }

#[derive(Debug, Clone, Serialize)]
pub struct HashState { pub level: HashLevel, pub paused: bool }

#[derive(Debug, Clone, Serialize)]
pub struct ReviewItem { pub game_id: i64, pub system_id: String, pub title: String, pub file_name: Option<String>, pub candidates: Vec<MetaCandidate>,
    /// "<system>:<group key>": skipping a game's match keeps it out of review until you look it up yourself
    pub skip_key: String }

/// Human-readable name of a metadata source id.
pub fn source_label(id: &str) -> &str {
    match id {
        "steamgriddb" => "SteamGridDB", "igdb" => "IGDB", "screenscraper" => "ScreenScraper",
        "titledb-switch" => "Switch titledb", "titledb-psp" => "PSP title list", other => other,
    }
}

pub struct Library {
    settings: RwLock<Settings>,
    settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    db_path: PathBuf,
    pub fs: Arc<dyn Transport>,
    pub machine: String,
    pub jobs: JobManager,
    pub throttle: Throttle,
    http: Arc<Http>,
    last_scan: Mutex<Option<ScanReport>>,
    /// the app has started (`startup`): fetching game lists on its own is on from then
    started: std::sync::atomic::AtomicBool,
    sync: SyncRuntime,
}

/// The sync server connection's moving parts: one sync at a time, a nudge after local
/// changes, and how the last one went.
#[derive(Default)]
struct SyncRuntime {
    lock: Mutex<()>,
    kick: Mutex<Option<std::time::Instant>>,
    /// the sync under way: what it's doing
    running: Mutex<Option<SyncProgress>>,
    /// a sync job is queued or running
    job: AtomicBool,
    looping: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress { pub message: String, pub done: u64, pub total: u64 }

/// The drawer label of a sync.
pub const SYNC_JOB: &str = "Sync with the server";

/// What Settings → Sync shows.
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub connected: bool,
    pub url: String,
    pub device_id: String,
    /// connected, waiting for the library folders to be mapped to restore from the server
    pub restore_pending: bool,
    /// the server's library folders offered for that restore (`path` empty)
    pub offer: Vec<crate::config::RootMap>,
    pub roots: Vec<crate::config::RootMap>,
    /// when the last sync ended, ms
    pub last_at: Option<i64>,
    pub last: Option<crate::sync::run::SyncReport>,
    pub error: Option<String>,
    /// a sync under way
    pub running: Option<SyncProgress>,
    /// changes here not yet on the server, as last counted
    pub pending: Option<usize>,
    /// what syncs sent and received, newest first
    pub activity: Vec<crate::sync::run::Activity>,
}

/// What this PC is called on the server's device list.
fn device_name() -> String {
    let h = std::env::var("COMPUTERNAME").ok().or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    format!("Index on {}", h.unwrap_or_else(|| "a PC".into()))
}

impl Library {
    /// Standard locations (platform config/data/cache dirs).
    pub fn open(sink: Sink) -> anyhow::Result<Arc<Library>> {
        let d = project_dirs();
        Self::open_with(Settings::default_path(), d.data_dir().to_path_buf(), d.cache_dir().to_path_buf(), sink)
    }
    /// Everything under one directory (portable mode, tests).
    pub fn open_at(home: &Path, sink: Sink) -> anyhow::Result<Arc<Library>> {
        Self::open_with(home.join("settings.toml"), home.join("data"), home.join("cache"), sink)
    }
    pub fn open_with(settings_path: PathBuf, data_dir: PathBuf, cache_dir: PathBuf, sink: Sink) -> anyhow::Result<Arc<Library>> {
        let settings = Settings::load(&settings_path)?;
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
        let db_path = data_dir.join("library.db");
        Db::open(&db_path)?; // migrate once up front
        let throttle = Throttle::new(HashLevel::parse(&settings.hash_level).unwrap_or(HashLevel::Background));
        Ok(Arc::new(Library {
            machine: machine_id(&data_dir)?, settings: RwLock::new(settings), settings_path, data_dir, cache_dir, db_path,
            fs: Arc::new(LocalFs), jobs: JobManager::new(sink), throttle, http: Arc::new(Http::new()), last_scan: Mutex::new(None),
            started: std::sync::atomic::AtomicBool::new(false),
            sync: SyncRuntime::default(),
        }))
    }

    pub fn db(&self) -> anyhow::Result<Db> { Db::open(&self.db_path) }
    pub fn settings(&self) -> Settings { self.settings.read().unwrap().clone() }
    pub fn settings_path(&self) -> &Path { &self.settings_path }
    pub fn save_settings(&self, s: Settings) -> anyhow::Result<()> {
        s.save(&self.settings_path)?;
        if let Some(l) = HashLevel::parse(&s.hash_level) { self.throttle.set_level(l); }
        *self.settings.write().unwrap() = s;
        Ok(())
    }
    /// Settings as the window edits them: the sync connection is left out (its token stays here).
    pub fn settings_for_window(&self) -> Settings { let mut s = self.settings(); s.sync.token.clear(); s }
    /// Save settings from the window. With `base` (the settings the window started from), only
    /// what the window changed is written: settings that changed meanwhile (from another device,
    /// a restore) aren't put back. The sync connection is kept as it is either way (only the
    /// Sync commands change it).
    pub fn save_window_settings(&self, new: Settings, base: Option<Settings>) -> anyhow::Result<()> {
        let cur = self.settings();
        let mut s = match base {
            None => new,
            Some(base) => {
                let (n, b) = (serde_json::to_value(&new)?, serde_json::to_value(&base)?);
                let mut c = serde_json::to_value(&cur)?;
                if let (Some(n), Some(c)) = (n.as_object(), c.as_object_mut()) {
                    for (k, v) in n { if b.get(k) != Some(v) { c.insert(k.clone(), v.clone()); } }
                }
                serde_json::from_value(c)?
            }
        };
        s.sync = cur.sync;
        self.save_settings(s)?;
        self.sync_soon();
        Ok(())
    }
    pub fn journal(&self) -> Journal { Journal::new(self.fs.clone(), self.settings().library_roots, self.machine.clone()) }

    // ---- startup / shared state ----
    pub fn startup(self: &Arc<Self>) -> anyhow::Result<StartupInfo> {
        let s = self.settings();
        let db = self.db()?;
        if s.library_roots.is_empty() { return Ok(StartupInfo { expired: vec![], expired_bytes: 0, synced: 0, needs_setup: true }); }
        self.started.store(true, std::sync::atomic::Ordering::Relaxed);
        self.reimport_switch_titledb();
        self.identify_on_start();
        // before the review is judged again: it matches against them
        self.ensure_catalogs();
        self.resettle_review();
        let _ = archive::rebuild_from_manifests(&db, &*self.fs, &s.library_roots);
        let synced = crate::shared::journal::sync(&db, &*self.fs, &s.library_roots, &self.machine).unwrap_or(0);
        self.start_sync_loop();
        let expired = archive::expired(&db, crate::db::now())?;
        let expired_bytes = expired.iter().map(|e| e.size).sum();
        Ok(StartupInfo { expired, expired_bytes, synced, needs_setup: false })
    }
    pub fn sync_shared(&self) -> anyhow::Result<usize> {
        crate::shared::journal::sync(&self.db()?, &*self.fs, &self.settings().library_roots, &self.machine)
    }

    // ---- scanning ----
    pub fn scan(self: &Arc<Self>, full: bool) -> JobId {
        let me = self.clone();
        self.jobs.submit(JobKind::Scan, if full { "Full rescan" } else { "Scan library" }, Box::new(move |ctx| {
            let db = me.db()?;
            let s = me.settings();
            let _ = crate::shared::journal::sync(&db, &*me.fs, &s.library_roots, &me.machine);
            ctx.progress(0, 0, "listing folders");
            let rep = crate::scan::scan(&db, &*me.fs, &s, ScanOpts { full }, &|p| ctx.progress(p.dirs_done, p.dirs_done + p.dirs_queued, &format!("{} files", p.files_seen)), ctx.cancel_flag())?;
            ctx.log(&format!("listed {} folders, skipped {} unchanged; +{} −{} ~{} files", rep.dirs_listed, rep.dirs_skipped, rep.files_added, rep.files_removed, rep.files_changed));
            for (p, e) in &rep.errors { ctx.log(&format!("error reading {}: {e}", p2s(p))); }
            for sys in registry().iter().filter(|s| s.probe.is_some()) {
                ctx.progress(0, 0, &format!("reading headers: {}", sys.name));
                if crate::probe::run_probes(&db, &*me.fs, sys, ctx.cancel_flag())? > 0 {
                    // ids read from headers can change how files group (e.g. Vita DLC under its game)
                    crate::grouping::assign_games(&db, sys)?;
                }
            }
            me.auto_import_titledbs(&db, &s);
            // games you own physically join the files that turned up for them
            for sys in crate::shelf::systems(&db).unwrap_or_default() { let _ = crate::shelf::join(&db, &sys); }
            *me.last_scan.lock().unwrap() = Some(rep);
            me.sync_after_scan(&db);
            Ok(())
        }))
    }
    pub fn last_scan(&self) -> Option<ScanReport> { self.last_scan.lock().unwrap().clone() }

    fn auto_import_titledbs(&self, db: &Db, s: &Settings) {
        let empty: bool = db.conn().query_row("SELECT COUNT(*)=0 FROM titledb WHERE source='psp'", [], |r| r.get(0)).unwrap_or(false);
        if !empty { return; }
        for r in &s.library_roots {
            // beside the games, or with the tools
            for p in [r.join("Sony - PSP").join("PSP_GAMES.tsv"), r.join("Sony - PSP").join(crate::systems::TOOLS_DIR).join("PSP_GAMES.tsv")] {
                if p.exists() { let _ = metadata::offline::psp_tsv_import(db, &p); break; }
            }
        }
    }

    // ---- browsing ----
    pub fn systems(&self) -> anyhow::Result<Vec<SystemSummary>> {
        let db = self.db()?;
        // games with files or shelf copies; files and bytes from the files
        let mut q = db.conn().prepare("SELECT g.system_id, COUNT(*), COALESCE((SELECT COUNT(*) FROM files f WHERE f.system_id=g.system_id),0), COALESCE((SELECT SUM(size) FROM files f WHERE f.system_id=g.system_id),0) FROM games g GROUP BY g.system_id")?;
        let mut v: Vec<SystemSummary> = q.query_map([], |r| Ok(SystemSummary { id: r.get(0)?, name: String::new(), games: r.get(1)?, files: r.get(2)?, bytes: r.get(3)? }))?.collect::<Result<_, _>>()?;
        for s in v.iter_mut() { s.name = crate::systems::by_id(&s.id).map(|d| d.name.to_string()).unwrap_or_else(|| s.id.clone()); }
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(v)
    }

    pub fn games(&self, f: &GameFilter, sort: GameSort, offset: i64, limit: i64) -> anyhow::Result<Page<GameRow>> {
        let db = self.db()?;
        let mut wh = vec!["1=1".to_string()];
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let arg = |v: String, args: &mut Vec<Box<dyn rusqlite::ToSql>>| { args.push(Box::new(v)); format!("?{}", args.len()) };
        if let Some(s) = &f.system { wh.push(format!("g.system_id = {}", arg(s.clone(), &mut args))); }
        if let Some(s) = f.search.as_ref().filter(|s| !s.trim().is_empty()) {
            let a = arg(format!("%{}%", s.trim()), &mut args);
            wh.push(format!("(g.title LIKE {a} OR g.canonical_id LIKE {a} OR EXISTS (SELECT 1 FROM files x WHERE x.game_id=g.id AND x.name LIKE {a})
                OR EXISTS (SELECT 1 FROM shelf_copies c WHERE c.ref = g.system_id || ':' || g.group_key AND c.deleted=0 AND (c.edition LIKE {a} OR c.location LIKE {a} OR c.release LIKE {a} OR c.notes LIKE {a})))"));
        }
        if let Some(r) = &f.region { wh.push(format!("g.parsed_json LIKE {}", arg(format!("%\"{r}\"%"), &mut args))); }
        if let Some(e) = &f.ext { wh.push(format!("EXISTS (SELECT 1 FROM files x WHERE x.game_id=g.id AND x.ext={})", arg(e.clone(), &mut args))); }
        if let Some(m) = &f.meta_state {
            if m == "none" { wh.push("(g.meta_state IS NULL OR g.meta_state='none')".into()); } else { wh.push(format!("g.meta_state = {}", arg(m.clone(), &mut args))); }
        }
        if let Some(p) = &f.play {
            match p.as_str() {
                "none" => wh.push("NOT EXISTS (SELECT 1 FROM play p WHERE p.ref = g.system_id || ':' || g.group_key AND p.status IS NOT NULL)".into()),
                "favorite" => wh.push("EXISTS (SELECT 1 FROM play p WHERE p.ref = g.system_id || ':' || g.group_key AND p.favorite=1)".into()),
                "scored" => wh.push("EXISTS (SELECT 1 FROM play p WHERE p.ref = g.system_id || ':' || g.group_key AND p.score IS NOT NULL)".into()),
                _ => { let a = arg(p.clone(), &mut args); wh.push(format!("EXISTS (SELECT 1 FROM play p WHERE p.ref = g.system_id || ':' || g.group_key AND p.status = {a})")); }
            }
        }
        const HAS_FILES: &str = "EXISTS (SELECT 1 FROM files x WHERE x.game_id=g.id)";
        const HAS_COPIES: &str = "EXISTS (SELECT 1 FROM shelf_copies c WHERE c.ref = g.system_id || ':' || g.group_key AND c.deleted=0)";
        match f.owned.as_deref() {
            Some("digital") => wh.push(HAS_FILES.into()),
            Some("physical") => wh.push(HAS_COPIES.into()),
            Some("both") => { wh.push(HAS_FILES.into()); wh.push(HAS_COPIES.into()); }
            _ => {}
        }
        if let Some(d) = &f.dat_status { wh.push(format!("EXISTS (SELECT 1 FROM files x WHERE x.game_id=g.id AND COALESCE(x.dat_status,'unhashed')={})", arg(d.clone(), &mut args))); }
        if let Some(k) = &f.status_key {
            let ka = arg(k.clone(), &mut args);
            match f.status_value.as_deref() {
                Some(v) if v.starts_with('!') => { let va = arg(v[1..].to_string(), &mut args); wh.push(format!("NOT EXISTS (SELECT 1 FROM status s WHERE s.game_id=g.id AND s.key={ka} AND s.value={va})")); }
                Some(v) => { let va = arg(v.to_string(), &mut args); wh.push(format!("EXISTS (SELECT 1 FROM status s WHERE s.game_id=g.id AND s.key={ka} AND s.value={va})")); }
                None => wh.push(format!("EXISTS (SELECT 1 FROM status s WHERE s.game_id=g.id AND s.key={ka})")),
            }
        }
        let where_sql = wh.join(" AND ");
        let order = match sort {
            GameSort::Title => "g.title COLLATE NOCASE", GameSort::Size => "size DESC", GameSort::System => "g.system_id, g.title COLLATE NOCASE",
            GameSort::Files => "files DESC", GameSort::Region => "json_extract(g.parsed_json,'$.regions[0]'), g.title COLLATE NOCASE",
        };
        let params: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let total: i64 = db.conn().query_row(&format!("SELECT COUNT(*) FROM games g WHERE {where_sql}"), params.as_slice(), |r| r.get(0))?;
        let sql = format!(
            "SELECT g.id, g.system_id, g.title, g.parsed_json, g.meta_state, g.art_json,
                    (SELECT COUNT(*) FROM files x WHERE x.game_id=g.id) AS files,
                    (SELECT COALESCE(SUM(size),0) FROM files x WHERE x.game_id=g.id) AS size,
                    (SELECT GROUP_CONCAT(DISTINCT ext) FROM files x WHERE x.game_id=g.id),
                    (SELECT json_group_object(key, value) FROM status s WHERE s.game_id=g.id),
                    (SELECT MIN(CASE dat_status WHEN 'verified' THEN 1 WHEN 'bad' THEN 2 WHEN 'unknown' THEN 3 ELSE 4 END) FROM files x WHERE x.game_id=g.id),
                    p.status, p.score, COALESCE(p.favorite, 0),
                    (SELECT COUNT(*) FROM shelf_copies c WHERE c.ref = g.system_id || ':' || g.group_key AND c.deleted=0),
                    (SELECT MIN(CASE WHEN c.sealed=1 THEN 1 WHEN c.cart=1 AND c.box=1 AND c.manual=1 THEN 2 WHEN c.cart=1 AND c.box=1 THEN 3 WHEN c.cart=1 THEN 4 WHEN c.box=1 THEN 5 WHEN c.manual=1 THEN 6 ELSE 4 END)
                       FROM shelf_copies c WHERE c.ref = g.system_id || ':' || g.group_key AND c.deleted=0)
             FROM games g LEFT JOIN play p ON p.ref = g.system_id || ':' || g.group_key WHERE {where_sql} ORDER BY {order} LIMIT {limit} OFFSET {offset}");
        let mut q = db.conn().prepare(&sql)?;
        let rows = q.query_map(params.as_slice(), |r| {
            let parsed: Option<ParsedName> = r.get::<_, Option<String>>(3)?.and_then(|s| serde_json::from_str(&s).ok());
            let art: BTreeMap<String, String> = r.get::<_, Option<String>>(5)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
            Ok(GameRow {
                id: r.get(0)?, system_id: r.get(1)?, title: r.get(2)?,
                regions: parsed.as_ref().map(|p| p.regions.clone()).unwrap_or_default(),
                tags: parsed.as_ref().map(|p| p.tags.iter().filter(|t| !t.to_lowercase().starts_with("track") && *t != "verified-good").cloned().collect()).unwrap_or_default(),
                meta_state: r.get(4)?, cover: art.get("cover").or(art.get("icon")).cloned(),
                files: r.get(6)?, size: r.get(7)?,
                exts: r.get::<_, Option<String>>(8)?.map(|s| s.split(',').map(String::from).collect()).unwrap_or_default(),
                status: r.get::<_, Option<String>>(9)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                dat: r.get::<_, Option<i64>>(10)?.and_then(|d| match d { 1 => Some("verified"), 2 => Some("bad"), 3 => Some("unknown"), _ => None }).map(String::from),
                play: r.get(11)?, score: r.get(12)?, favorite: r.get::<_, i64>(13)? != 0,
                shelf: ShelfSummary { copies: r.get(14)?, best: r.get::<_, Option<i64>>(15)?.and_then(|i| crate::shelf::LABELS.get(i as usize - 1)).map(|s| s.to_string()) },
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(Page { rows, total, offset })
    }

    pub fn game_detail(&self, id: i64) -> anyhow::Result<GameDetail> {
        let db = self.db()?;
        let sys: String = db.conn().query_row("SELECT system_id FROM games WHERE id=?1", [id], |r| r.get(0))?;
        let row = self.games(&GameFilter { system: Some(sys), ..Default::default() }, GameSort::Title, 0, i64::MAX)?
            .rows.into_iter().find(|g| g.id == id).ok_or_else(|| anyhow::anyhow!("game {id} not found"))?;
        let (parsed, canonical, meta, art): (Option<String>, Option<String>, Option<String>, Option<String>) =
            db.conn().query_row("SELECT parsed_json, canonical_id, meta_json, art_json FROM games WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        let mut q = db.conn().prepare("SELECT id, path, name, ext, size, crc32, sha1, dat_status, dat_game, probe_json FROM files WHERE game_id=?1 ORDER BY name")?;
        let files: Vec<FileDetail> = q.query_map([id], |r| Ok(FileDetail { id: r.get(0)?, path: r.get(1)?, name: r.get(2)?, ext: r.get(3)?, size: r.get(4)?, crc32: r.get(5)?, sha1: r.get(6)?, dat_status: r.get(7)?, dat_game: r.get(8)?, probe: r.get::<_, Option<String>>(9)?.and_then(|s| serde_json::from_str(&s).ok()) }))?.collect::<Result<_, _>>()?;
        let ids: Vec<i64> = files.iter().map(|f| f.id).collect();
        let mut meta: Option<serde_json::Value> = meta.and_then(|m| serde_json::from_str(&m).ok());
        // Switch: fill what the matched source left out (SteamGridDB only has art) from the title database
        if row.system_id == "switch" {
            if let Some(m) = meta.as_mut().filter(|m| m.get("candidates").is_none()) {
                let base = files.iter().flat_map(|f| f.probe.as_ref().and_then(|p| p["id"].as_str().map(String::from)).into_iter().chain(f.name.split(|c: char| !c.is_ascii_hexdigit()).map(String::from)))
                    .find_map(|t| crate::util::switch_base_id(&t));
                let extra: Option<String> = base.and_then(|b| db.conn().query_row("SELECT extra_json FROM titledb WHERE source='switch' AND id=?1", [format!("{b:016X}")], |r| r.get(0)).ok());
                if let Some(e) = extra.and_then(|e| serde_json::from_str::<serde_json::Value>(&e).ok()) {
                    for (ours, theirs) in [("description", "description"), ("publisher", "publisher"), ("release", "releaseDate")] {
                        let empty = m.get(ours).map(|v| v.is_null() || v.as_str() == Some("")).unwrap_or(true);
                        if empty { if let Some(v) = e.get(theirs).filter(|v| !v.is_null()) { m[ours] = if v.is_number() { v.to_string().into() } else { v.clone() }; } }
                    }
                }
            }
        }
        Ok(GameDetail {
            row, canonical, files,
            parsed: parsed.and_then(|p| serde_json::from_str(&p).ok()),
            meta,
            art: art.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default(),
            actions: self.actions_for(&ids)?,
            play: crate::userdata::play_of(&db, id)?,
            collections: crate::userdata::collections_of(&db, id)?,
            copies: crate::shelf::copies_of(&db, id)?,
        })
    }

    // ---- your physical collection ----
    pub fn shelf_add(&self, system: &str, release: &str, region: Option<&str>, f: crate::shelf::CopyFields) -> anyhow::Result<crate::shelf::Copy> {
        let db = self.db()?;
        let c = crate::shelf::add(&db, system, release, region, f)?;
        // a release you own the files of too: one game
        let _ = crate::shelf::join(&db, system);
        self.sync_soon();
        crate::shelf::get(&db, &c.id).map(|x| x.unwrap_or(c))
    }
    pub fn shelf_update(&self, id: &str, f: crate::shelf::CopyFields) -> anyhow::Result<crate::shelf::Copy> {
        let c = crate::shelf::update(&self.db()?, id, f)?;
        self.sync_soon();
        Ok(c)
    }
    pub fn shelf_remove(&self, id: &str) -> anyhow::Result<()> {
        let db = self.db()?;
        crate::shelf::remove(&db, id)?;
        crate::shelf::drop_if_empty(&db, id)?;
        self.sync_soon();
        Ok(())
    }
    /// A spreadsheet's rows matched to releases (columns guessed from the headers unless given).
    pub fn shelf_csv_preview(&self, text: &str, mapping: Option<crate::shelf_csv::Mapping>) -> anyhow::Result<Vec<crate::shelf_csv::ImportRow>> { self.shelf_csv_preview_for(text, mapping, None) }
    /// The same, with the console for rows that don't name one (a one-console spreadsheet).
    pub fn shelf_csv_preview_for(&self, text: &str, mapping: Option<crate::shelf_csv::Mapping>, console: Option<&str>) -> anyhow::Result<Vec<crate::shelf_csv::ImportRow>> {
        let t = crate::shelf_csv::parse(text)?;
        let m = mapping.unwrap_or_else(|| crate::shelf_csv::guess_mapping(&t.headers));
        crate::shelf_csv::preview(&self.db()?, &t, &m, console, &self.settings().region_order)
    }
    /// A spreadsheet's columns, and which holds what as guessed from their headers.
    pub fn shelf_csv_columns(&self, text: &str) -> anyhow::Result<(Vec<String>, crate::shelf_csv::Mapping)> {
        let t = crate::shelf_csv::parse(text)?;
        let m = crate::shelf_csv::guess_mapping(&t.headers);
        Ok((t.headers, m))
    }
    /// Add the matched rows to the shelf (repeats are skipped).
    pub fn shelf_csv_import(&self, rows: &[crate::shelf_csv::ImportRow]) -> anyhow::Result<usize> {
        let db = self.db()?;
        let n = crate::shelf_csv::import(&db, rows)?;
        for s in crate::shelf::systems(&db)? { let _ = crate::shelf::join(&db, &s); }
        self.sync_soon();
        Ok(n)
    }
    pub fn shelf_locations(&self) -> anyhow::Result<Vec<String>> { crate::shelf::locations(&self.db()?) }
    /// Releases matching what you typed, to add a copy of: from the console's No-Intro / Redump
    /// list (the preferred region first), else its title database, else IGDB's catalog.
    pub fn shelf_releases(&self, system: &str, query: &str, limit: usize) -> anyhow::Result<Vec<ReleaseHit>> {
        let db = self.db()?;
        let c = db.conn();
        let q = query.trim();
        if q.is_empty() { return Ok(vec![]); }
        let like = format!("%{}%", q.replace(' ', "%"));
        let sys = crate::systems::by_id(system).ok_or_else(|| anyhow::anyhow!("unknown console {system}"))?;
        let parsers = sys.parsers;
        let order = self.settings().region_order;
        let rank = |regions: &[String]| regions.iter().filter_map(|r| order.iter().position(|o| o == r)).min().unwrap_or(order.len());
        let hit = |name: String, source: &str| { let p = crate::parse::parse(parsers, &name); ReleaseHit { title: p.display().to_string(), regions: p.regions.clone(), release: name, source: source.into() } };
        let mut q1 = c.prepare("SELECT DISTINCT r.game_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1 AND r.game_name LIKE ?2 LIMIT 400")?;
        let mut hits: Vec<ReleaseHit> = q1.query_map((system, &like), |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|n| hit(n, "list")).collect();
        if hits.is_empty() {
            let src = match system { "psvita" => Some("psv"), "switch" => Some("switch"), "psp" => Some("psp"), _ => None };
            if let Some(src) = src {
                let mut q2 = c.prepare("SELECT name, COALESCE(region,'') FROM titledb WHERE source=?1 AND name LIKE ?2 LIMIT 400")?;
                hits = q2.query_map((src, &like), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.filter_map(|r| r.ok())
                    .map(|(n, reg)| { let reg = match reg.as_str() { "US" => "USA", "EU" => "Europe", "JP" => "Japan", "ASIA" => "Asia", "" => "", x => x }; hit(if reg.is_empty() { n } else { format!("{n} ({reg})") }, "titledb") }).collect();
            }
        }
        if hits.is_empty() {
            let mut q3 = c.prepare("SELECT name FROM catalog WHERE system_id=?1 AND name LIKE ?2 LIMIT 400")?;
            hits = q3.query_map((system, &like), |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).map(|n| hit(n, "catalog")).collect();
        }
        let key = crate::completion::title_key(q);
        // closest titles first (starting with what you typed), then the preferred region
        hits.sort_by(|a, b| {
            let (ka, kb) = (crate::completion::title_key(&a.title), crate::completion::title_key(&b.title));
            (!ka.starts_with(&key)).cmp(&!kb.starts_with(&key)).then(ka.cmp(&kb)).then(rank(&a.regions).cmp(&rank(&b.regions))).then(a.release.len().cmp(&b.release.len())).then(a.release.cmp(&b.release))
        });
        hits.dedup_by(|a, b| a.release == b.release);
        hits.truncate(limit);
        Ok(hits)
    }

    // ---- your play status and collections ----
    pub fn set_play(&self, game_id: i64, p: crate::userdata::PlayInfo) -> anyhow::Result<crate::userdata::PlayInfo> { crate::userdata::set_play(&self.db()?, &self.journal(), game_id, p).inspect(|_| self.sync_soon()) }
    pub fn play_summary(&self) -> anyhow::Result<crate::userdata::PlaySummary> { crate::userdata::play_summary(&self.db()?) }
    pub fn collections(&self) -> anyhow::Result<Vec<crate::userdata::CollectionSummary>> { crate::userdata::collections(&self.db()?) }
    pub fn collection(&self, id: &str) -> anyhow::Result<crate::userdata::CollectionDetail> { crate::userdata::collection(&self.db()?, id) }
    pub fn create_collection(&self, name: &str, game_ids: &[i64]) -> anyhow::Result<String> { crate::userdata::create_collection(&self.db()?, &self.journal(), name, game_ids).inspect(|_| self.sync_soon()) }
    pub fn update_collection(&self, id: &str, name: &str, notes: Option<String>, deleted: bool) -> anyhow::Result<()> { crate::userdata::update_collection(&self.db()?, &self.journal(), id, name, notes, deleted).inspect(|_| self.sync_soon()) }
    pub fn set_collection_items(&self, id: &str, entries: &[(String, Option<String>)]) -> anyhow::Result<()> { crate::userdata::set_items(&self.db()?, &self.journal(), id, entries).inspect(|_| self.sync_soon()) }
    /// How a collection file (JSON, or an AI's reply containing it) matches the library.
    pub fn collection_preview(&self, text: &str) -> anyhow::Result<crate::collection_io::ImportPreview> {
        let f = crate::collection_io::parse(text)?;
        crate::collection_io::preview(&self.db()?, &f, &self.settings().region_order)
    }
    pub fn collection_import(&self, text: &str, choices: &[Option<i64>]) -> anyhow::Result<String> {
        let f = crate::collection_io::parse(text)?;
        crate::collection_io::import(&self.db()?, &self.journal(), &f, choices)
    }
    pub fn collection_export(&self, id: &str) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(&crate::collection_io::export(&self.db()?, id)?)?)
    }
    pub fn add_to_collection(&self, id: &str, game_ids: &[i64]) -> anyhow::Result<usize> { crate::userdata::add_to_collection(&self.db()?, &self.journal(), id, game_ids).inspect(|_| self.sync_soon()) }

    /// Show a library file or folder in the system's file manager. Only paths inside the library.
    pub fn reveal(&self, path: &Path) -> anyhow::Result<()> {
        if !self.settings().library_roots.iter().any(|r| path.starts_with(r)) { anyhow::bail!("not inside a library folder"); }
        if !path.exists() { anyhow::bail!("{} no longer exists", p2s(path)); }
        crate::util::reveal(path)
    }

    /// Wide hero art and the game's logo from SteamGridDB, for the details panel (once per game).
    /// `force` tries again for a game already looked at.
    pub fn fetch_game_art(&self, game_id: i64, force: bool) -> anyhow::Result<BTreeMap<String, String>> {
        let s = self.settings();
        let db = self.db()?;
        let (title, sys, meta, art): (String, String, Option<String>, Option<String>) = db.conn().query_row("SELECT title, system_id, meta_json, art_json FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        let mut map: BTreeMap<String, String> = art.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default();
        if map.contains_key("_extra") && !force { return Ok(map); }
        // Switch: the title database's wide banner, by the game's id (no key needed)
        if sys == "switch" && !map.contains_key("banner") {
            let ids: Vec<String> = {
                let mut q = db.conn().prepare("SELECT COALESCE(json_extract(probe_json,'$.id'), '') || ' ' || name FROM files WHERE game_id=?1")?;
                let v = q.query_map([game_id], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
                v
            };
            let base = ids.iter().flat_map(|x| x.split(|c: char| !c.is_ascii_hexdigit())).find_map(|t| crate::util::switch_base_id(t));
            if let Some(b) = base {
                let extra: Option<String> = db.conn().query_row("SELECT extra_json FROM titledb WHERE source='switch' AND id=?1", [format!("{b:016X}")], |r| r.get(0)).ok();
                let url = extra.and_then(|e| serde_json::from_str::<serde_json::Value>(&e).ok()).and_then(|e| e["bannerUrl"].as_str().map(String::from));
                if let Some(u) = url { if let Ok(p) = self.http.fetch_art(&self.cache_dir, "titledb-switch", &format!("{b:016x}-banner"), &u) { map.insert("banner".into(), p2s(&p)); } }
            }
        }
        if !metadata::sgdb::configured(&s) {
            map.insert("_extra".into(), "done".into());
            db.conn().execute("UPDATE games SET art_json=?1 WHERE id=?2", (serde_json::to_string(&map)?, game_id))?;
            return Ok(map);
        }
        let meta: serde_json::Value = meta.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or_default();
        let name = meta["title"].as_str().unwrap_or(&title).to_string();
        let sgdb_id = if meta["source"].as_str() == Some("steamgriddb") { meta["source_id"].as_str().map(String::from) } else {
            metadata::sgdb::search(&db, &self.http, &s, &name)?.into_iter().find(|c| c.score >= 0.8).map(|c| c.source_id)
        };
        if let Some(id) = sgdb_id {
            for (kind, key) in [(metadata::ArtKind::Hero, "hero"), (metadata::ArtKind::Logo, "logo"), (metadata::ArtKind::Cover, "cover")] {
                if map.contains_key(key) { continue; }
                if let Some(a) = metadata::sgdb::art(&db, &self.http, &s, &id, kind).ok().and_then(|v| v.into_iter().next()) {
                    if let Ok(p) = self.http.fetch_art(&self.cache_dir, "steamgriddb", &format!("{id}-{key}"), &a.url) { map.insert(key.into(), p2s(&p)); }
                }
            }
        }
        map.insert("_extra".into(), "done".into());
        db.conn().execute("UPDATE games SET art_json=?1 WHERE id=?2", (serde_json::to_string(&map)?, game_id))?;
        Ok(map)
    }

    /// File ids of the given games.
    pub fn file_ids(&self, game_ids: &[i64]) -> anyhow::Result<Vec<i64>> {
        let db = self.db()?;
        let mut q = db.conn().prepare("SELECT id FROM files WHERE game_id=?1")?;
        let mut out = Vec::new();
        for g in game_ids { for r in q.query_map([g], |r| r.get(0))? { out.push(r?); } }
        Ok(out)
    }

    pub fn game_ids_for(&self, f: &GameFilter) -> anyhow::Result<Vec<i64>> {
        Ok(self.games(f, GameSort::Title, 0, i64::MAX)?.rows.into_iter().map(|g| g.id).collect())
    }

    // ---- rename / ops ----
    pub fn plan_renames(&self, game_ids: &[i64]) -> anyhow::Result<Vec<RenameItem>> { crate::rename::plan_renames(&self.db()?, &*self.fs, game_ids) }
    /// Names you chose to keep (skipped) aren't offered again.
    pub fn plan_renames_with(&self, game_ids: &[i64], use_matched: &[i64]) -> anyhow::Result<Vec<RenameItem>> {
        let db = self.db()?;
        let kept = crate::skips::keys(&db, "rename")?;
        Ok(crate::rename::plan_renames_with(&db, &*self.fs, game_ids, use_matched)?.into_iter().filter(|i| !kept.contains(&i.skip_key)).collect())
    }
    pub fn apply_renames(&self, items: &[RenameItem]) -> anyhow::Result<BatchResult> {
        let db = self.db()?;
        let ops = crate::rename::to_ops(&db, &*self.fs, items)?;
        crate::ops::apply(&db, &*self.fs, &self.journal(), ops, &format!("Rename {} files", items.len()))
    }
    pub fn undo(&self, batch_id: &str) -> anyhow::Result<BatchResult> { crate::ops::undo_batch(&self.db()?, &*self.fs, &self.journal(), batch_id) }
    pub fn history(&self) -> anyhow::Result<Vec<BatchInfo>> { crate::ops::history(&self.db()?, 200) }

    // ---- hashing / DATs ----
    pub fn hash(self: &Arc<Self>, scope: HashScope) -> anyhow::Result<JobId> {
        let db = self.db()?;
        let ids: Vec<i64> = match &scope {
            HashScope::All => { let mut q = db.conn().prepare("SELECT id FROM files WHERE hash_key IS NULL OR hash_key != size || ':' || mtime")?; let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?; v }
            HashScope::System(s) => { let mut q = db.conn().prepare("SELECT id FROM files WHERE system_id=?1")?; let v = q.query_map([s], |r| r.get(0))?.collect::<Result<_, _>>()?; v }
            HashScope::Games(g) => self.file_ids(g)?,
        };
        let label = match &scope { HashScope::All => "Hash library".to_string(), HashScope::System(s) => format!("Hash {s}"), HashScope::Games(g) => format!("Hash {} games", g.len()) };
        Ok(self.hash_files_job(ids, &label))
    }
    fn hash_files_job(self: &Arc<Self>, ids: Vec<i64>, label: &str) -> JobId {
        let me = self.clone();
        self.jobs.submit(JobKind::Hash, label, Box::new(move |ctx| {
            let db = me.db()?;
            let rep = crate::hash::hash_files(&db, &*me.fs, &ids, &me.throttle, &|p| ctx.progress(p.done, p.total, &format!("{:.1} GB read", p.bytes as f64 / 1e9)), ctx.cancel_flag())?;
            for (p, e) in &rep.failed { ctx.log(&format!("could not hash {}: {e}", p2s(p))); }
            ctx.log(&format!("hashed {} files ({:.1} GB), {} already up to date", rep.hashed, rep.bytes as f64 / 1e9, rep.skipped));
            // re-verify systems that have a DAT
            let mut q = db.conn().prepare("SELECT DISTINCT system_id FROM dats")?;
            let systems: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for s in systems { crate::dat::verify(&db, &s)?; let _ = crate::shelf::join(&db, &s); }
            // systems fingerprinted for the first time get their lists
            me.ensure_dats();
            Ok(())
        }))
    }

    /// Identify on start: fingerprint again the files that need a headerless fingerprint (NES,
    /// SNES, Atari 7800, Lynx), and get the game lists the library is missing.
    fn identify_on_start(self: &Arc<Self>) {
        let Ok(db) = self.db() else { return };
        let marks = crate::hash::HEADER_SYSTEMS.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(",");
        let ids: Vec<i64> = db.conn().prepare(&format!("SELECT id FROM files WHERE hash_key IS NULL AND system_id IN ({marks})"))
            .and_then(|mut q| q.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
        // the hash job gets the lists when it's done; otherwise get them now
        if ids.is_empty() { self.ensure_dats(); } else { self.hash_files_job(ids, "Fingerprint ROMs without their headers"); }
    }

    /// Download the No-Intro / Redump list of each system that has fingerprinted files and no
    /// list yet, or one more than 30 days old; matching files are then identified exactly.
    pub fn ensure_dats(self: &Arc<Self>) {
        if !self.started.load(std::sync::atomic::Ordering::Relaxed) { return; }
        let Ok(db) = self.db() else { return };
        let have: Vec<String> = db.conn().prepare("SELECT DISTINCT system_id FROM files WHERE sha1 IS NOT NULL")
            .and_then(|mut q| q.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
        let listed: std::collections::HashSet<String> = db.conn().prepare("SELECT system_id FROM dats")
            .and_then(|mut q| q.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
        let month = std::time::Duration::from_secs(30 * 24 * 3600);
        let stale = |id: &str| std::fs::metadata(self.cache_dir.join("dats").join(format!("{id}.dat"))).and_then(|m| m.modified()).ok()
            .and_then(|t| t.elapsed().ok()).is_none_or(|age| age > month);
        // arcade sets are known by name: FBNeo's list says which game each set is
        let arcade = have.iter().any(|id| crate::systems::by_id(id).is_some_and(|s| s.parsers.contains(&crate::systems::ParserKind::Mame)));
        let names: i64 = db.conn().query_row("SELECT COUNT(*) FROM titledb WHERE source='arcade'", [], |r| r.get(0)).unwrap_or(0);
        let mut todo: Vec<String> = have.into_iter().filter(|id| crate::dat::libretro_dat(id).is_some() && (!listed.contains(id) || stale(id))).collect();
        if arcade && (names == 0 || stale("arcade")) { todo.push("arcade".into()); }
        if !todo.is_empty() { self.download_dats(todo); } else { self.fetch_box_scans(); }
    }

    /// Download IGDB's catalog of each console in the library that has none yet, or one more than
    /// a month old (names and other names of every game it lists), to match games against locally.
    pub fn ensure_catalogs(self: &Arc<Self>) {
        if !self.started.load(std::sync::atomic::Ordering::Relaxed) { return; }
        let s = self.settings();
        if !metadata::igdb::configured(&s) { return; }
        let Ok(db) = self.db() else { return };
        let have: Vec<String> = db.conn().prepare("SELECT DISTINCT system_id FROM games").and_then(|mut q| q.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
        let todo: Vec<(String, u32)> = have.into_iter().filter_map(|id| crate::systems::by_id(&id).and_then(|s| s.igdb_platform).map(|p| (id, p)))
            .filter(|(id, _)| metadata::catalog::fetched_at(&db, id, "igdb").is_none_or(|at| crate::db::now() - at > metadata::catalog::REFRESH_SECS)).collect();
        if todo.is_empty() { return; }
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Download game catalogs (IGDB)", Box::new(move |ctx| {
            let db = me.db()?;
            let s = me.settings();
            let n = todo.len() as u64;
            let mut fetched: std::collections::HashMap<u32, Vec<metadata::igdb::CatalogEntry>> = std::collections::HashMap::new();
            for (i, (id, platform)) in todo.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                let name = crate::systems::by_id(id).map(|s| s.name).unwrap_or(id);
                ctx.progress(i as u64, n, name);
                // consoles sharing a platform (GBA hacks) share its list
                if !fetched.contains_key(platform) {
                    let big = std::sync::atomic::AtomicBool::new(false);
                    match metadata::igdb::catalog(&me.http, &s, *platform, &|k| { if k > metadata::catalog::MAX_ENTRIES { big.store(true, std::sync::atomic::Ordering::Relaxed); return false; } !ctx.cancelled() }) {
                        Ok(v) => { fetched.insert(*platform, v); }
                        Err(_) if big.load(std::sync::atomic::Ordering::Relaxed) => {
                            ctx.log(&format!("{name}: more than {} games listed; looked up by name instead", metadata::catalog::MAX_ENTRIES));
                            metadata::catalog::store(&db, id, "igdb", &[])?;
                            continue;
                        }
                        Err(e) => { ctx.log(&format!("✗ {name}: {e:#}")); continue; }
                    }
                }
                let v = &fetched[platform];
                metadata::catalog::store(&db, id, "igdb", v)?;
                ctx.log(&format!("✓ {name}: {} games", v.len()));
            }
            ctx.progress(n, n, "done");
            Ok(())
        }));
    }

    /// Get the box scan of every identified game whose cover isn't one yet (libretro's
    /// thumbnails, by the release name), and make it the game's cover.
    pub fn fetch_box_scans(self: &Arc<Self>) {
        if !self.started.load(std::sync::atomic::Ordering::Relaxed) { return; }
        let Ok(db) = self.db() else { return };
        let rows: Vec<(i64, String, String, Option<String>)> = db.conn().prepare(
            // (with MAX, SQLite takes the other columns from the largest verified file)
            "SELECT f.game_id, f.system_id, f.dat_game, json_extract(g.art_json,'$.cover'), MAX(f.size) FROM files f JOIN games g ON g.id=f.game_id
             WHERE f.dat_status='verified' AND f.dat_game IS NOT NULL GROUP BY f.game_id")
            .and_then(|mut q| q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()).unwrap_or_default();
        let todo: Vec<(i64, String, String)> = rows.into_iter()
            .filter(|(_, sys, _, cover)| metadata::boxart::folder(sys).is_some() && !cover.as_deref().is_some_and(|c| metadata::boxart::is_scan(&self.cache_dir, c) && Path::new(c).exists()))
            .map(|(g, s, r, _)| (g, s, r)).collect();
        if todo.is_empty() { return; }
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Get box scans for identified games", Box::new(move |ctx| {
            let db = me.db()?;
            let (mut got, mut none, mut failed) = (0, 0, 0);
            let n = todo.len() as u64;
            for (i, (g, sys, rel)) in todo.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                ctx.progress(i as u64, n, rel);
                match metadata::boxart::fetch(&me.http, &me.cache_dir, sys, rel) {
                    Ok(Some(p)) => { metadata::boxart::set_cover(&db, *g, &p)?; got += 1; }
                    Ok(None) => none += 1,
                    Err(e) => { failed += 1; if failed <= 20 { ctx.log(&format!("✗ {rel}: {e:#}")); } }
                }
            }
            ctx.progress(n, n, "done");
            ctx.log(&format!("{got} identified games have their box scan as the cover; libretro has no scan for {none}{}", if failed > 0 { format!("; {failed} couldn't be fetched (tried again next time)") } else { String::new() }));
            Ok(())
        }));
    }
    /// "Unpack and compare": fingerprint the given files, reading every file inside archives
    /// (streamed, nothing written to disk), so duplicates can be compared by content.
    pub fn compare_contents(self: &Arc<Self>, file_ids: &[i64]) -> anyhow::Result<JobId> {
        let ids = file_ids.to_vec();
        let me = self.clone();
        Ok(self.jobs.submit(JobKind::Hash, &format!("Unpack and compare {} files", ids.len()), Box::new(move |ctx| {
            let db = me.db()?;
            let settings = me.settings();
            let archives = crate::deep::needs_unpack(&db, &ids)?;
            let plain: Vec<i64> = {
                let mut q = db.conn().prepare("SELECT ext FROM files WHERE id=?1")?;
                let mut q2 = db.conn().prepare("SELECT system_id FROM files WHERE id=?1")?;
                ids.iter().copied().filter(|id| match (q.query_row([id], |r| r.get::<_, String>(0)), q2.query_row([id], |r| r.get::<_, String>(0))) { (Ok(e), Ok(s)) => !crate::deep::can_unpack(&s, &e), _ => false }).collect()
            };
            ctx.progress(0, 0, "fingerprinting the other copies");
            let rep = crate::hash::hash_files(&db, &*me.fs, &plain, &me.throttle, &|p| ctx.progress(p.done, p.total, &format!("fingerprinting: {:.1} GB read", p.bytes as f64 / 1e9)), ctx.cancel_flag())?;
            for (p, e) in &rep.failed { ctx.log(&format!("✗ could not read {}: {e}", p2s(p))); }
            let total: u64 = archives.iter().map(|a| a.2).sum();
            let read = std::sync::atomic::AtomicU64::new(0);
            for (id, path, _) in &archives {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let done = std::sync::atomic::AtomicBool::new(false);
                let r = std::thread::scope(|sc| {
                    sc.spawn(|| while !done.load(std::sync::atomic::Ordering::Relaxed) {
                        let b = read.load(std::sync::atomic::Ordering::Relaxed);
                        ctx.progress(b.min(total), total.max(1), &format!("unpacking {name}"));
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    });
                    let r = crate::deep::unpack_digest(&*me.fs, &settings, path, ctx.cancel_flag(), &read);
                    done.store(true, std::sync::atomic::Ordering::Relaxed);
                    r
                });
                match r {
                    Ok(d) => { crate::deep::save(&db, *id, &d)?; ctx.log(&format!("✓ {name}: {} file{} inside, fingerprinted", d.len(), if d.len() == 1 { "" } else { "s" })); }
                    Err(e) => { if ctx.cancelled() { anyhow::bail!("cancelled"); } ctx.log(&format!("✗ {name}: {e:#}")); }
                }
            }
            ctx.progress(total.max(1), total.max(1), "done");
            Ok(())
        })))
    }
    pub fn set_hash_level(&self, l: HashLevel) { self.throttle.set_level(l); }
    pub fn pause_hashing(&self, paused: bool) { if paused { self.throttle.pause() } else { self.throttle.resume() } }
    pub fn hash_state(&self) -> HashState { HashState { level: self.throttle.level(), paused: self.throttle.is_paused() } }
    pub fn import_dat(&self, system: &str, path: &Path) -> anyhow::Result<crate::dat::DatSummary> {
        let db = self.db()?;
        let s = crate::dat::import_dat(&db, system, path)?;
        crate::dat::verify(&db, system)?;
        let _ = crate::shelf::join(&db, system);
        Ok(s)
    }
    pub fn verify(&self, system: &str) -> anyhow::Result<crate::dat::VerifyReport> {
        let db = self.db()?;
        let r = crate::dat::verify(&db, system)?;
        let _ = crate::shelf::join(&db, system);
        Ok(r)
    }
    /// Join games you own physically to their files on a console (after a scan or verify).
    pub fn shelf_join(&self, system: &str) -> anyhow::Result<usize> { crate::shelf::join(&self.db()?, system) }
    /// How complete a console's collection is against its No-Intro / Redump list.
    pub fn completion(&self, system: &str, o: &crate::completion::Options) -> anyhow::Result<crate::completion::Completion> {
        let clones = crate::completion::clonelist(&self.http, &self.cache_dir, system);
        let db = self.db()?;
        if system == "psvita" { crate::completion::ensure_psv(&self.http, &db, &self.cache_dir); }
        crate::completion::completion(&db, system, &self.settings().region_order, clones.as_ref(), o)
    }
    /// Completion of every console that has a list, in sidebar order.
    pub fn completion_summary(&self, o: &crate::completion::Options) -> anyhow::Result<Vec<crate::completion::Summary>> {
        let db = self.db()?;
        let order = self.settings().region_order;
        let mut q = db.conn().prepare("SELECT DISTINCT system_id FROM dats")?;
        let have: std::collections::HashSet<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        // GBA Hacks borrows the GBA list: its "completion" means nothing
        crate::completion::ensure_psv(&self.http, &db, &self.cache_dir);
        for sys in registry().iter().filter(|s| (have.contains(s.id) || s.id == "psvita") && s.id != "gbahacks") {
            let clones = crate::completion::clonelist(&self.http, &self.cache_dir, sys.id);
            let c = crate::completion::completion(&db, sys.id, &order, clones.as_ref(), o)?;
            if c.total == 0 { continue; }
            out.push(crate::completion::Summary { system: sys.id.to_string(), name: sys.name.to_string(), total: c.total, have: c.have, unverified: c.unverified });
        }
        Ok(out)
    }
    pub fn missing(&self, system: &str) -> anyhow::Result<Vec<String>> { crate::dat::missing(&self.db()?, system) }

    // ---- cleanup ----
    /// Duplicate groups, minus the ones you said to keep as they are.
    pub fn dupes(&self, system: Option<&str>) -> anyhow::Result<Vec<DupeGroup>> {
        let s = self.settings();
        let db = self.db()?;
        let kept = crate::skips::keys(&db, "dupe")?;
        Ok(crate::dupes::find_dupes_with(&db, system, &s.region_order, s.one_game_one_rom)?.into_iter().filter(|g| !kept.contains(&g.skip_key)).collect())
    }
    /// Cleanup suggestions, minus the ones you ignored.
    pub fn junk(&self) -> anyhow::Result<Vec<JunkItem>> {
        let db = self.db()?;
        let ignored = crate::skips::keys(&db, "junk")?;
        Ok(crate::junk::find_junk(&db, &*self.fs, &self.settings())?.into_iter().filter(|j| !ignored.contains(&p2s(&j.path))).collect())
    }

    // ---- suggestions you said no to ----
    pub fn skip(&self, kind: &str, key: &str, label: &str, system_id: Option<&str>) -> anyhow::Result<()> { crate::skips::skip(&self.db()?, kind, key, label, system_id).inspect(|_| self.sync_soon()) }
    /// Bring a skipped suggestion back. A game's match goes back to review.
    pub fn unskip(&self, kind: &str, key: &str) -> anyhow::Result<()> { crate::skips::unskip(&self.db()?, kind, key).inspect(|_| self.sync_soon()) }
    pub fn skipped(&self, kind: &str) -> anyhow::Result<Vec<crate::skips::Skipped>> { crate::skips::list(&self.db()?, kind) }
    /// Archive files (duplicates, junk) with the normal hold.
    pub fn archive_paths(&self, paths: &[PathBuf], reason: &str) -> anyhow::Result<Vec<(PathBuf, String)>> {
        let db = self.db()?;
        let (j, hold) = (self.journal(), self.settings().archive_hold_days);
        let mut failed = Vec::new();
        for p in paths {
            let m = self.fs.stat(p);
            let r = match m {
                Ok(m) if m.is_dir => crate::ops::apply(&db, &*self.fs, &j, vec![crate::ops::Op::remove_empty_dir(p.clone())], "Remove empty folder")
                    .and_then(|r| match r.failed.first() { Some((_, e)) => Err(anyhow::anyhow!(e.clone())), None => Ok(()) }),
                _ => archive::archive_file(&db, &*self.fs, &j, p, reason, None, hold).map(|_| ()),
            };
            if let Err(e) = r { failed.push((p.clone(), format!("{e:#}"))); }
        }
        Ok(failed)
    }
    /// Games whose folders don't follow their system's layout.
    /// Games you chose to leave as they are aren't listed.
    pub fn layout(&self, system: Option<&str>) -> anyhow::Result<Vec<crate::layout::SystemLayout>> {
        let db = self.db()?;
        let left = crate::skips::keys(&db, "layout")?;
        let mut v = crate::layout::check(&db, &self.settings(), system)?;
        for s in v.iter_mut() {
            for i in s.items.iter_mut() {
                let mut srcs: Vec<String> = i.moves.iter().map(|m| p2s(&m.src)).chain(i.duplicates.iter().map(|d| p2s(&d.src))).collect();
                srcs.sort();
                i.skip_key = format!("{}:{:?}\n{}\n{}", s.system_id, i.problem, i.places.join("\n"), srcs.join("\n"));
            }
            s.items.retain(|i| !left.contains(&i.skip_key));
        }
        Ok(v)
    }
    /// Bring the given games in line with their system's layout, as one undoable batch.
    pub fn fix_layout(&self, game_ids: &[i64]) -> anyhow::Result<BatchResult> {
        let db = self.db()?;
        let settings = self.settings();
        let report = crate::layout::check(&db, &settings, None)?;
        let want: std::collections::HashSet<i64> = game_ids.iter().copied().collect();
        let items: Vec<&crate::layout::LayoutItem> = report.iter().flat_map(|s| s.items.iter()).filter(|i| want.contains(&i.game_id) && (!i.moves.is_empty() || !i.duplicates.is_empty())).collect();
        let roots = settings.library_roots.clone();
        let sys_dir_of = |p: &Path| roots.iter().find_map(|r| p.strip_prefix(r).ok().and_then(|rel| rel.components().next()).map(|c| r.join(c.as_os_str())));
        // spare copies go to the archive first, so the folders they sat in can be removed
        let spare: Vec<PathBuf> = items.iter().flat_map(|i| i.duplicates.iter().map(|d| d.src.clone())).collect();
        let failed_archive = self.archive_paths(&spare, "duplicate")?;
        let ops = crate::layout::to_ops(&*self.fs, &items, sys_dir_of);
        let mut res = crate::ops::apply(&db, &*self.fs, &self.journal(), ops, "Tidy folders")?;
        // a folder that turned out not to be empty is simply kept
        res.failed.retain(|(op, _)| op.kind != crate::ops::OpKind::RemoveDir);
        for (p, e) in failed_archive { res.failed.push((crate::ops::Op::archive(p.clone(), p), e)); }
        Ok(res)
    }
    /// Move tools (program folders, scripts and the files they use) into their system's _Tools
    /// folder, as one undoable step. A folder holding game-sized files is refused.
    pub fn move_to_tools(&self, paths: &[PathBuf]) -> anyhow::Result<BatchResult> {
        let roots = self.settings().library_roots;
        let mut ops = Vec::new();
        let mut refused = Vec::new();
        for p in paths {
            let Some(sys_dir) = roots.iter().find_map(|r| p.strip_prefix(r).ok().and_then(|rel| rel.components().next()).map(|c| r.join(c.as_os_str()))) else { refused.push((p.clone(), "not inside a library folder".to_string())); continue };
            if let Ok(m) = self.fs.stat(p) {
                if m.is_dir {
                    if let Some(big) = crate::fsx::walk_files(&*self.fs, p).into_iter().find(|(_, size)| *size > 64 << 20) {
                        refused.push((p.clone(), format!("it holds {} — move game files out of it first", big.0.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
                        continue;
                    }
                }
            }
            let dst = sys_dir.join(crate::systems::TOOLS_DIR).join(p.file_name().unwrap_or_default());
            ops.push(crate::ops::Op::mv(p.clone(), dst));
        }
        let mut res = crate::ops::apply(&self.db()?, &*self.fs, &self.journal(), ops, "Move to _Tools")?;
        for (p, why) in refused { res.failed.push((crate::ops::Op::mv(p.clone(), p), why)); }
        Ok(res)
    }
    /// Move a file to another folder (e.g. a ROM out of a tool folder).
    pub fn move_file(&self, src: &Path, dst_dir: &Path) -> anyhow::Result<BatchResult> {
        let dst = dst_dir.join(src.file_name().unwrap_or_default());
        crate::ops::apply(&self.db()?, &*self.fs, &self.journal(), vec![crate::ops::Op::mv(src.to_path_buf(), dst)], "Move file")
    }

    // ---- archive ----
    pub fn archive_list(&self) -> anyhow::Result<Vec<ArchiveEntry>> { archive::list(&self.db()?) }
    pub fn archive_expired(&self) -> anyhow::Result<Vec<ArchiveEntry>> { archive::expired(&self.db()?, crate::db::now()) }
    pub fn archive_purge(&self, ids: &[i64]) -> anyhow::Result<PurgeReport> { archive::purge(&self.db()?, &*self.fs, &self.journal(), ids) }
    /// Delete only entries still past their hold after re-reading the shared manifests.
    pub fn archive_purge_expired(&self, ids: &[i64]) -> anyhow::Result<PurgeReport> { archive::purge_expired(&self.db()?, &*self.fs, &self.journal(), ids) }
    /// "Empty archive": everything except pinned originals.
    pub fn archive_purge_all(&self) -> anyhow::Result<PurgeReport> { archive::purge_all(&self.db()?, &*self.fs, &self.journal()) }
    pub fn archive_restore(&self, id: i64) -> anyhow::Result<PathBuf> { archive::restore(&self.db()?, &*self.fs, &self.journal(), id) }
    pub fn archive_extend(&self, ids: &[i64], days: u32) -> anyhow::Result<()> {
        let db = self.db()?;
        for id in ids { archive::extend(&db, &*self.fs, &self.journal(), *id, days)?; }
        Ok(())
    }

    // ---- actions ----
    pub fn actions(&self) -> Vec<ActionInfo> { actions::registry().iter().map(|a| a.info()).collect() }
    pub fn actions_for(&self, file_ids: &[i64]) -> anyhow::Result<Vec<ApplicableAction>> {
        Ok(actions::applicable(&self.db()?, file_ids)?.into_iter().map(|(a, ids)| ApplicableAction { action: a.info(), file_ids: ids }).collect())
    }
    pub fn requirements(&self, action_id: &str) -> anyhow::Result<Vec<Requirement>> {
        Ok(actions::by_id(action_id).ok_or_else(|| anyhow::anyhow!("unknown action {action_id}"))?.requires(&self.settings()))
    }
    pub fn run_action(self: &Arc<Self>, action_id: &str, file_ids: &[i64]) -> anyhow::Result<JobId> {
        let a = actions::by_id(action_id).ok_or_else(|| anyhow::anyhow!("unknown action {action_id}"))?;
        let missing: Vec<String> = a.requires(&self.settings()).into_iter().filter(|r| !r.ok).map(|r| format!("{} — {}", r.name, r.hint)).collect();
        if !missing.is_empty() { anyhow::bail!("{} can't run yet:\n{}", a.name(), missing.join("\n")); }
        let me = self.clone();
        let ids = file_ids.to_vec();
        Ok(self.jobs.submit(JobKind::Action, &format!("{} ({} files)", a.name(), ids.len()), Box::new(move |ctx| {
            let db = me.db()?;
            let rep = actions::execute(&db, &*me.fs, &me.settings(), &me.journal(), a, &ids, Some(ctx), &AtomicBool::new(false))?;
            ctx.log(&format!("{} done, {} skipped, {} failed", rep.ok, rep.skipped.len(), rep.failed.len()));
            if !rep.failed.is_empty() { anyhow::bail!("{} of {} files failed — see the log", rep.failed.len(), ids.len()); }
            Ok(())
        })))
    }

    // ---- metadata ----
    /// You asked for these games yourself: any you skipped in review come back first.
    pub fn look_up(self: &Arc<Self>, game_ids: &[i64]) -> anyhow::Result<JobId> {
        let db = self.db()?;
        for g in game_ids { if let Some((key, _, _)) = crate::skips::game_key(&db, *g)? { crate::skips::unskip(&db, "match", &key)?; } }
        Ok(self.match_metadata_inner(game_ids, true, true))
    }
    /// Look games up; games whose match you skipped (or marked "No match") are left alone —
    /// `look_up` is the way back.
    /// Games that already have info are left as they are.
    pub fn match_metadata(self: &Arc<Self>, game_ids: &[i64]) -> JobId { self.match_metadata_with(game_ids, false) }
    /// `refresh`: look up games that already have info too. A match you picked is still kept, and
    /// an automatic one is only replaced by another confident match — an unsure lookup never sends
    /// a matched game back to review.
    pub fn match_metadata_with(self: &Arc<Self>, game_ids: &[i64], refresh: bool) -> JobId { self.match_metadata_inner(game_ids, refresh, false) }
    /// `asked`: you asked for these yourself, so what's found is your decision (it replaces a
    /// "No match" or pick you made before, on every synced device).
    fn match_metadata_inner(self: &Arc<Self>, game_ids: &[i64], refresh: bool, asked: bool) -> JobId {
        let me = self.clone();
        let skipped = self.db().and_then(|db| crate::skips::keys(&db, "match")).unwrap_or_default();
        let ids: Vec<i64> = match self.db() {
            Ok(db) => game_ids.iter().copied().filter(|g| crate::skips::game_key(&db, *g).ok().flatten().map_or(true, |(k, _, _)| !skipped.contains(&k))).collect(),
            Err(_) => game_ids.to_vec(),
        };
        self.jobs.submit(JobKind::Metadata, &format!("Look up {} games", ids.len()), Box::new(move |ctx| {
            let db = me.db()?;
            let s = me.settings();
            let offline: i64 = db.conn().query_row("SELECT COUNT(*) FROM titledb", [], |r| r.get(0))?;
            if !metadata::any_source(&s) && offline == 0 {
                anyhow::bail!("No game info sources are set up. Add a SteamGridDB key, IGDB credentials, or ScreenScraper developer credentials in Settings → Game info sources.");
            }
            let (mut auto, mut review, mut none, mut kept) = (0, 0, 0, 0);
            let total = ids.len() as u64;
            for (i, g) in ids.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                let (title, state, chosen): (String, Option<String>, bool) = db.conn().query_row(
                    "SELECT title, meta_state, COALESCE(json_extract(meta_json,'$.chosen'), 0) FROM games WHERE id=?1", [g],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))).unwrap_or_else(|_| (format!("game {g}"), None, false));
                ctx.progress(i as u64, total, &title);
                let matched = state.as_deref() == Some("matched");
                if matched && (!refresh || chosen) {
                    if refresh { ctx.log(&format!("= {title}: kept the match you picked")); }
                    kept += 1;
                    continue;
                }
                match metadata::match_game(&db, &me.http, &s, *g) {
                    Ok(MatchResult::Auto(c)) => {
                        let (found, source) = (c.title.clone(), c.source.clone());
                        match metadata::apply_match(&db, &me.http, &s, &me.cache_dir, *g, c) {
                            Ok(()) => { ctx.log(&format!("✓ {title} → {found} ({})", source_label(&source))); auto += 1; }
                            Err(e) => ctx.log(&format!("✗ {title}: matched {found} but saving failed: {e:#}")),
                        }
                    }
                    // an unsure lookup never undoes a match the game already has
                    Ok(_) if matched => { ctx.log(&format!("= {title}: no surer match found; kept the current one")); kept += 1; }
                    Ok(MatchResult::NeedsReview(c)) => { ctx.log(&format!("? {title}: {} possible matches — added to Match review", c.len())); metadata::set_review(&db, *g, &c)?; review += 1; }
                    Ok(MatchResult::NoMatch) => { ctx.log(&format!("– {title}: no match found")); db.conn().execute("UPDATE games SET meta_state='none' WHERE id=?1", [g])?; none += 1; }
                    Err(e) => ctx.log(&format!("✗ {title}: {e:#}")),
                }
                if asked { db.conn().execute("UPDATE games SET meta_json=json_set(meta_json,'$.asked',1) WHERE id=?1 AND meta_json IS NOT NULL AND meta_state IN ('matched','review')", [g])?; }
            }
            ctx.progress(ids.len() as u64, ids.len() as u64, "done");
            ctx.log(&format!("{auto} matched, {review} need review, {none} not found{}", if kept > 0 { format!(", {kept} already had info and were kept") } else { String::new() }));
            Ok(())
        }))
    }
    pub fn review_list(&self) -> anyhow::Result<Vec<ReviewItem>> {
        let db = self.db()?;
        let skipped = crate::skips::keys(&db, "match")?;
        let mut q = db.conn().prepare("SELECT id, system_id, title, meta_json, (SELECT name FROM files f WHERE f.game_id=g.id ORDER BY size DESC LIMIT 1), system_id || ':' || group_key FROM games g WHERE meta_state='review' ORDER BY title")?;
        let v = q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?)))?
            .filter_map(|r| r.ok()).filter(|r| !skipped.contains(&r.5)).map(|(game_id, system_id, title, m, file_name, skip_key)| {
                let candidates = m.and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok()).and_then(|v| serde_json::from_value(v["candidates"].clone()).ok()).unwrap_or_default();
                ReviewItem { game_id, system_id, title, file_name, candidates, skip_key }
            }).collect();
        Ok(v)
    }
    /// "None of these": mark the game as having no info and drop it from review.
    /// It's remembered: later lookups leave the game alone until you look it up yourself.
    pub fn reject_metadata(&self, game_id: i64) -> anyhow::Result<()> {
        let db = self.db()?;
        db.conn().execute("UPDATE games SET meta_state='none', meta_json=NULL WHERE id=?1", [game_id])?;
        if let Some((key, title, sys)) = crate::skips::game_key(&db, game_id)? { crate::skips::skip(&db, "match", &key, &format!("{title} — no match"), Some(&sys))?; }
        Ok(())
    }
    /// "Search again" with a different name; the results replace the game's review candidates.
    pub fn search_metadata(&self, game_id: i64, query: &str) -> anyhow::Result<Vec<MetaCandidate>> {
        let db = self.db()?;
        let c = metadata::search_titles(&db, &self.http, &self.settings(), game_id, query)?;
        metadata::set_review(&db, game_id, &c)?;
        Ok(c)
    }
    pub fn candidate_preview(&self, c: &MetaCandidate) -> anyhow::Result<Option<String>> {
        metadata::candidate_preview(&self.db()?, &self.http, &self.settings(), c)
    }
    /// "Wrong match? Choose again": look the game up afresh and put every option in Match review —
    /// even a confident one — for you to pick. The current match is set aside until you do.
    pub fn choose_again(self: &Arc<Self>, game_id: i64) -> anyhow::Result<JobId> {
        let db = self.db()?;
        let (key, title, _) = crate::skips::game_key(&db, game_id)?.ok_or_else(|| anyhow::anyhow!("no such game"))?;
        crate::skips::unskip(&db, "match", &key)?;
        let me = self.clone();
        Ok(self.jobs.submit(JobKind::Metadata, &format!("Find matches for {title}"), Box::new(move |ctx| {
            let db = me.db()?;
            let cands = match metadata::match_game(&db, &me.http, &me.settings(), game_id)? {
                MatchResult::Auto(c) => vec![c],
                MatchResult::NeedsReview(v) => v,
                MatchResult::NoMatch => vec![],
            };
            ctx.log(&format!("{} possible matches for {title} — pick one in Match review{}", cands.len(), if cands.is_empty() { " (or search there with another name)" } else { "" }));
            metadata::set_review(&db, game_id, &cands)?;
            // you asked to choose this one yourself: re-judging the review never picks for you
            db.conn().execute("UPDATE games SET meta_json=json_set(meta_json,'$.again',1) WHERE id=?1", [game_id])?;
            Ok(())
        })))
    }
    /// You picked this match: it's marked as yours, so later lookups keep it.
    pub fn choose_metadata(&self, game_id: i64, mut c: MetaCandidate) -> anyhow::Result<()> {
        c.chosen = true;
        metadata::apply_match(&self.db()?, &self.http, &self.settings(), &self.cache_dir, game_id, c)
    }
    /// Download the Switch title database (blawar/titledb, US English: names, publishers,
    /// release dates, icons and banners by title id), import it, then look up again the Switch
    /// games that have no info yet or are waiting in Match review.
    pub fn download_switch_titledb(self: &Arc<Self>) -> JobId {
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Download Switch title database", Box::new(move |ctx| {
            let dest = me.cache_dir.join("titledb").join("switch-US.en.json");
            ctx.progress(0, 0, "downloading");
            me.http.download("https://raw.githubusercontent.com/blawar/titledb/master/US.en.json", &dest,
                &|got, total| ctx.progress(got, total.max(got), &format!("{:.0} of {:.0} MB", got as f64 / 1e6, total as f64 / 1e6)), ctx.cancel_flag())?;
            ctx.progress(0, 0, "importing");
            let db = me.db()?;
            let n = metadata::offline::switch_titledb_import(&db, &dest)?;
            ctx.log(&format!("imported {n} Switch titles"));
            let _ = std::fs::write(me.cache_dir.join("titledb").join("switch-import-version"), metadata::offline::SWITCH_IMPORT_VERSION.to_string());
            me.redo_stale_switch_matches(&db);
            let ids: Vec<i64> = {
                let mut q = db.conn().prepare("SELECT id FROM games WHERE system_id='switch' AND (meta_state IS NULL OR meta_state IN ('review','nomatch','none'))")?;
                let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
                v
            };
            if !ids.is_empty() { ctx.log(&format!("looking up {} Switch games again", ids.len())); me.match_metadata(&ids); }
            Ok(())
        }))
    }

    /// Check matched games against the consoles their match was released on (needs IGDB or RAWG).
    /// A match for another console goes back to Match review with this console's options.
    pub fn check_consoles(self: &Arc<Self>) -> JobId {
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Check matches against consoles", Box::new(move |ctx| {
            let db = me.db()?;
            let s = me.settings();
            if !metadata::can_check_consoles(&s) { anyhow::bail!("Checking consoles needs IGDB or RAWG in Settings → Game info."); }
            let rows: Vec<(i64, String, String, String)> = {
                let mut q = db.conn().prepare("SELECT id, system_id, title, meta_json FROM games WHERE meta_state='matched' AND json_extract(meta_json,'$.console') IS NULL")?;
                let v = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
                v
            };
            let (mut ok, mut wrong, mut unknown) = (0, 0, 0);
            for (i, (gid, sys, title, mj)) in rows.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                ctx.progress(i as u64, rows.len() as u64, title);
                let Some(system) = crate::systems::by_id(sys) else { continue };
                let Ok(mut c) = serde_json::from_str::<metadata::MetaCandidate>(mj) else { continue };
                match metadata::check_console(&db, &me.http, &s, system, &c.title) {
                    Some((v, p)) => {
                        c.console = Some(v.clone()); c.platforms = p.clone();
                        if v == "match" {
                            db.conn().execute("UPDATE games SET meta_json=?1 WHERE id=?2", (serde_json::to_string(&c)?, gid))?;
                            ok += 1;
                        } else {
                            // the right console's options, with the old match listed last
                            let mut cands = metadata::search_titles(&db, &me.http, &s, *gid, title).unwrap_or_default();
                            cands.retain(|x| !(x.source == c.source && x.source_id == c.source_id));
                            cands.push(c.clone());
                            metadata::set_review(&db, *gid, &cands)?;
                            ctx.log(&format!("✗ {title}: matched “{}”, released on {} — not {}; back in Match review", c.title, p.join(", "), system.name));
                            wrong += 1;
                        }
                    }
                    None => unknown += 1,
                }
            }
            ctx.log(&format!("{ok} on the right console, {wrong} for another console (sent to Match review), {unknown} not found to check"));
            Ok(())
        }))
    }

    /// Download the No-Intro / Redump name lists from libretro-database for the given systems
    /// (all in the library when empty) and verify against them. Files need fingerprints to match.
    pub fn download_dats(self: &Arc<Self>, systems: Vec<String>) -> JobId {
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Download game lists (No-Intro / Redump)", Box::new(move |ctx| {
            let db = me.db()?;
            let ids: Vec<String> = if systems.is_empty() { me.systems()?.into_iter().map(|s| s.id).collect() } else { systems.clone() };
            let todo: Vec<(String, &'static str)> = ids.into_iter().filter_map(|id| if id == "arcade" { Some((id, crate::dat::ARCADE_LIST)) } else { crate::dat::libretro_dat(&id).map(|r| (id, r)) }).collect();
            for (i, (id, rel)) in todo.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                if id == "arcade" {
                    ctx.progress(i as u64, todo.len() as u64, "Arcade game names (FBNeo)");
                    let dest = me.cache_dir.join("dats").join("arcade.dat");
                    match me.http.download(&crate::dat::libretro_url(rel), &dest, &|_, _| {}, ctx.cancel_flag()).and_then(|_| metadata::offline::arcade_import(&db, &dest)) {
                        Ok(n) => ctx.log(&format!("✓ Arcade game names (FBNeo): {n} sets")),
                        Err(e) => ctx.log(&format!("✗ Arcade game names (FBNeo): {e:#}")),
                    }
                    continue;
                }
                let name = crate::systems::by_id(id).map(|s| s.name).unwrap_or(id);
                ctx.progress(i as u64, todo.len() as u64, name);
                let dest = me.cache_dir.join("dats").join(format!("{id}.dat"));
                let r = me.http.download(&crate::dat::libretro_url(rel), &dest, &|_, _| {}, ctx.cancel_flag())
                    .and_then(|_| crate::dat::import_dat(&db, id, &dest))
                    .and_then(|s| crate::dat::verify(&db, id).map(|v| (s, v)));
                let _ = crate::shelf::join(&db, id);
                match r {
                    Ok((s, v)) => ctx.log(&format!("✓ {name}: {} games listed; {} files verified{}", s.games, v.verified, if v.verified == 0 { " (fingerprint the system to match files)" } else { "" })),
                    Err(e) => ctx.log(&format!("✗ {name}: {e:#}")),
                }
            }
            ctx.progress(todo.len() as u64, todo.len() as u64, "done");
            me.fetch_box_scans();
            Ok(())
        }))
    }

    /// Read the downloaded Switch title database again when it was imported by an older version
    /// of the import, then correct the matches that changed. Once per import version, as a job,
    /// when the app starts.
    /// Once after the way matches are judged changes: judge the games waiting in Match review again
    /// from the options already found (no new lookups), and keep the ones that are now sure.
    fn resettle_review(self: &Arc<Self>) {
        let marker = self.cache_dir.join("matcher-version");
        let current = std::fs::read_to_string(&marker).ok().and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
        if current >= metadata::MATCHER_VERSION { return; }
        // the games waiting in review, and the ones not found before: both are looked up again
        let waiting: i64 = self.db().and_then(|db| Ok(db.conn().query_row("SELECT COUNT(*) FROM games WHERE meta_state IN ('review', 'none')", [], |r| r.get(0))?)).unwrap_or(0);
        if waiting == 0 { let _ = std::fs::write(&marker, metadata::MATCHER_VERSION.to_string()); return; }
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Settle Match review", Box::new(move |ctx| {
            let (sure, left) = me.settle_review(&|i, n, t| { if ctx.cancelled() { return false; } ctx.progress(i, n, t); true })?;
            if ctx.cancelled() { anyhow::bail!("cancelled"); }
            std::fs::write(&marker, metadata::MATCHER_VERSION.to_string())?;
            ctx.log(&format!("{sure} games in Match review were a sure match after all and have their info; {left} still need you"));
            // the rest were searched the old way: look them up again (now also without a label in
            // front, a pack's wording, or a subtitle), with the games not found before. Skipped
            // games and ones you sent back to choose yourself are left alone.
            let packs: Vec<i64> = {
                let db = me.db()?;
                let mut q = db.conn().prepare("SELECT id FROM games WHERE (meta_state='review' AND COALESCE(json_extract(meta_json,'$.again'), 0) = 0) OR meta_state='none'")?;
                let v = q.query_map([], |r| r.get::<_, i64>(0))?.filter_map(|r| r.ok()).collect();
                v
            };
            if !packs.is_empty() { me.match_metadata(&packs); }
            Ok(())
        }));
    }

    /// Judge every game waiting in Match review again from its options (not the ones you sent
    /// back with "Choose again", or skipped). Returns (now matched, still waiting).
    pub fn settle_review(&self, progress: &dyn Fn(u64, u64, &str) -> bool) -> anyhow::Result<(usize, usize)> {
        let db = self.db()?;
        let skipped = crate::skips::keys(&db, "match")?;
        let s = self.settings();
        let rows: Vec<(i64, String, String, String, String)> = {
            let mut q = db.conn().prepare("SELECT id, system_id, title, meta_json, system_id || ':' || group_key FROM games WHERE meta_state='review' AND meta_json IS NOT NULL AND COALESCE(json_extract(meta_json,'$.again'), 0) = 0")?;
            let v = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.filter_map(|r| r.ok()).filter(|r: &(i64, String, String, String, String)| !skipped.contains(&r.4)).collect();
            v
        };
        let (mut sure, mut left) = (0, 0);
        let n = rows.len() as u64;
        for (i, (id, sys, title, m, _)) in rows.into_iter().enumerate() {
            if !progress(i as u64, n, &title) { break; }
            let Some(system) = crate::systems::by_id(&sys) else { continue };
            let mut cands: Vec<MetaCandidate> = serde_json::from_str::<serde_json::Value>(&m).ok().and_then(|v| serde_json::from_value(v["candidates"].clone()).ok()).unwrap_or_default();
            let release = metadata::release_name(&db, id)?;
            let names: Vec<&str> = std::iter::once(title.as_str()).chain(release.as_deref()).collect();
            metadata::rescore(system, &names, &mut cands);
            match metadata::decide(cands) {
                MatchResult::Auto(c) => match metadata::apply_match(&db, &self.http, &s, &self.cache_dir, id, c) { Ok(()) => sure += 1, Err(_) => left += 1 },
                MatchResult::NeedsReview(c) => { metadata::set_review(&db, id, &c)?; left += 1; }
                MatchResult::NoMatch => left += 1,
            }
        }
        Ok((sure, left))
    }

    fn reimport_switch_titledb(self: &Arc<Self>) {
        let dir = self.cache_dir.join("titledb");
        let (file, marker) = (dir.join("switch-US.en.json"), dir.join("switch-import-version"));
        let current = std::fs::read_to_string(&marker).ok().and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(1);
        if !file.exists() || current >= metadata::offline::SWITCH_IMPORT_VERSION { return; }
        let me = self.clone();
        self.jobs.submit(JobKind::Metadata, "Update Switch title database", Box::new(move |ctx| {
            ctx.progress(0, 0, "reading");
            let db = me.db()?;
            let n = metadata::offline::switch_titledb_import(&db, &file)?;
            std::fs::write(&marker, metadata::offline::SWITCH_IMPORT_VERSION.to_string())?;
            let stale = metadata::offline::stale_switch_matches(&db)?.len();
            ctx.log(&format!("read {n} Switch titles; {stale} games were matched to a bundle or later edition and are looked up again"));
            me.redo_stale_switch_matches(&db);
            Ok(())
        }));
    }

    /// Games matched by title id to a listing the title database no longer gives for that id
    /// (a bundle or later edition): forget that listing's cached art and look them up again.
    fn redo_stale_switch_matches(self: &Arc<Self>, db: &Db) {
        let Ok(stale) = metadata::offline::stale_switch_matches(db) else { return };
        if stale.is_empty() { return; }
        let art = self.cache_dir.join("art").join("titledb-switch");
        for (_, id) in &stale {
            let prefix = format!("{}-", id.to_lowercase());
            for e in std::fs::read_dir(&art).into_iter().flatten().flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) { let _ = std::fs::remove_file(e.path()); }
            }
        }
        let mut ids: Vec<i64> = stale.into_iter().map(|(g, _)| g).collect();
        ids.dedup();
        self.match_metadata_with(&ids, true);
    }

    pub fn sources_status(&self) -> anyhow::Result<Vec<metadata::SourceStatus>> { Ok(metadata::source_status(&self.db()?, &self.settings())) }

    pub fn import_titledb(&self, kind: &str, path: &Path) -> anyhow::Result<usize> {
        let db = self.db()?;
        match kind {
            "psp" => metadata::offline::psp_tsv_import(&db, path),
            "switch" => metadata::offline::switch_titledb_import(&db, path),
            _ => anyhow::bail!("unknown title database kind {kind} (psp | switch)"),
        }
    }

    // ---- system images (sidebar) ----
    /// Locally available sidebar images, by system id.
    pub fn system_images(&self) -> BTreeMap<String, String> {
        registry().iter().filter_map(|s| metadata::system_art::image_for(&self.cache_dir, s.id).map(|p| (s.id.to_string(), p2s(&p)))).collect()
    }
    /// Download missing system images in the background.
    pub fn fetch_system_images(self: &Arc<Self>) -> anyhow::Result<Option<JobId>> {
        let present: Vec<String> = self.systems()?.into_iter().map(|s| s.id).filter(|id| metadata::system_art::image_for(&self.cache_dir, id).is_none()).collect();
        if present.is_empty() { return Ok(None); }
        let me = self.clone();
        Ok(Some(self.jobs.submit(JobKind::Other, "Download system images", Box::new(move |ctx| {
            let urls = metadata::system_art::lookup_urls(&me.http, &present)?;
            for (i, (id, url)) in urls.iter().enumerate() {
                if ctx.cancelled() { anyhow::bail!("cancelled"); }
                let name = crate::systems::by_id(id).map(|s| s.name).unwrap_or(id);
                ctx.progress(i as u64, urls.len() as u64, name);
                match url {
                    None => ctx.log(&format!("– {name}: no image on Wikipedia")),
                    Some(u) => match metadata::system_art::download(&me.http, &me.cache_dir, id, u) {
                        Ok(_) => ctx.log(&format!("✓ {name}")),
                        Err(e) => ctx.log(&format!("✗ {name}: {e:#}")),
                    },
                }
            }
            ctx.progress(present.len() as u64, present.len() as u64, "done");
            Ok(())
        }))))
    }
    pub fn set_system_image(&self, system_id: &str, path: Option<&Path>) -> anyhow::Result<()> {
        metadata::system_art::set_custom(&self.cache_dir, system_id, path)
    }

    // ---- sync server (optional) ----
    fn sync_client(&self) -> Option<crate::sync::client::Client> {
        let s = self.settings().sync;
        s.enabled().then(|| crate::sync::client::Client::new(&s.url, &s.token))
    }
    /// Sign this PC in to a sync server. A library with nothing in it, connecting to a server
    /// that has a library, is offered a restore: the server's library folders come back, to be
    /// mapped to this PC's (`sync_restore`). Otherwise this library is what gets uploaded.
    pub fn sync_connect(self: &Arc<Self>, url: &str, password: &str) -> anyhow::Result<Option<Vec<crate::config::RootMap>>> {
        let url = crate::sync::client::normalize_url(url);
        let (token, device_id) = crate::sync::client::Client::login(&url, password, &device_name())?;
        let client = crate::sync::client::Client::new(&url, &token);
        let db = self.db()?;
        // a different server: everything is sent afresh
        if self.settings().sync.url != url {
            db.conn().execute_batch("DELETE FROM sync_sent; DELETE FROM sync_inbox; DELETE FROM sync_state;")?;
        }
        let mut s = self.settings();
        s.sync.url = url;
        s.sync.token = token;
        s.sync.device_id = device_id;
        // a PC that has never synced with this server, connecting to one that has a library,
        // takes that library first (even if it has scanned its folders already); its own
        // settings never go up before the server's are taken
        let never: bool = db.conn().query_row("SELECT COUNT(*)=0 FROM sync_sent", [], |r| r.get(0))?;
        let offer = if never { crate::sync::run::server_roots(&client)? } else { None };
        match &offer {
            Some(roots) => {
                crate::sync::run::state_set(&db, "restore", Some("1"))?;
                crate::sync::run::state_set(&db, "offer", Some(&serde_json::to_string(roots)?))?;
            }
            None => { crate::sync::run::ensure_root_ids(&mut s); crate::sync::run::state_set(&db, "settings_ok", Some("1"))?; }
        }
        self.save_settings(s)?;
        self.start_sync_loop();
        if offer.is_none() {
            // the first upload (or a reconnect) as a job, with progress
            self.sync_start()?;
        }
        Ok(offer)
    }
    /// Restore from the server on this new PC: its library folders mapped to these. Settings
    /// come now; matches, fingerprints and the rest as the folders are scanned.
    pub fn sync_restore(self: &Arc<Self>, mapping: Vec<crate::config::RootMap>) -> anyhow::Result<JobId> {
        let client = self.sync_client().ok_or_else(|| anyhow::anyhow!("not connected to a sync server"))?;
        let mapping: Vec<_> = mapping.into_iter().filter(|m| !m.path.as_os_str().is_empty()).collect();
        if mapping.is_empty() { anyhow::bail!("choose where at least one library folder is on this PC"); }
        let mut s = self.settings();
        s.library_roots = mapping.iter().map(|m| m.path.clone()).collect();
        s.sync.roots = mapping;
        self.save_settings(s)?;
        let me = self.clone();
        Ok(self.jobs.submit(JobKind::Other, "Restore from the sync server", Box::new(move |ctx| {
            let _g = me.sync.lock.lock().unwrap();
            let db = me.db()?;
            let mut s = me.settings();
            ctx.progress(0, 0, "getting the library from the server");
            let rep = crate::sync::run::restore(&db, &client, &mut s, &me.cache_dir, &|n| ctx.progress(n as u64, 0, &format!("{n} records")))?;
            // the server's shared settings onto the settings as they are now (the window may
            // have saved some meanwhile)
            let mut cur = me.settings();
            cur.apply_shared(&s.shared_json());
            me.save_settings(cur)?;
            crate::sync::run::state_set(&db, "restore", Some("scanning"))?;
            crate::sync::run::state_set(&db, "offer", None)?;
            crate::sync::run::state_set(&db, "settings_ok", Some("1"))?;
            ctx.log(&format!("{} records from the server; matches, fingerprints and the rest follow as the library is scanned", rep.pulled));
            // the inbox is applied at the end of the scan, which also ends the restore
            me.scan(false);
            Ok(())
        })))
    }
    /// After a scan: records waiting for the games and files it found are applied.
    fn sync_after_scan(&self, db: &Db) {
        let mut s = self.settings();
        if !s.sync.enabled() { return; }
        let mut rep = crate::sync::run::SyncReport::default();
        let waiting: i64 = db.conn().query_row("SELECT COUNT(*) FROM sync_inbox", [], |r| r.get(0)).unwrap_or(0);
        if waiting == 0 {
            if crate::sync::run::state_get(db, "restore").ok().flatten().as_deref() == Some("scanning") { let _ = crate::sync::run::state_set(db, "restore", None); }
            self.sync_soon();
            return;
        }
        let _g = self.sync.lock.lock().unwrap();
        let before = s.clone();
        if crate::sync::run::apply_inbox(db, None, &mut s, &self.cache_dir, &mut rep).is_ok() && s != before { let _ = self.save_settings(s); }
        // a restore ends with the first scan after it (not one before the folders were mapped)
        if crate::sync::run::state_get(db, "restore").ok().flatten().as_deref() == Some("scanning") { let _ = crate::sync::run::state_set(db, "restore", None); }
        drop(_g);
        self.sync_soon();
    }
    /// Sync with the server now (sends what changed here, takes what changed elsewhere).
    pub fn sync_now(&self) -> anyhow::Result<crate::sync::run::SyncReport> { self.sync_with(&|_, _, _| {}) }
    fn sync_with(&self, progress: &dyn Fn(u64, u64, &str)) -> anyhow::Result<crate::sync::run::SyncReport> {
        let client = self.sync_client().ok_or_else(|| anyhow::anyhow!("not connected to a sync server"))?;
        let _g = self.sync.lock.lock().unwrap();
        let db = self.db()?;
        *self.sync.running.lock().unwrap() = Some(SyncProgress { message: "starting".into(), done: 0, total: 0 });
        let r = self.sync_locked(&db, &client, &|done, total, what| {
            *self.sync.running.lock().unwrap() = Some(SyncProgress { message: what.to_string(), done, total });
            progress(done, total, what);
        });
        *self.sync.running.lock().unwrap() = None;
        let _ = crate::sync::run::record(&db, &r.as_ref().map(|x| x.clone()).map_err(|e| format!("{e:#}")));
        r
    }
    fn sync_locked(&self, db: &Db, client: &crate::sync::client::Client, progress: &dyn Fn(u64, u64, &str)) -> anyhow::Result<crate::sync::run::SyncReport> {
        let (db, client) = (db, client);
        if crate::sync::run::state_get(db, "restore")?.is_some() { anyhow::bail!("choose where the library folders are on this PC first (Settings → Sync)"); }
        let mut s = self.settings();
        if crate::sync::run::ensure_root_ids(&mut s) { self.save_settings(s.clone())?; }
        let r = crate::sync::run::sync_once(db, client, &self.machine, &mut s, &self.cache_dir, progress);
        if let Ok(rep) = &r {
            if rep.settings_changed { let mut cur = self.settings(); cur.apply_shared(&s.shared_json()); self.save_settings(cur)?; }
        }
        r
    }
    /// Sync now, as a job in the drawer (nothing new is started while a sync job is waiting or
    /// running: that one is returned).
    pub fn sync_start(self: &Arc<Self>) -> anyhow::Result<JobId> {
        self.sync_client().ok_or_else(|| anyhow::anyhow!("not connected to a sync server"))?;
        if let Some(j) = self.jobs.list().into_iter().find(|j| j.label == SYNC_JOB && matches!(j.state, crate::jobs::JobState::Queued | crate::jobs::JobState::Running)) { return Ok(j.id); }
        self.sync.job.store(true, std::sync::atomic::Ordering::SeqCst);
        let me = self.clone();
        Ok(self.jobs.submit(JobKind::Other, SYNC_JOB, Box::new(move |ctx| {
            let r = me.sync_with(&|done, total, what| ctx.progress(done, total, what));
            me.sync.job.store(false, std::sync::atomic::Ordering::SeqCst);
            let rep = r?;
            let list = |m: &BTreeMap<String, usize>| m.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ");
            if rep.sent.is_empty() && rep.received.is_empty() { ctx.log("up to date"); }
            if !rep.sent.is_empty() { ctx.log(&format!("sent {}{}", list(&rep.sent), if rep.covers_up > 0 { format!(", {} covers", rep.covers_up) } else { String::new() })); }
            if !rep.received.is_empty() { ctx.log(&format!("received {}{}", list(&rep.received), if rep.covers_down > 0 { format!(", {} covers", rep.covers_down) } else { String::new() })); }
            Ok(())
        })))
    }
    /// Sign out of the sync server (this PC's data stays; the library folders keep their ids).
    pub fn sync_disconnect(&self) -> anyhow::Result<()> {
        let _g = self.sync.lock.lock().unwrap();
        let mut s = self.settings();
        s.sync.url.clear(); s.sync.token.clear(); s.sync.device_id.clear();
        self.save_settings(s)?;
        self.db()?.conn().execute_batch("DELETE FROM sync_sent; DELETE FROM sync_inbox; DELETE FROM sync_state;")?;
        Ok(())
    }
    pub fn sync_status(&self) -> SyncStatus {
        let s = self.settings().sync;
        let db = self.db().ok();
        let state = |k: &str| db.as_ref().and_then(|db| crate::sync::run::state_get(db, k).ok().flatten());
        let restore_pending = state("restore").is_some();
        let offer = state("offer").and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        let last = db.as_ref().and_then(crate::sync::run::last);
        SyncStatus {
            connected: s.enabled(), url: s.url, device_id: s.device_id, restore_pending, offer, roots: s.roots,
            last_at: last.as_ref().map(|l| l.0),
            error: last.as_ref().and_then(|l| l.1.as_ref().err().cloned()),
            last: last.and_then(|l| l.1.ok()),
            running: self.sync.running.lock().unwrap().clone(),
            pending: state("pending").and_then(|v| v.parse().ok()),
            activity: db.as_ref().map(crate::sync::run::activity).unwrap_or_default(),
        }
    }
    /// Sync in a few seconds (after a change here that other devices should see).
    pub fn sync_soon(&self) {
        if self.settings.read().unwrap().sync.enabled() { *self.sync.kick.lock().unwrap() = Some(std::time::Instant::now()); }
    }
    /// Sync in the background: shortly after changes here, and every few minutes for changes
    /// elsewhere. Does nothing while no server is set.
    fn start_sync_loop(self: &Arc<Self>) {
        if self.sync.looping.swap(true, std::sync::atomic::Ordering::SeqCst) { return; }
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            let every = std::time::Duration::from_secs(300);
            let mut last = std::time::Instant::now() - every + std::time::Duration::from_secs(10);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let Some(me) = me.upgrade() else { return };
                if !me.settings.read().unwrap().sync.enabled() { continue; }
                let kicked = me.sync.kick.lock().unwrap().is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(3));
                if !kicked && last.elapsed() < every { continue; }
                *me.sync.kick.lock().unwrap() = None;
                last = std::time::Instant::now();
                if me.sync.job.load(std::sync::atomic::Ordering::SeqCst) { continue; }
                // only a sync with something to send or take shows in the drawer; a quiet check
                // just notes that all is up to date
                let check = (|| -> anyhow::Result<bool> {
                    let client = me.sync_client().ok_or_else(|| anyhow::anyhow!("not connected"))?;
                    let Ok(_g) = me.sync.lock.try_lock() else { return Ok(false) };
                    let db = me.db()?;
                    if crate::sync::run::state_get(&db, "restore")?.is_some() { return Ok(false); }
                    let work = crate::sync::run::has_work(&db, &client, &me.machine, &me.settings(), &me.cache_dir);
                    if let Ok(false) = work { let _ = crate::sync::run::record(&db, &Ok(Default::default())); }
                    if let Err(e) = &work { let _ = crate::sync::run::record(&db, &Err(format!("{e:#}"))); }
                    work
                })();
                if let Ok(true) = check { let _ = me.sync_start(); }
            }
        });
    }

    pub fn stats(&self) -> anyhow::Result<Stats> { crate::stats::stats(&self.db()?, &self.settings()) }
}
