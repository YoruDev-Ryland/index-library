//! The server as a recovery point: a library syncs up; a brand-new PC with the same files (at
//! another path) connects and ends where the first one was, without fingerprinting again.
use index_server::{api, auth::Auth, store::Store};
use lo_core::config::RootMap;
use lo_core::jobs::JobEvent;
use lo_core::library::{HashScope, Library};
use lo_core::userdata::PlayInfo;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn serve(dir: &Path) -> String {
    let store = Arc::new(Store::open(dir).unwrap());
    let auth = Arc::new(Auth::new(dir, Some("pw"), "").unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(l.local_addr().unwrap()).unwrap();
            axum::serve(l, api::router(store, auth).into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
        });
    });
    format!("http://{}", rx.recv().unwrap())
}

fn open(home: &Path) -> Arc<Library> { Library::open_at(home, Arc::new(|_e: JobEvent| {})).unwrap() }
fn idle(l: &Library) { assert!(l.jobs.wait_idle(Duration::from_secs(60))); }

fn snapshot(l: &Library) -> BTreeMap<String, String> {
    let db = l.db().unwrap();
    let mut m = BTreeMap::new();
    let mut q = db.conn().prepare("SELECT system_id || ':' || group_key, COALESCE(meta_state,''), COALESCE(meta_json,'') FROM games").unwrap();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap() {
        let (k, s, j) = r.unwrap();
        m.insert(format!("meta {k}"), format!("{s} {}", serde_json::from_str::<serde_json::Value>(&j).unwrap_or_default()));
    }
    let mut q = db.conn().prepare("SELECT kind, key, label FROM skipped").unwrap();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap() { let (a, b, c) = r.unwrap(); m.insert(format!("skip {a} {b}"), c); }
    let mut q = db.conn().prepare("SELECT ref, COALESCE(status,''), COALESCE(score,0) FROM play").unwrap();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))).unwrap() { let (a, b, c) = r.unwrap(); m.insert(format!("play {a}"), format!("{b} {c}")); }
    let mut q = db.conn().prepare("SELECT c.name, i.pos, i.ref FROM collections c JOIN collection_items i ON i.collection_id=c.id").unwrap();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))).unwrap() { let (a, b, c) = r.unwrap(); m.insert(format!("coll {a} {b}"), c); }
    let mut q = db.conn().prepare("SELECT name, COALESCE(sha1,''), COALESCE(crc32,'') FROM files").unwrap();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap() { let (a, b, c) = r.unwrap(); m.insert(format!("file {a}"), format!("{b} {c}")); }
    m
}

/// Copy a folder, keeping modification times (as a NAS seen from another PC would).
fn copy_tree(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dst = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() { std::fs::create_dir_all(&dst).unwrap(); copy_tree(&e.path(), &dst); }
        else {
            std::fs::copy(e.path(), &dst).unwrap();
            let m = std::fs::metadata(e.path()).unwrap().modified().unwrap();
            std::fs::File::options().write(true).open(&dst).unwrap().set_modified(m).unwrap();
        }
    }
}

#[test]
fn recovery_restores_matches_skips_play_and_fingerprints_without_rehashing() {
    let server_dir = tempfile::tempdir().unwrap();
    let url = serve(server_dir.path());
    let (root_a, home_a, root_b, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let snes = root_a.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&snes).unwrap();
    for (n, b) in [("Hit the Ice (USA).sfc", 3u8), ("Pilotwings (USA).sfc", 5), ("F-Zero (USA).sfc", 7)] { std::fs::write(snes.join(n), vec![b; 8192]).unwrap(); }

    // PC A: scanned, fingerprinted, a pick, a "no match", play data and a collection
    let a = open(home_a.path());
    let mut s = a.settings();
    s.library_roots = vec![root_a.path().to_path_buf()];
    s.theme = "dark".into();
    a.save_settings(s).unwrap();
    a.scan(false); idle(&a);
    a.hash(HashScope::All).unwrap(); idle(&a);
    let db = a.db().unwrap();
    let id = |t: &str| -> i64 { db.conn().query_row("SELECT id FROM games WHERE title=?1", [t], |r| r.get(0)).unwrap() };
    db.conn().execute("UPDATE games SET meta_state='matched', meta_json=?1 WHERE id=?2", (serde_json::json!({"source":"igdb","source_id":"1","title":"Hit the Ice","chosen":true,"release":"1990-01-01"}).to_string(), id("Hit the Ice"))).unwrap();
    db.conn().execute("UPDATE games SET meta_state='matched', meta_json=?1 WHERE id=?2", (serde_json::json!({"source":"igdb","source_id":"2","title":"Pilotwings"}).to_string(), id("Pilotwings"))).unwrap();
    a.reject_metadata(id("F-Zero")).unwrap();
    a.set_play(id("Pilotwings"), PlayInfo { status: Some("finished".into()), score: Some(9), ..Default::default() }).unwrap();
    let c = a.create_collection("Racing", &[id("F-Zero")]).unwrap();
    a.add_to_collection(&c, &[id("Hit the Ice")]).unwrap();
    assert!(a.sync_connect(&url, "pw").unwrap().is_none(), "a library on a server without one uploads, it doesn't restore");
    idle(&a); // the upload runs as a job
    let up = a.sync_status().last.expect("synced");
    assert!(up.pushed > 0, "{up:?}");
    let before = snapshot(&a);

    // PC B: brand new, the same files mounted elsewhere
    copy_tree(root_a.path(), root_b.path());
    let b = open(home_b.path());
    let roots = b.sync_connect(&url, "pw").unwrap().expect("an empty library is offered a restore");
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].origin, lo_core::util::p2s(root_a.path()));
    b.sync_restore(vec![RootMap { path: root_b.path().to_path_buf(), ..roots[0].clone() }]).unwrap();
    idle(&b);

    assert_eq!(snapshot(&b), before);
    assert_eq!(b.settings().theme, "dark");
    let unhashed: i64 = b.db().unwrap().conn().query_row("SELECT COUNT(*) FROM files WHERE hash_key IS NULL OR hash_key != size || ':' || mtime", [], |r| r.get(0)).unwrap();
    assert_eq!(unhashed, 0, "nothing needs fingerprinting again");

    // and nothing B received goes back to the server
    let rep = b.sync_now().unwrap();
    assert_eq!(rep.pushed, 0, "{rep:?}");

    // a phone-style change from B reaches A
    let bdb = b.db().unwrap();
    let pw: i64 = bdb.conn().query_row("SELECT id FROM games WHERE title='Pilotwings'", [], |r| r.get(0)).unwrap();
    b.set_play(pw, PlayInfo { status: Some("completed".into()), score: Some(10), ..Default::default() }).unwrap();
    b.sync_now().unwrap();
    let rep = a.sync_now().unwrap();
    assert!(rep.applied >= 1, "{rep:?}");
    assert_eq!(lo_core::userdata::play_of(&a.db().unwrap(), id("Pilotwings")).unwrap().unwrap().status.as_deref(), Some("completed"));
}

#[test]
fn a_new_pc_that_already_scanned_is_offered_the_restore_and_keys_stay() {
    let server_dir = tempfile::tempdir().unwrap();
    let url = serve(server_dir.path());
    let (root_a, home_a, root_b, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let snes = root_a.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&snes).unwrap();
    std::fs::write(snes.join("Pilotwings (USA).sfc"), vec![5u8; 8192]).unwrap();
    let a = open(home_a.path());
    let mut s = a.settings();
    s.library_roots = vec![root_a.path().to_path_buf()];
    s.keys.rawg = "the key".into();
    a.save_settings(s).unwrap();
    a.scan(false); idle(&a);
    a.hash(HashScope::All).unwrap(); idle(&a);
    assert!(a.sync_connect(&url, "pw").unwrap().is_none());
    a.sync_now().unwrap();

    // B went through first-run setup: its folder is chosen and scanned before it connects
    copy_tree(root_a.path(), root_b.path());
    let b = open(home_b.path());
    let mut s = b.settings();
    s.library_roots = vec![root_b.path().to_path_buf()];
    b.save_settings(s).unwrap();
    b.scan(false); idle(&b);
    let offer = b.sync_connect(&url, "pw").unwrap().expect("a PC that never synced is offered the server's library");
    // closing the app before choosing the folders keeps the offer
    drop(b);
    let b = open(home_b.path());
    let st = b.sync_status();
    assert!(st.restore_pending);
    assert_eq!(st.offer.len(), 1);
    assert_eq!(st.offer[0].id, offer[0].id);
    // nothing goes up before the restore
    assert!(b.sync_now().is_err());
    b.sync_restore(vec![RootMap { path: root_b.path().to_path_buf(), ..offer[0].clone() }]).unwrap();
    idle(&b);
    assert_eq!(b.settings().keys.rawg, "the key");
    b.sync_now().unwrap();
    a.sync_now().unwrap();
    assert_eq!(a.settings().keys.rawg, "the key", "A keeps its keys");
    let unhashed: i64 = b.db().unwrap().conn().query_row("SELECT COUNT(*) FROM files WHERE hash_key IS NULL OR hash_key != size || ':' || mtime", [], |r| r.get(0)).unwrap();
    assert_eq!(unhashed, 0);
}

#[test]
fn the_sync_status_says_what_happened_and_survives_a_restart() {
    let server_dir = tempfile::tempdir().unwrap();
    let url = serve(server_dir.path());
    let (root, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let snes = root.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&snes).unwrap();
    std::fs::write(snes.join("Pilotwings (USA).sfc"), vec![5u8; 8192]).unwrap();
    let a = open(home.path());
    let mut s = a.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    a.save_settings(s).unwrap();
    a.scan(false); idle(&a);
    a.hash(HashScope::All).unwrap(); idle(&a);
    assert!(a.sync_connect(&url, "pw").unwrap().is_none());
    idle(&a);
    // "Sync now" is a job you can watch in the drawer
    let job = a.sync_start().unwrap();
    idle(&a);
    let j = a.jobs.get(job).unwrap();
    assert!(j.log.iter().any(|l| l.contains("up to date") || l.contains("sent")), "{:?}", j.log);
    drop(a);
    let a = open(home.path());
    let st = a.sync_status();
    assert!(st.last_at.is_some(), "the last sync is remembered");
    assert_eq!(st.error, None);
    assert_eq!(st.pending, Some(0), "nothing waiting to go up");
    assert!(st.running.is_none());
    let first = st.activity.iter().find(|x| x.sent.get("Fingerprint").copied().unwrap_or(0) > 0).expect("the upload is in the activity list");
    assert!(first.sent.get("Game").copied().unwrap_or(0) > 0);
}

#[test]
fn shelf_copies_sync_and_deletions_stay_deleted() {
    use lo_core::shelf::CopyFields;
    let server_dir = tempfile::tempdir().unwrap();
    let url = serve(server_dir.path());
    let (root_a, home_a, root_b, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let snes = root_a.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&snes).unwrap();
    std::fs::write(snes.join("Pilotwings (USA).sfc"), vec![5u8; 8192]).unwrap();
    let a = open(home_a.path());
    let mut s = a.settings();
    s.library_roots = vec![root_a.path().to_path_buf()];
    a.save_settings(s).unwrap();
    a.scan(false); idle(&a);
    a.hash(HashScope::All).unwrap(); idle(&a);
    let pw = a.shelf_add("snes", "Pilotwings (USA)", None, CopyFields { cart: true, box_: true, manual: true, location: Some("Shelf 3".into()), ..Default::default() }).unwrap();
    let eb = a.shelf_add("snes", "EarthBound (USA)", None, CopyFields { sealed: true, ..Default::default() }).unwrap();
    assert!(a.sync_connect(&url, "pw").unwrap().is_none());
    idle(&a);

    copy_tree(root_a.path(), root_b.path());
    let b = open(home_b.path());
    let offer = b.sync_connect(&url, "pw").unwrap().unwrap();
    b.sync_restore(vec![RootMap { path: root_b.path().to_path_buf(), ..offer[0].clone() }]).unwrap();
    idle(&b);
    let copies = |l: &Library| -> Vec<(String, String, bool)> {
        let db = l.db().unwrap();
        let mut q = db.conn().prepare("SELECT c.release, COALESCE(c.location,''), EXISTS(SELECT 1 FROM games g WHERE g.system_id || ':' || g.group_key = c.ref) FROM shelf_copies c WHERE c.deleted=0 ORDER BY c.release").unwrap();
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|x| x.unwrap()).collect()
    };
    assert_eq!(copies(&b), [("EarthBound (USA)".into(), "".into(), true), ("Pilotwings (USA)".into(), "Shelf 3".into(), true)], "copies and the shelf-only game come back");
    assert_eq!(b.sync_now().unwrap().pushed, 0, "nothing echoes back");

    // removed on B: gone on A, and stays gone
    b.shelf_remove(&eb.id).unwrap();
    b.sync_now().unwrap();
    a.sync_now().unwrap();
    assert_eq!(copies(&a).len(), 1);
    let eb_game: i64 = a.db().unwrap().conn().query_row("SELECT COUNT(*) FROM games WHERE title='EarthBound'", [], |r| r.get(0)).unwrap();
    assert_eq!(eb_game, 0, "the shelf-only game went with its copy");
    a.sync_now().unwrap(); b.sync_now().unwrap(); a.sync_now().unwrap();
    assert_eq!(copies(&a).len(), 1);
    assert_eq!(copies(&b).len(), 1);
    let _ = pw;
}
