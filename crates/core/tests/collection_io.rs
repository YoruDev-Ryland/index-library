//! Collections as files: what an AI (or another library) writes, matched to this library.
mod common;
use lo_core::collection_io::{parse, system_for};
use lo_core::jobs::JobEvent;
use lo_core::library::Library;
use std::sync::Arc;
use std::time::Duration;

fn lib(files: &[&str]) -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    for p in files {
        let full = root.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, p.as_bytes()).unwrap();
    }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    (l, home, root)
}

#[test]
fn systems_by_the_names_people_use() {
    assert_eq!(system_for("3DS").unwrap().id, "n3ds");
    assert_eq!(system_for("Nintendo 3DS").unwrap().id, "n3ds");
    assert_eq!(system_for("Mega Drive").unwrap().id, "genesis");
    assert_eq!(system_for("psx").unwrap().id, "ps1");
    assert_eq!(system_for("switch").unwrap().id, "switch");
    assert!(system_for("Dreamcast").is_none());
}

#[test]
fn reads_an_ai_reply_with_text_around_the_json() {
    let reply = "Here's your collection!\n```json\n{\"name\": \"Zelda\", \"games\": [{\"title\": \"A Link to the Past\", \"system\": \"SNES\"}]}\n```\nEnjoy.";
    let f = parse(reply).unwrap();
    assert_eq!((f.name.as_str(), f.games.len(), f.games[0].system.as_deref()), ("Zelda", 1, Some("SNES")));
    assert!(parse("no json here").is_err());
}

#[test]
fn preview_import_export_round_trip() {
    let (l, _h, root) = lib(&[
        "Sony - PSP/Games/Persona 3 Portable (USA).iso",
        "Sony - PSP/Games/Persona 3 Portable (Europe).iso",
        "Sony - PSP/Games/Shin Megami Tensei - Persona 2 - Innocent Sin (USA).iso",
        "Sony - PSP/Games/Crisis Core - Final Fantasy VII (USA).iso",
    ]);
    let text = r#"{
      "format": "index-collection", "version": 1,
      "name": "Persona on PSP", "notes": "Release order",
      "games": [
        {"title": "Shin Megami Tensei: Persona 2: Innocent Sin", "system": "PSP", "note": "Start here"},
        {"title": "Persona 3 Portable", "system": "PlayStation Portable"},
        {"title": "Crisis Core: Final Fantasy 7", "system": "psp", "note": "Not Persona, but a close title"},
        {"title": "Persona 4 Golden", "system": "Vita", "note": "Not owned yet"}
      ]
    }"#;
    let p = l.collection_preview(text).unwrap();
    let how: Vec<&str> = p.entries.iter().map(|e| e.how.as_str()).collect();
    assert_eq!(how, ["title", "title", "close", "none"], "{p:#?}");
    // your preferred region first, the other region offered as an alternative
    assert_eq!(p.entries[1].matched.as_ref().unwrap().regions, vec!["USA".to_string()]);
    assert_eq!(p.entries[1].alternatives.len(), 1);
    assert_eq!(p.entries[3].system.as_deref(), Some("psvita"));

    let choices: Vec<Option<i64>> = p.entries.iter().map(|e| e.matched.as_ref().map(|m| m.game_id)).collect();
    let id = l.collection_import(text, &choices).unwrap();
    let c = l.collection(&id).unwrap();
    assert_eq!((c.name.as_str(), c.notes.as_deref()), ("Persona on PSP", Some("Release order")));
    assert_eq!(c.items.len(), 4);
    assert_eq!(c.items[0].note.as_deref(), Some("Start here"));
    assert!(c.items[3].game_id.is_none(), "kept by name until the game is in the library");
    assert_eq!(c.items[3].title, "Persona 4 Golden");

    // exported, it reads back as the same collection
    let out = l.collection_export(&id).unwrap();
    let back = parse(&out).unwrap();
    assert_eq!(back.format, "index-collection");
    assert_eq!(back.games.iter().map(|g| g.system.as_deref().unwrap()).collect::<Vec<_>>(), ["psp", "psp", "psp", "psvita"]);

    // the missing game is found once it arrives
    let p4 = root.path().join("Sony - PS Vita/Games/Persona 4 Golden (USA).vpk");
    std::fs::create_dir_all(p4.parent().unwrap()).unwrap();
    std::fs::write(&p4, b"p4g").unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    assert!(l.collection(&id).unwrap().items[3].game_id.is_some());
}
