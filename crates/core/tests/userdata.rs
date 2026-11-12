//! Play status and collections, and that they follow you to another PC using the same library.
mod common;
use lo_core::jobs::JobEvent;
use lo_core::library::{GameFilter, GameSort, Library};
use lo_core::userdata::PlayInfo;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn pc(root: &Path) -> (Arc<Library>, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    (l, home)
}

fn library() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for t in ["Atelier Ryza (USA)", "Atelier Sophie (USA)", "Atelier Lulua (USA)", "Atelier Nelke (USA)"] {
        let p = root.path().join("Sony - PSP/Games").join(format!("{t}.iso"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t.as_bytes()).unwrap();
    }
    root
}

fn id_of(l: &Library, title: &str) -> i64 {
    l.games(&GameFilter { search: Some(title.into()), ..Default::default() }, GameSort::Title, 0, 10).unwrap().rows[0].id
}

#[test]
fn play_status_and_collections() {
    let root = library();
    let (a, _h) = pc(root.path());
    let (ryza, sophie, lulua, nelke) = (id_of(&a, "Ryza"), id_of(&a, "Sophie"), id_of(&a, "Lulua"), id_of(&a, "Nelke"));

    let p = a.set_play(ryza, PlayInfo { status: Some("playing".into()), ..Default::default() }).unwrap();
    assert!(p.started.is_some(), "starting to play fills in the start date");
    let p = a.set_play(ryza, PlayInfo { status: Some("finished".into()), score: Some(9), notes: Some("great".into()), started: p.started, ..Default::default() }).unwrap();
    assert!(p.finished.is_some());
    a.set_play(nelke, PlayInfo { status: Some("skip".into()), notes: Some("reviews are poor".into()), ..Default::default() }).unwrap();

    let c = a.create_collection("Atelier", &[ryza, sophie, nelke, lulua]).unwrap();
    // play order: Sophie, Ryza, Lulua — Nelke skipped
    let d = a.collection(&c).unwrap();
    let key = |t: &str| d.items.iter().find(|i| i.title.contains(t)).unwrap().r#ref.clone();
    a.set_collection_items(&c, &[(key("Sophie"), None), (key("Ryza"), None), (key("Lulua"), Some("after Ryza 2".into())), (key("Nelke"), None)]).unwrap();
    let d = a.collection(&c).unwrap();
    assert_eq!(d.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Atelier Sophie", "Atelier Ryza", "Atelier Lulua", "Atelier Nelke"]);
    assert_eq!(d.items[2].note.as_deref(), Some("after Ryza 2"));
    // a game with only an icon (Switch title database art) still shows a picture, as in the library
    let conn = rusqlite::Connection::open(_h.path().join("data/library.db")).unwrap();
    conn.execute("UPDATE games SET art_json='{\"icon\":\"/art/sophie-icon.jpg\"}' WHERE id=?1", [sophie]).unwrap();
    assert_eq!(a.collection(&c).unwrap().items[0].cover.as_deref(), Some("/art/sophie-icon.jpg"));
    let s = a.collections().unwrap();
    assert_eq!((s[0].count, s[0].done, s[0].skipped, s[0].next.as_deref(), s[0].system.as_deref()), (4, 1, 1, Some("Atelier Sophie"), Some("psp")));

    // the library list knows your status and score, and filters by it
    let rows = a.games(&GameFilter { play: Some("finished".into()), ..Default::default() }, GameSort::Title, 0, 10).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].play.as_deref(), rows[0].score), (Some("finished"), Some(9)));
    let detail = a.game_detail(lulua).unwrap();
    assert_eq!(detail.collections.len(), 1);

    // the Overview's summary
    let sum = a.play_summary().unwrap();
    assert_eq!((sum.counts.get("finished"), sum.counts.get("skip")), (Some(&1), Some(&1)));
    assert_eq!(sum.finished[0].title, "Atelier Ryza");
    assert_eq!(sum.average_score, Some(9.0));

    // another PC using the same library sees all of it after syncing
    let (b, _h2) = pc(root.path());
    b.sync_shared().unwrap();
    let d2 = b.collection(&c).unwrap();
    assert_eq!(d2.name, "Atelier");
    assert_eq!(d2.items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), ["Atelier Sophie", "Atelier Ryza", "Atelier Lulua", "Atelier Nelke"]);
    assert!(d2.items.iter().all(|i| i.game_id.is_some()), "entries point at that PC's games");
    let p2 = b.game_detail(id_of(&b, "Ryza")).unwrap().play.unwrap();
    assert_eq!((p2.status.as_deref(), p2.score, p2.notes.as_deref()), (Some("finished"), Some(9), Some("great")));

    // and its changes come back: newest wins
    b.set_play(id_of(&b, "Sophie"), PlayInfo { status: Some("playing".into()), ..Default::default() }).unwrap();
    b.update_collection(&c, "Atelier (in order)", Some("Mysterious trilogy first".into()), false).unwrap();
    a.sync_shared().unwrap();
    assert_eq!(a.collection(&c).unwrap().name, "Atelier (in order)");
    assert_eq!(a.game_detail(sophie).unwrap().play.unwrap().status.as_deref(), Some("playing"));

    // deleting a collection deletes it everywhere
    a.update_collection(&c, "", None, true).unwrap();
    b.sync_shared().unwrap();
    assert!(b.collections().unwrap().is_empty());
}
