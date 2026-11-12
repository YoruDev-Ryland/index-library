#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod commands;

use lo_core::jobs::JobEvent;
use lo_core::library::Library;
use std::sync::Arc;
use tauri::{Emitter, Manager};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let sink = Arc::new(move |e: JobEvent| { let _ = handle.emit("job", &e); });
            // INDEX_HOME=/some/dir runs fully portable (settings, db, cache in one folder).
            let lib = match std::env::var_os("INDEX_HOME") {
                Some(h) => Library::open_at(std::path::Path::new(&h), sink)?,
                None => Library::open(sink)?,
            };
            // No white flash: the window starts hidden with the app's own ground colour behind the
            // page, and the page shows it after its first paint. If that never comes (a broken
            // page), it's shown anyway after a moment.
            if let Some(win) = app.get_webview_window("main") {
                let theme = lib.settings().theme;
                let dark = theme == "dark" || (theme != "light" && matches!(win.theme(), Ok(tauri::Theme::Dark)));
                let ground = if dark { tauri::window::Color(0x19, 0x19, 0x18, 0xff) } else { tauri::window::Color(0xee, 0xec, 0xe7, 0xff) };
                let _ = win.set_background_color(Some(ground));
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if !win.is_visible().unwrap_or(true) { let _ = win.show(); }
                });
            }
            app.manage(lib);
            Ok(())
        })
        .invoke_handler(commands::handlers())
        .run(tauri::generate_context!())
        .expect("error while running Index");
}
