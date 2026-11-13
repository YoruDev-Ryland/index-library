//! Updating Index from its GitHub releases: a check (the GitHub API, else latest.json), then the
//! updater plugin downloads and verifies the new version in the background; it's installed when
//! you choose Restart to update.
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt;

/// A downloaded update waiting for Restart to update.
#[derive(Default)]
pub struct Pending(Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current: String,
    pub available: Option<lo_core::update::Available>,
    /// this copy can update itself (not a .deb or other package-manager install)
    pub can_install: bool,
    /// the version downloaded and ready to install
    pub ready: Option<String>,
}

/// Whether the updater can replace this copy: Windows and macOS builds, and the Linux AppImage.
pub fn can_install() -> bool {
    if cfg!(target_os = "linux") { std::env::var_os("APPIMAGE").is_some() } else { true }
}

fn e<E: std::fmt::Display>(err: E) -> String { format!("{err:#}") }

#[tauri::command]
pub async fn update_check(app: AppHandle, pending: State<'_, Pending>) -> Result<UpdateStatus, String> {
    let current = app.package_info().version.to_string();
    let cur = current.clone();
    let available = tauri::async_runtime::spawn_blocking(move || lo_core::update::check(&cur)).await.map_err(e)?.map_err(e)?;
    let ready = pending.0.lock().unwrap().as_ref().map(|(u, _)| u.version.clone());
    Ok(UpdateStatus { current, available, can_install: can_install(), ready })
}

/// Download and verify the new version (it waits until you restart). Returns its version.
#[tauri::command]
pub async fn update_download(app: AppHandle, pending: State<'_, Pending>) -> Result<Option<String>, String> {
    if !can_install() { return Err("this copy of Index was installed by your system's package manager: update it there".into()); }
    if let Some((u, _)) = pending.0.lock().unwrap().as_ref() { return Ok(Some(u.version.clone())); }
    let Some(update) = app.updater().map_err(e)?.check().await.map_err(e)? else { return Ok(None) };
    let bytes = update.download(|_, _| {}, || {}).await.map_err(e)?;
    let v = update.version.clone();
    *pending.0.lock().unwrap() = Some((update, bytes));
    Ok(Some(v))
}

/// Install the downloaded version and start it.
#[tauri::command]
pub async fn update_install(app: AppHandle, pending: State<'_, Pending>) -> Result<(), String> {
    let Some((update, bytes)) = pending.0.lock().unwrap().take() else { return Err("no update is downloaded".into()) };
    update.install(bytes).map_err(e)?;
    app.restart();
}

/// Open one of Index's GitHub pages (a release's "What's new") in the browser.
#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), String> {
    if !lo_core::update::is_release_page(&url) { return Err("not an Index page".into()); }
    lo_core::util::open_url(&url).map_err(e)
}
