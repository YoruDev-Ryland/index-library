//! Thin Tauri wrappers over lo_core::library::Library. Blocking work runs off the UI thread.
use lo_core::actions::{ActionInfo, Requirement};
use lo_core::archive::{ArchiveEntry, PurgeReport};
use lo_core::config::Settings;
use lo_core::dupes::DupeGroup;
use lo_core::hash::HashLevel;
use lo_core::jobs::{JobId, JobInfo};
use lo_core::junk::JunkItem;
use lo_core::library::*;
use lo_core::metadata::MetaCandidate;
use lo_core::ops::{BatchInfo, BatchResult};
use lo_core::rename::RenameItem;
use lo_core::scan::ScanReport;
use lo_core::stats::Stats;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

type Lib<'a> = State<'a, Arc<Library>>;
type R<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String { format!("{err:#}") }

/// Run a closure on the blocking pool so the UI never waits on the NAS.
async fn blocking<T: Send + 'static>(lib: &Arc<Library>, f: impl FnOnce(&Arc<Library>) -> anyhow::Result<T> + Send + 'static) -> R<T> {
    let lib = lib.clone();
    tauri::async_runtime::spawn_blocking(move || f(&lib)).await.map_err(e)?.map_err(|x| format!("{x:#}"))
}

#[tauri::command] async fn startup(lib: Lib<'_>) -> R<StartupInfo> { blocking(&lib, |l| l.startup()).await }
#[tauri::command] fn get_settings(lib: Lib<'_>) -> Settings { lib.settings_for_window() }
#[tauri::command] fn settings_path(lib: Lib<'_>) -> String { lib.settings_path().to_string_lossy().into_owned() }
#[tauri::command] fn save_settings(lib: Lib<'_>, settings: Settings, base: Option<Settings>) -> R<()> { lib.save_window_settings(settings, base).map_err(e) }
#[tauri::command] async fn systems(lib: Lib<'_>) -> R<Vec<SystemSummary>> { blocking(&lib, |l| l.systems()).await }
#[tauri::command] fn all_systems() -> Vec<(String, String)> { lo_core::systems::registry().iter().map(|s| (s.id.to_string(), s.name.to_string())).collect() }
#[tauri::command] async fn games(lib: Lib<'_>, filter: GameFilter, sort: GameSort, offset: i64, limit: i64) -> R<Page<GameRow>> { blocking(&lib, move |l| l.games(&filter, sort, offset, limit)).await }
#[tauri::command] async fn game_ids_for(lib: Lib<'_>, filter: GameFilter) -> R<Vec<i64>> { blocking(&lib, move |l| l.game_ids_for(&filter)).await }
#[tauri::command] async fn set_play(lib: Lib<'_>, game_id: i64, play: lo_core::userdata::PlayInfo) -> R<lo_core::userdata::PlayInfo> { blocking(&lib, move |l| l.set_play(game_id, play)).await }
#[tauri::command] async fn play_summary(lib: Lib<'_>) -> R<lo_core::userdata::PlaySummary> { blocking(&lib, |l| l.play_summary()).await }
#[tauri::command] async fn collections(lib: Lib<'_>) -> R<Vec<lo_core::userdata::CollectionSummary>> { blocking(&lib, |l| l.collections()).await }
#[tauri::command] async fn collection(lib: Lib<'_>, id: String) -> R<lo_core::userdata::CollectionDetail> { blocking(&lib, move |l| l.collection(&id)).await }
#[tauri::command] async fn create_collection(lib: Lib<'_>, name: String, game_ids: Vec<i64>) -> R<String> { blocking(&lib, move |l| l.create_collection(&name, &game_ids)).await }
#[tauri::command] async fn update_collection(lib: Lib<'_>, id: String, name: String, notes: Option<String>, deleted: bool) -> R<()> { blocking(&lib, move |l| l.update_collection(&id, &name, notes, deleted)).await }
#[tauri::command] async fn set_collection_items(lib: Lib<'_>, id: String, entries: Vec<(String, Option<String>)>) -> R<()> { blocking(&lib, move |l| l.set_collection_items(&id, &entries)).await }
#[tauri::command] async fn collection_preview(lib: Lib<'_>, text: String) -> R<lo_core::collection_io::ImportPreview> { blocking(&lib, move |l| l.collection_preview(&text)).await }
#[tauri::command] async fn collection_import(lib: Lib<'_>, text: String, choices: Vec<Option<i64>>) -> R<String> { blocking(&lib, move |l| l.collection_import(&text, &choices)).await }
#[tauri::command] async fn collection_export(lib: Lib<'_>, id: String) -> R<String> { blocking(&lib, move |l| l.collection_export(&id)).await }
/// Only collection files, chosen in a file dialog: .json (or .txt to read an AI's reply).
fn collection_file(path: &std::path::Path, read: bool) -> R<()> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "json" || (read && ext == "txt") { Ok(()) } else { Err("collections are saved and opened as .json files".into()) }
}
#[tauri::command] async fn write_text_file(path: PathBuf, text: String) -> R<()> { collection_file(&path, false)?; std::fs::write(&path, text).map_err(|e| e.to_string()) }
#[tauri::command] async fn read_text_file(path: PathBuf) -> R<String> {
    collection_file(&path, true)?;
    if std::fs::metadata(&path).map(|m| m.len() > 4 << 20).unwrap_or(false) { return Err("that file is too large to be a collection".into()); }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}
/// A spreadsheet chosen in a file dialog (.csv, .tsv or .txt), as text whatever its encoding.
#[tauri::command] async fn read_csv_file(path: PathBuf) -> R<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "csv" | "tsv" | "txt") { return Err("choose a .csv file".into()); }
    if std::fs::metadata(&path).map(|m| m.len() > 16 << 20).unwrap_or(false) { return Err("that file is too large to be a spreadsheet of games".into()); }
    std::fs::read(&path).map(|b| lo_core::util::text_of(&b)).map_err(|e| e.to_string())
}
#[tauri::command] async fn add_to_collection(lib: Lib<'_>, id: String, game_ids: Vec<i64>) -> R<usize> { blocking(&lib, move |l| l.add_to_collection(&id, &game_ids)).await }
#[tauri::command] async fn reveal(lib: Lib<'_>, path: PathBuf) -> R<()> { blocking(&lib, move |l| l.reveal(&path)).await }
#[tauri::command] async fn fetch_game_art(lib: Lib<'_>, game_id: i64, force: bool) -> R<std::collections::BTreeMap<String, String>> { blocking(&lib, move |l| l.fetch_game_art(game_id, force)).await }
#[tauri::command] async fn game_detail(lib: Lib<'_>, id: i64) -> R<GameDetail> { blocking(&lib, move |l| l.game_detail(id)).await }
#[tauri::command] fn scan(lib: Lib<'_>, full: bool) -> JobId { lib.scan(full) }
#[tauri::command] fn last_scan(lib: Lib<'_>) -> Option<ScanReport> { lib.last_scan() }
#[tauri::command] fn jobs_list(lib: Lib<'_>) -> Vec<JobInfo> { lib.jobs.list() }
#[tauri::command] fn job_cancel(lib: Lib<'_>, id: JobId) { lib.jobs.cancel(id) }
#[tauri::command] fn jobs_clear(lib: Lib<'_>) { lib.jobs.clear_finished() }
#[tauri::command] async fn plan_renames(lib: Lib<'_>, game_ids: Vec<i64>, use_matched: Option<Vec<i64>>) -> R<Vec<RenameItem>> { blocking(&lib, move |l| l.plan_renames_with(&game_ids, &use_matched.unwrap_or_default())).await }
#[tauri::command] async fn apply_renames(lib: Lib<'_>, items: Vec<RenameItem>) -> R<BatchResult> { blocking(&lib, move |l| l.apply_renames(&items)).await }
#[tauri::command] async fn undo(lib: Lib<'_>, batch_id: String) -> R<BatchResult> { blocking(&lib, move |l| l.undo(&batch_id)).await }
#[tauri::command] async fn history(lib: Lib<'_>) -> R<Vec<BatchInfo>> { blocking(&lib, |l| l.history()).await }
#[tauri::command] async fn hash(lib: Lib<'_>, scope: HashScope) -> R<JobId> { blocking(&lib, move |l| l.hash(scope)).await }
#[tauri::command] fn set_hash_level(lib: Lib<'_>, level: HashLevel) { lib.set_hash_level(level) }
#[tauri::command] fn pause_hashing(lib: Lib<'_>, paused: bool) { lib.pause_hashing(paused) }
#[tauri::command] fn hash_state(lib: Lib<'_>) -> HashState { lib.hash_state() }
#[tauri::command] async fn import_dat(lib: Lib<'_>, system: String, path: PathBuf) -> R<lo_core::dat::DatSummary> { blocking(&lib, move |l| l.import_dat(&system, &path)).await }
#[tauri::command] async fn verify(lib: Lib<'_>, system: String) -> R<lo_core::dat::VerifyReport> { blocking(&lib, move |l| l.verify(&system)).await }
#[tauri::command] async fn missing(lib: Lib<'_>, system: String) -> R<Vec<String>> { blocking(&lib, move |l| l.missing(&system)).await }
#[tauri::command] async fn dupes(lib: Lib<'_>, system: Option<String>) -> R<Vec<DupeGroup>> { blocking(&lib, move |l| l.dupes(system.as_deref())).await }
#[tauri::command] async fn junk(lib: Lib<'_>) -> R<Vec<JunkItem>> { blocking(&lib, |l| l.junk()).await }
#[tauri::command] async fn archive_paths(lib: Lib<'_>, paths: Vec<PathBuf>, reason: String) -> R<Vec<(PathBuf, String)>> { blocking(&lib, move |l| l.archive_paths(&paths, &reason)).await }
#[tauri::command] async fn archive_files(lib: Lib<'_>, file_ids: Vec<i64>, reason: String) -> R<Vec<(PathBuf, String)>> {
    blocking(&lib, move |l| {
        let db = l.db()?;
        let mut paths = Vec::new();
        for id in &file_ids { paths.push(PathBuf::from(db.conn().query_row("SELECT path FROM files WHERE id=?1", [id], |r| r.get::<_, String>(0))?)); }
        l.archive_paths(&paths, &reason)
    }).await
}
#[tauri::command] async fn compare_contents(lib: Lib<'_>, file_ids: Vec<i64>) -> R<JobId> { blocking(&lib, move |l| l.compare_contents(&file_ids)).await }
#[tauri::command] async fn layout(lib: Lib<'_>, system: Option<String>) -> R<Vec<lo_core::layout::SystemLayout>> { blocking(&lib, move |l| l.layout(system.as_deref())).await }
#[tauri::command] async fn fix_layout(lib: Lib<'_>, game_ids: Vec<i64>) -> R<BatchResult> { blocking(&lib, move |l| l.fix_layout(&game_ids)).await }
#[tauri::command] async fn move_to_tools(lib: Lib<'_>, paths: Vec<PathBuf>) -> R<BatchResult> { blocking(&lib, move |l| l.move_to_tools(&paths)).await }
#[tauri::command] async fn move_file(lib: Lib<'_>, src: PathBuf, dst_dir: PathBuf) -> R<BatchResult> { blocking(&lib, move |l| l.move_file(&src, &dst_dir)).await }
#[tauri::command] async fn archive_list(lib: Lib<'_>) -> R<Vec<ArchiveEntry>> { blocking(&lib, |l| l.archive_list()).await }
#[tauri::command] async fn archive_purge(lib: Lib<'_>, ids: Vec<i64>) -> R<PurgeReport> { blocking(&lib, move |l| l.archive_purge(&ids)).await }
#[tauri::command] async fn archive_purge_expired(lib: Lib<'_>, ids: Vec<i64>) -> R<PurgeReport> { blocking(&lib, move |l| l.archive_purge_expired(&ids)).await }
#[tauri::command] async fn archive_purge_all(lib: Lib<'_>) -> R<PurgeReport> { blocking(&lib, |l| l.archive_purge_all()).await }
#[tauri::command] async fn archive_restore(lib: Lib<'_>, id: i64) -> R<PathBuf> { blocking(&lib, move |l| l.archive_restore(id)).await }
#[tauri::command] async fn archive_extend(lib: Lib<'_>, ids: Vec<i64>, days: u32) -> R<()> { blocking(&lib, move |l| l.archive_extend(&ids, days)).await }
#[tauri::command] fn actions(lib: Lib<'_>) -> Vec<ActionInfo> { lib.actions() }
#[tauri::command] async fn actions_for_games(lib: Lib<'_>, game_ids: Vec<i64>) -> R<Vec<ApplicableAction>> { blocking(&lib, move |l| { let f = l.file_ids(&game_ids)?; l.actions_for(&f) }).await }
#[tauri::command] async fn requirements(lib: Lib<'_>, action_id: String) -> R<Vec<Requirement>> { blocking(&lib, move |l| l.requirements(&action_id)).await }
#[tauri::command] async fn run_action(lib: Lib<'_>, action_id: String, file_ids: Vec<i64>) -> R<JobId> { blocking(&lib, move |l| l.run_action(&action_id, &file_ids)).await }
/// `explicit`: you asked for these games yourself, so any you skipped come back.
#[tauri::command] fn match_metadata(lib: Lib<'_>, game_ids: Vec<i64>, explicit: Option<bool>) -> R<JobId> {
    if explicit.unwrap_or(false) { lib.look_up(&game_ids).map_err(|x| format!("{x:#}")) } else { Ok(lib.match_metadata(&game_ids)) }
}
#[tauri::command] async fn shelf_add(lib: Lib<'_>, system: String, release: String, region: Option<String>, fields: lo_core::shelf::CopyFields) -> R<lo_core::shelf::Copy> { blocking(&lib, move |l| l.shelf_add(&system, &release, region.as_deref(), fields)).await }
#[tauri::command] async fn shelf_update(lib: Lib<'_>, id: String, fields: lo_core::shelf::CopyFields) -> R<lo_core::shelf::Copy> { blocking(&lib, move |l| l.shelf_update(&id, fields)).await }
#[tauri::command] async fn shelf_remove(lib: Lib<'_>, id: String) -> R<()> { blocking(&lib, move |l| l.shelf_remove(&id)).await }
#[tauri::command] async fn shelf_locations(lib: Lib<'_>) -> R<Vec<String>> { blocking(&lib, |l| l.shelf_locations()).await }
#[tauri::command] async fn shelf_releases(lib: Lib<'_>, system: String, query: String) -> R<Vec<ReleaseHit>> { blocking(&lib, move |l| l.shelf_releases(&system, &query, 40)).await }
#[tauri::command] async fn shelf_csv_columns(lib: Lib<'_>, text: String) -> R<(Vec<String>, lo_core::shelf_csv::Mapping)> { blocking(&lib, move |l| l.shelf_csv_columns(&text)).await }
#[tauri::command] async fn shelf_csv_preview(lib: Lib<'_>, text: String, mapping: Option<lo_core::shelf_csv::Mapping>, console: Option<String>) -> R<Vec<lo_core::shelf_csv::ImportRow>> { blocking(&lib, move |l| l.shelf_csv_preview_for(&text, mapping, console.as_deref())).await }
#[tauri::command] async fn shelf_csv_import(lib: Lib<'_>, rows: Vec<lo_core::shelf_csv::ImportRow>) -> R<usize> { blocking(&lib, move |l| l.shelf_csv_import(&rows)).await }
#[tauri::command] async fn completion(lib: Lib<'_>, system: String, options: lo_core::completion::Options) -> R<lo_core::completion::Completion> { blocking(&lib, move |l| l.completion(&system, &options)).await }
#[tauri::command] async fn completion_summary(lib: Lib<'_>, options: lo_core::completion::Options) -> R<Vec<lo_core::completion::Summary>> { blocking(&lib, move |l| l.completion_summary(&options)).await }
#[tauri::command] fn sync_status(lib: Lib<'_>) -> SyncStatus { lib.sync_status() }
#[tauri::command] async fn sync_connect(lib: Lib<'_>, url: String, password: String) -> R<Option<Vec<lo_core::config::RootMap>>> { blocking(&lib, move |l| l.sync_connect(&url, &password)).await }
#[tauri::command] fn sync_restore(lib: Lib<'_>, mapping: Vec<lo_core::config::RootMap>) -> R<JobId> { lib.sync_restore(mapping).map_err(e) }
#[tauri::command] fn sync_now(lib: Lib<'_>) -> R<JobId> { lib.sync_start().map_err(e) }
#[tauri::command] async fn sync_disconnect(lib: Lib<'_>) -> R<()> { blocking(&lib, |l| l.sync_disconnect()).await }
#[tauri::command] fn choose_again(lib: Lib<'_>, game_id: i64) -> R<JobId> { lib.choose_again(game_id).map_err(|x| format!("{x:#}")) }
#[tauri::command] fn skip(lib: Lib<'_>, kind: String, key: String, label: String, system_id: Option<String>) -> R<()> { lib.skip(&kind, &key, &label, system_id.as_deref()).map_err(|x| format!("{x:#}")) }
#[tauri::command] fn unskip(lib: Lib<'_>, kind: String, key: String) -> R<()> { lib.unskip(&kind, &key).map_err(|x| format!("{x:#}")) }
#[tauri::command] fn skipped(lib: Lib<'_>, kind: String) -> R<Vec<lo_core::skips::Skipped>> { lib.skipped(&kind).map_err(|x| format!("{x:#}")) }
#[tauri::command] async fn review_list(lib: Lib<'_>) -> R<Vec<ReviewItem>> { blocking(&lib, |l| l.review_list()).await }
#[tauri::command] async fn choose_metadata(lib: Lib<'_>, game_id: i64, candidate: MetaCandidate) -> R<()> { blocking(&lib, move |l| l.choose_metadata(game_id, candidate)).await }
#[tauri::command] async fn reject_metadata(lib: Lib<'_>, game_id: i64) -> R<()> { blocking(&lib, move |l| l.reject_metadata(game_id)).await }
#[tauri::command] async fn search_metadata(lib: Lib<'_>, game_id: i64, query: String) -> R<Vec<MetaCandidate>> { blocking(&lib, move |l| l.search_metadata(game_id, &query)).await }
#[tauri::command] async fn candidate_preview(lib: Lib<'_>, candidate: MetaCandidate) -> R<Option<String>> { blocking(&lib, move |l| l.candidate_preview(&candidate)).await }
#[tauri::command] async fn sources_status(lib: Lib<'_>) -> R<Vec<lo_core::metadata::SourceStatus>> { blocking(&lib, |l| l.sources_status()).await }
#[tauri::command] fn download_dats(lib: Lib<'_>, systems: Vec<String>) -> JobId { lib.download_dats(systems) }
#[tauri::command] fn check_consoles(lib: Lib<'_>) -> JobId { lib.check_consoles() }
#[tauri::command] fn download_switch_titledb(lib: Lib<'_>) -> JobId { lib.download_switch_titledb() }
#[tauri::command] async fn import_titledb(lib: Lib<'_>, kind: String, path: PathBuf) -> R<usize> { blocking(&lib, move |l| l.import_titledb(&kind, &path)).await }
#[tauri::command] fn system_images(lib: Lib<'_>) -> std::collections::BTreeMap<String, String> { lib.system_images() }
#[tauri::command] async fn fetch_system_images(lib: Lib<'_>) -> R<Option<JobId>> { blocking(&lib, |l| l.fetch_system_images()).await }
#[tauri::command] async fn set_system_image(lib: Lib<'_>, system_id: String, path: Option<PathBuf>) -> R<()> { blocking(&lib, move |l| l.set_system_image(&system_id, path.as_deref())).await }
#[tauri::command] async fn stats(lib: Lib<'_>) -> R<Stats> { blocking(&lib, |l| l.stats()).await }
#[tauri::command] async fn sync_shared(lib: Lib<'_>) -> R<usize> { blocking(&lib, |l| l.sync_shared()).await }

/// Diagnostics from the UI (e.g. images that fail to load) go to the terminal running the app.
#[tauri::command] fn ui_log(line: String) { eprintln!("[ui] {line}"); }

pub fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        startup, get_settings, settings_path, save_settings, completion, completion_summary, shelf_add, shelf_update, shelf_remove, shelf_locations, shelf_releases, shelf_csv_columns, shelf_csv_preview, shelf_csv_import, read_csv_file, sync_status, sync_connect, sync_restore, sync_now, sync_disconnect, systems, all_systems, games, game_ids_for, game_detail, reveal, fetch_game_art, set_play, play_summary, collections, collection, create_collection, update_collection, set_collection_items, add_to_collection, collection_preview, collection_import, collection_export, write_text_file, read_text_file, scan, last_scan,
        jobs_list, job_cancel, jobs_clear, plan_renames, apply_renames, undo, history, hash, set_hash_level, pause_hashing, hash_state,
        import_dat, verify, missing, dupes, junk, compare_contents, layout, fix_layout, archive_paths, archive_files, move_to_tools, move_file, archive_list, archive_purge, archive_purge_expired, archive_purge_all,
        archive_restore, archive_extend, actions, actions_for_games, requirements, run_action, match_metadata, choose_again, skip, unskip, skipped, review_list,
        choose_metadata, reject_metadata, search_metadata, candidate_preview, import_titledb, sources_status, check_consoles, download_dats, download_switch_titledb, system_images, fetch_system_images, set_system_image, stats, sync_shared, ui_log
    ]
}
