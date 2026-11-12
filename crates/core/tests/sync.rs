use lo_core::sync::portable::Roots;
use lo_core::sync::record::{Body, Display, Record};
use lo_core::userdata::{GameRef, PlayInfo};
use std::path::PathBuf;

fn roots() -> Roots { Roots { ids: vec![("r1".into(), PathBuf::from("/mnt/lib"))] } }

#[test]
fn dir_keys_round_trip_through_portable_form() {
    let r = roots();
    let p = r.to_portable("dir:/mnt/lib/Sony - PS1/Crash");
    assert_eq!(p, "dir:{root:r1}/Sony - PS1/Crash");
    assert_eq!(r.to_local(&p).as_deref(), Some("dir:/mnt/lib/Sony - PS1/Crash"));
    assert_eq!(r.to_local("dir:{root:r2}/x"), None);
    assert_eq!(r.to_portable("crash bandicoot|usa"), "crash bandicoot|usa");
    assert_eq!(r.to_local("crash bandicoot|usa").as_deref(), Some("crash bandicoot|usa"));
    // a sibling folder sharing the prefix is not inside the root
    assert_eq!(r.to_portable("dir:/mnt/library2/x"), "dir:/mnt/library2/x");
    assert_eq!(r.rel(std::path::Path::new("/mnt/lib/a/b.zip")), Some(("r1".into(), "a/b.zip".into())));
    assert_eq!(r.abs("r1", "a/b.zip"), Some(PathBuf::from("/mnt/lib/a/b.zip")));
}

fn game() -> GameRef { GameRef { system: "snes".into(), key: "super mario world|usa".into(), title: "Super Mario World".into(), ids: vec![] } }

#[test]
fn record_ids_depend_on_content_only() {
    let b = Body::Play { game: game(), info: PlayInfo { status: Some("playing".into()), ..Default::default() } };
    let a = Record::new("m1", 1000, b.clone());
    assert_eq!(a.id, Record::new("m1", 1000, b.clone()).id);
    assert_ne!(a.id, Record::new("m1", 1001, b.clone()).id);
    assert_ne!(a.id, Record::new("m2", 1000, b).id);
}

#[test]
fn records_round_trip_as_json() {
    let bodies = vec![
        Body::Game { game: game(), console: "snes".into(), file_title: "Super Mario World".into(), regions: vec!["USA".into()], removed: false,
            display: Some(Display { title: "Super Mario World".into(), year: Some(1990), ..Default::default() }) },
        Body::Fingerprint { root: "r1".into(), path: "snes/smw.zip".into(), size: 5, mtime_s: 7, crc32: Some("a".into()), sha1: None, md5: None, headerless_crc32: None, headerless_sha1: None },
        Body::Meta { game: game(), state: "matched".into(), meta: Some(serde_json::json!({"title": "x"})), manual: true, cover: None },
        Body::Skip { skip_kind: "match".into(), key: "snes:x".into(), label: "x".into(), system: Some("snes".into()), removed: false },
        Body::Play { game: game(), info: PlayInfo::default() },
        Body::Collection { id: "c".into(), name: "n".into(), notes: None, deleted: false, created: 1 },
        Body::CollectionItems { id: "c".into(), items: vec![] },
        Body::Op { batch_id: "b".into(), op: "move".into(), src: Some("{root:r1}/a".into()), dst: None, label: None, at: 3 },
        Body::Settings { shared: serde_json::json!({"theme": "dark"}) },
    ];
    for b in bodies {
        let r = Record::new("m", 1, b);
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["body"]["kind"], r.body.kind(), "{j}");
        let back: Record = serde_json::from_value(j).unwrap();
        assert_eq!(back, r);
    }
}

#[test]
fn keys_and_standing() {
    let m = Body::Meta { game: game(), state: "matched".into(), meta: None, manual: false, cover: None };
    assert_eq!(m.key(), "meta:snes:super mario world|usa");
    assert!(!m.manual());
    assert!(Body::Play { game: game(), info: PlayInfo::default() }.manual());
    assert_eq!(Body::Settings { shared: serde_json::json!({}) }.key(), "settings");
}

// ---- the desktop's side ----

use lo_core::config::RootMap;
use lo_core::jobs::JobEvent;
use lo_core::library::{HashScope, Library};
use lo_core::sync::export;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// A library with one SNES game, hashed, its root known to sync as "r1".
fn library(root: &std::path::Path, home: &std::path::Path) -> Arc<Library> {
    std::fs::create_dir_all(root.join("Nintendo - SNES")).unwrap();
    if !root.join("Nintendo - SNES/Hit the Ice (USA).sfc").exists() {
        std::fs::write(root.join("Nintendo - SNES/Hit the Ice (USA).sfc"), vec![7u8; 4096]).unwrap();
        std::fs::write(root.join("Nintendo - SNES/Pilotwings (USA).sfc"), vec![9u8; 4096]).unwrap();
    }
    let l = Library::open_at(home, Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.to_path_buf()];
    s.sync.roots = vec![RootMap { id: "r1".into(), path: root.to_path_buf(), origin: "/orig/lib".into() }];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l
}

fn listed() -> HashSet<String> { ["r1".to_string()].into() }

#[test]
fn export_derives_portable_records_for_a_library() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let recs = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap();
    let kinds: Vec<&str> = recs.iter().map(|(r, _)| r.body.kind()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "Game").count(), 2, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == "Fingerprint").count(), 2, "{kinds:?}");
    assert!(kinds.contains(&"Settings"));
    let all = serde_json::to_string(&recs.iter().map(|(r, _)| r).collect::<Vec<_>>()).unwrap();
    assert!(!all.contains(&*root.path().to_string_lossy()), "no absolute paths leave the machine");
    let fp = recs.iter().find_map(|(r, _)| match &r.body { Body::Fingerprint { root, path, sha1, .. } => Some((root.clone(), path.clone(), sha1.clone())), _ => None }).unwrap();
    assert_eq!(fp.0, "r1");
    assert!(fp.1.starts_with("Nintendo - SNES/"));
    assert!(fp.2.is_some());
}

#[test]
fn pending_is_empty_after_marking_sent() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let p = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap();
    assert!(!p.is_empty());
    export::mark_sent(&db, &p).unwrap();
    assert!(export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap().is_empty());
    // a change is pending again
    let gid: i64 = db.conn().query_row("SELECT id FROM games LIMIT 1", [], |r| r.get(0)).unwrap();
    l.set_play(gid, PlayInfo { status: Some("playing".into()), ..Default::default() }).unwrap();
    let p = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap();
    assert_eq!(p.iter().map(|(r, _)| r.body.kind()).collect::<Vec<_>>(), ["Play"]);
}

#[test]
fn unmapped_or_unlisted_roots_report_no_removals() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let p = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap();
    export::mark_sent(&db, &p).unwrap();
    std::fs::remove_file(root.path().join("Nintendo - SNES/Pilotwings (USA).sfc")).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let current = || export::current(&db, &l.settings(), &l.cache_dir).unwrap();
    // the folder couldn't be listed: nothing is said to be gone
    let p = export::pending(&db, &l.machine, current(), &HashSet::new()).unwrap();
    assert!(!p.iter().any(|(r, _)| matches!(&r.body, Body::Game { removed: true, .. })), "{p:?}");
    let p = export::pending(&db, &l.machine, current(), &listed()).unwrap();
    let gone: Vec<&str> = p.iter().filter_map(|(r, _)| match &r.body { Body::Game { removed: true, file_title, .. } => Some(file_title.as_str()), _ => None }).collect();
    assert_eq!(gone, ["Pilotwings"]);
}

#[test]
fn shared_settings_round_trip_without_machine_fields() {
    let mut s = lo_core::config::Settings::default();
    s.theme = "dark".into();
    s.keys.igdb_client_id = "id".into();
    s.temp_dir = Some("/tmp/x".into());
    s.currency = "EUR".into();
    s.sync.url = "http://x".into();
    s.sync.roots = vec![RootMap { id: "r1".into(), path: "/mnt/lib".into(), origin: "/mnt/lib".into() }];
    let j = s.shared_json();
    for f in ["temp_dir", "tools", "python", "scan_concurrency", "hash_level", "sync", "library_roots"] { assert!(j.get(f).is_none(), "{f} is this machine's"); }
    assert_eq!(j["roots"][0]["id"], "r1");
    let mut t = lo_core::config::Settings::default();
    t.apply_shared(&j);
    assert_eq!(t.theme, "dark");
    assert_eq!(t.currency, "EUR");
    assert_eq!(lo_core::config::Settings::default().currency, "USD");
    assert_eq!(t.keys.igdb_client_id, "id");
    assert_eq!(t.temp_dir, None);
}

#[test]
fn manual_match_is_not_replaced_by_an_automatic_record() {
    use lo_core::sync::apply::{apply, Applied};
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let (gid, sys, key, title): (i64, String, String, String) = db.conn().query_row("SELECT id, system_id, group_key, title FROM games ORDER BY title LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    db.conn().execute("UPDATE games SET meta_state='matched', meta_json=?1 WHERE id=?2", (serde_json::json!({"source":"igdb","source_id":"1","title":"Mine","chosen":true}).to_string(), gid)).unwrap();
    let g = GameRef { system: sys, key, title, ids: vec![] };
    let mut s = l.settings();
    let roots = lo_core::sync::roots_of(&s);
    let auto = Record::new("other", lo_core::shared::now_ms() + 60_000, Body::Meta { game: g.clone(), state: "matched".into(), meta: Some(serde_json::json!({"title":"Auto"})), manual: false, cover: None });
    assert_eq!(apply(&db, &roots, &mut s, &l.cache_dir, &auto).unwrap(), Applied::Skipped);
    let t: String = db.conn().query_row("SELECT json_extract(meta_json,'$.title') FROM games WHERE id=?1", [gid], |r| r.get(0)).unwrap();
    assert_eq!(t, "Mine");
    let picked = Record::new("other", lo_core::shared::now_ms() + 60_000, Body::Meta { game: g, state: "matched".into(), meta: Some(serde_json::json!({"title":"Theirs","chosen":true})), manual: true, cover: None });
    assert_eq!(apply(&db, &roots, &mut s, &l.cache_dir, &picked).unwrap(), Applied::Done);
    let t: String = db.conn().query_row("SELECT json_extract(meta_json,'$.title') FROM games WHERE id=?1", [gid], |r| r.get(0)).unwrap();
    assert_eq!(t, "Theirs");
}

#[test]
fn saving_settings_from_the_window_keeps_the_sync_connection() {
    let home = tempfile::tempdir().unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.sync.url = "http://server".into();
    s.sync.token = "secret".into();
    l.save_settings(s).unwrap();
    // the window doesn't know about sync: what it sends has none
    let mut from_window = l.settings_for_window();
    assert!(from_window.sync.token.is_empty(), "the token isn't handed to the window");
    from_window.theme = "dark".into();
    l.save_window_settings(from_window, None).unwrap();
    assert_eq!(l.settings().sync.token, "secret");
    assert_eq!(l.settings().theme, "dark");
}

#[test]
fn a_game_s_ids_are_listed_in_one_order() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    // two files of one game, their ids stored in the opposite order to how they sort
    let gid: i64 = db.conn().query_row("SELECT id FROM games ORDER BY title LIMIT 1", [], |r| r.get(0)).unwrap();
    db.conn().execute("UPDATE files SET game_id=?1", [gid]).unwrap();
    let ids: Vec<i64> = db.conn().prepare("SELECT id FROM files ORDER BY id").unwrap().query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    db.conn().execute("UPDATE files SET probe_json='{\"id\":\"BBBB\"}' WHERE id=?1", [ids[0]]).unwrap();
    db.conn().execute("UPDATE files SET probe_json='{\"id\":\"AAAA\"}' WHERE id=?1", [ids[1]]).unwrap();
    assert_eq!(lo_core::userdata::game_ref(&db, gid).unwrap().ids, ["AAAA", "BBBB"]);
    let recs = export::current(&db, &l.settings(), &l.cache_dir).unwrap();
    let g = recs.iter().find_map(|d| match &d.body { Body::Game { game, .. } if !game.ids.is_empty() => Some(game.ids.clone()), _ => None }).unwrap();
    assert_eq!(g, ["AAAA", "BBBB"]);
}

// ---- review fixes ----

#[test]
fn keys_holding_several_paths_are_portable_throughout() {
    let r = Roots { ids: vec![("r1".into(), PathBuf::from("/mnt/lib")), ("r2".into(), PathBuf::from("/mnt/lib/sub"))] };
    let k = "SameData\n/mnt/lib/snes/a.sfc\n/mnt/lib/sub/b.sfc\n/mnt/lib/snes/c.sfc";
    let p = r.to_portable(k);
    assert_eq!(p, "SameData\n{root:r1}/snes/a.sfc\n{root:r2}/b.sfc\n{root:r1}/snes/c.sfc");
    assert_eq!(r.to_local(&p).as_deref(), Some(k));
    let other = Roots { ids: vec![("r1".into(), PathBuf::from("/media/nas")), ("r2".into(), PathBuf::from("/media/sub"))] };
    assert_eq!(other.to_local(&p).as_deref(), Some("SameData\n/media/nas/snes/a.sfc\n/media/sub/b.sfc\n/media/nas/snes/c.sfc"));
    let half = Roots { ids: vec![("r1".into(), PathBuf::from("/media/nas"))] };
    assert_eq!(half.to_local(&p), None, "a key naming a folder not mapped here isn't half-translated");
}

#[test]
fn a_window_save_changes_only_what_the_window_changed() {
    let home = tempfile::tempdir().unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec!["/mnt/lib".into()];
    s.keys.rawg = "key".into();
    l.save_settings(s).unwrap();
    let window = l.settings_for_window();
    // meanwhile another device's settings arrive, and a restore maps the folders
    let mut s = l.settings();
    s.theme = "dark".into();
    s.keys.rawg = "newer key".into();
    s.library_roots = vec!["/media/nas".into()];
    l.save_settings(s).unwrap();
    // the window, still showing what it loaded, stars a system
    let mut w = window.clone();
    w.favorite_systems = vec!["snes".into()];
    l.save_window_settings(w, Some(window)).unwrap();
    let now = l.settings();
    assert_eq!(now.favorite_systems, ["snes"]);
    assert_eq!(now.theme, "dark");
    assert_eq!(now.keys.rawg, "newer key");
    assert_eq!(now.library_roots, [PathBuf::from("/media/nas")]);
}

fn meta_rec(g: &GameRef, title: &str, manual: bool, ts: i64) -> Record {
    Record::new("other", ts, Body::Meta { game: g.clone(), state: "matched".into(), meta: Some(serde_json::json!({"title": title, "chosen": manual})), manual, cover: None })
}

#[test]
fn a_manual_record_beats_a_newer_automatic_one_here_too() {
    use lo_core::sync::apply::{apply, Applied};
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let (gid, sys, key, title): (i64, String, String, String) = db.conn().query_row("SELECT id, system_id, group_key, title FROM games ORDER BY title LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
    let g = GameRef { system: sys, key, title, ids: vec![] };
    let mut s = l.settings();
    let roots = lo_core::sync::roots_of(&s);
    // this PC's own automatic match, sent at t=2000
    db.conn().execute("UPDATE games SET meta_state='matched', meta_json='{\"title\":\"Auto\"}' WHERE id=?1", [gid]).unwrap();
    export::mark_sent(&db, &[(Record::new("me", 2000, Body::Meta { game: g.clone(), state: "matched".into(), meta: Some(serde_json::json!({"title":"Auto"})), manual: false, cover: None }), None)]).unwrap();
    // the owner's pick from another PC, made earlier (t=1000)
    assert_eq!(apply(&db, &roots, &mut s, &l.cache_dir, &meta_rec(&g, "Picked", true, 1000)).unwrap(), Applied::Done);
    let t: String = db.conn().query_row("SELECT json_extract(meta_json,'$.title') FROM games WHERE id=?1", [gid], |r| r.get(0)).unwrap();
    assert_eq!(t, "Picked");
}

#[test]
fn records_are_applied_to_the_game_with_that_key_only() {
    use lo_core::sync::apply::{apply, Applied};
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let (sys, title): (String, String) = db.conn().query_row("SELECT system_id, title FROM games ORDER BY title LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let mut s = l.settings();
    let roots = lo_core::sync::roots_of(&s);
    // the same title, under a key this library doesn't have (a folder since renamed)
    let stale = GameRef { system: sys, key: "dir:{root:r1}/old name".into(), title, ids: vec![] };
    assert_eq!(apply(&db, &roots, &mut s, &l.cache_dir, &meta_rec(&stale, "Stale", true, 1)).unwrap(), Applied::Waiting);
    let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM games WHERE meta_json LIKE '%Stale%'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn a_resent_change_keeps_its_id() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let ids = || { let mut v: Vec<String> = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap().into_iter().map(|(r, _)| r.id).collect(); v.sort(); v };
    let first = ids();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(ids(), first, "not acknowledged, sent again: the same records");
}

#[test]
fn a_look_up_you_asked_for_is_your_decision() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    db.conn().execute("UPDATE games SET meta_state='matched', meta_json='{\"title\":\"X\",\"asked\":1}'", []).unwrap();
    let recs = export::current(&db, &l.settings(), &l.cache_dir).unwrap();
    assert!(recs.iter().filter_map(|d| match &d.body { Body::Meta { manual, .. } => Some(*manual), _ => None }).all(|m| m));
}

#[test]
fn a_copy_of_a_game_in_a_folder_not_mapped_here_still_arrives_and_isn_t_echoed() {
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let l = library(root.path(), home.path());
    let db = l.db().unwrap();
    let mut s = l.settings();
    let copy = lo_core::shelf::Copy { id: "c1".into(), game: GameRef { system: "ps1".into(), key: "dir:{root:elsewhere}/Sony - PS1/Crash Bandicoot".into(), title: "Crash Bandicoot".into(), ids: vec![] },
        release: "Crash Bandicoot (USA)".into(), cart: true, box_: true, manual: true, ..Default::default() };
    let r = Record::new("phone", 5000, Body::ShelfCopy { copy });
    assert_eq!(lo_core::sync::run::receive(&db, &mut s, &l.cache_dir, &r).unwrap(), lo_core::sync::apply::Applied::Done);
    let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM games WHERE system_id='ps1' AND title='Crash Bandicoot'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "on the game for its release");
    let p = export::pending(&db, &l.machine, export::current(&db, &l.settings(), &l.cache_dir).unwrap(), &listed()).unwrap();
    assert!(!p.iter().any(|(r, _)| matches!(r.body, Body::ShelfCopy { .. })), "not sent back: {p:?}");
}
