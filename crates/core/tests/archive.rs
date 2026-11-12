mod common;
use common::*;
use lo_core::archive::{archive_file, expired, extend, list, purge, rebuild_from_manifests, restore};
use lo_core::db::Db;
use lo_core::fsx::LocalFs;
use lo_core::ops::undo_batch;
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::Journal;
use lo_core::shared::manifest::Manifest;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

const DAY: i64 = 86_400;

fn setup() -> (Fixture, Journal, std::path::PathBuf) {
    let f = Fixture::new();
    let p = f.put("Nintendo - 3DS/Game (USA).cia", b"encrypted");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let j = Journal::new(Arc::new(LocalFs), f.settings.library_roots.clone(), "m1".into());
    (f, j, p)
}

#[test]
fn archive_moves_file_and_writes_manifest() {
    let (f, j, p) = setup();
    let a = archive_file(&f.db, &LocalFs, &j, &p, "decrypted", Some("n3ds.decrypt_cia"), 7).unwrap();
    assert_eq!(a.path, f.root().join("Nintendo - 3DS/_archive/Game (USA).cia"));
    assert!(!p.exists() && a.path.exists());
    let m = Manifest::load(&LocalFs, &f.root().join("Nintendo - 3DS/_archive")).unwrap();
    assert_eq!(m.entries.len(), 1);
    assert_eq!(m.entries[0].original_path, "Nintendo - 3DS/Game (USA).cia");
    assert_eq!(m.entries[0].delete_after - m.entries[0].archived_at, 7 * DAY);
    assert_eq!(f.count("SELECT count(*) FROM files"), 0, "archived files leave the library index");
    // a second file with the same name gets a numbered slot
    let p2 = f.put("Nintendo - 3DS/Game (USA).cia", b"again");
    let b = archive_file(&f.db, &LocalFs, &j, &p2, "dup", None, 7).unwrap();
    assert_eq!(b.path.file_name().unwrap(), "Game (USA) (2).cia");
}

#[test]
fn expiry_purge_restore_extend() {
    let (f, j, p) = setup();
    let a = archive_file(&f.db, &LocalFs, &j, &p, "decrypted", None, 7).unwrap();
    let now = a.archived_at;
    assert!(expired(&f.db, now + 6 * DAY).unwrap().is_empty());
    assert_eq!(expired(&f.db, now + 8 * DAY).unwrap().len(), 1);
    extend(&f.db, &LocalFs, &j, a.id, 7).unwrap();
    assert!(expired(&f.db, now + 8 * DAY).unwrap().is_empty());
    restore(&f.db, &LocalFs, &j, a.id).unwrap();
    assert!(p.exists());
    assert!(list(&f.db).unwrap().is_empty());
    let b = archive_file(&f.db, &LocalFs, &j, &p, "decrypted", None, 7).unwrap();
    let r = purge(&f.db, &LocalFs, &j, &[b.id]).unwrap();
    assert_eq!(r.deleted, 1);
    assert!(!b.path.exists());
    assert!(Manifest::load(&LocalFs, &f.root().join("Nintendo - 3DS/_archive")).unwrap().entries.is_empty());
}

#[test]
fn undo_of_archive_restores_and_forgets() {
    let (f, j, p) = setup();
    let a = archive_file(&f.db, &LocalFs, &j, &p, "x", None, 7).unwrap();
    undo_batch(&f.db, &LocalFs, &j, &a.batch_id).unwrap();
    assert!(p.exists());
    assert!(list(&f.db).unwrap().is_empty());
    assert!(Manifest::load(&LocalFs, &f.root().join("Nintendo - 3DS/_archive")).unwrap().entries.is_empty());
}

#[test]
fn fresh_db_rebuilds_from_manifests() {
    let (f, j, p) = setup();
    archive_file(&f.db, &LocalFs, &j, &p, "decrypted", None, 7).unwrap();
    let other = Db::open_in_memory().unwrap();
    assert_eq!(rebuild_from_manifests(&other, &LocalFs, &f.settings.library_roots).unwrap(), 1);
    assert_eq!(list(&other).unwrap()[0].original_path, p);
}
