mod common;
use common::*;
use lo_core::db::Db;
use lo_core::fsx::{LocalFs, Transport};
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::{game_key, sync, EntryKind, Journal};
use lo_core::shared::lease::Lease;
use lo_core::shared::manifest::{Manifest, ManifestEntry};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

fn fs() -> Arc<dyn Transport> { Arc::new(LocalFs) }

#[test]
fn status_entries_merge_exactly_once() {
    let f = Fixture::new();
    f.put("Nintendo - NES/a (U).zip", b"a");
    f.put("Nintendo - NES/b (U).zip", b"b");
    let db_b = Db::open_in_memory().unwrap();
    for db in [&f.db, &db_b] {
        scan(db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    }
    let roots = f.settings.library_roots.clone();
    let ja = Journal::new(fs(), roots.clone(), "machine-a".into());
    let gid_a: i64 = f.count("SELECT game_id FROM files WHERE name='a (U).zip'");
    let gid_b: i64 = f.count("SELECT game_id FROM files WHERE name='b (U).zip'");
    for g in [gid_a, gid_b] {
        let key = game_key(&f.db, &roots, g).unwrap();
        ja.append(&EntryKind::Status { game_key: key, key: "decrypted".into(), value: "true".into() }).unwrap();
    }
    assert_eq!(sync(&db_b, &LocalFs, &roots, "machine-b").unwrap(), 2);
    assert_eq!(sync(&db_b, &LocalFs, &roots, "machine-b").unwrap(), 0);
    let n: i64 = db_b.conn().query_row("SELECT count(*) FROM status WHERE key='decrypted' AND value='true'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    // own journal is never re-applied
    assert_eq!(sync(&f.db, &LocalFs, &roots, "machine-a").unwrap(), 0);
    // paths in the journal are root-relative (portable across OSes)
    let text = std::fs::read_to_string(f.root().join(".index-library/journal/machine-a.jsonl")).unwrap();
    assert!(!text.contains(&*f.root().to_string_lossy()), "{text}");
}

#[test]
fn partial_trailing_line_is_not_consumed() {
    let f = Fixture::new();
    let roots = f.settings.library_roots.clone();
    let p = f.root().join(".index-library/journal/machine-a.jsonl");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"{\"ts\":1,\"machine\":\"machine-a\",\"type\":\"ArchiveHold\",\"path\":\"x\",\"delete_after\":5}\n{\"ts\":2,").unwrap();
    assert_eq!(sync(&f.db, &LocalFs, &roots, "machine-b").unwrap(), 1);
    let mut d = std::fs::read(&p).unwrap();
    d.extend_from_slice(b"\"machine\":\"machine-a\",\"type\":\"ArchiveHold\",\"path\":\"y\",\"delete_after\":6}\n");
    std::fs::write(&p, d).unwrap();
    assert_eq!(sync(&f.db, &LocalFs, &roots, "machine-b").unwrap(), 1);
}

#[test]
fn lease_blocks_then_expires_then_releases() {
    let f = Fixture::new();
    let root = f.root().to_path_buf();
    let target = root.join("Nintendo - 3DS/x.cia");
    let a = Lease::acquire(fs(), &root, &target, "a", Duration::from_secs(60)).unwrap();
    assert!(Lease::acquire(fs(), &root, &target, "b", Duration::from_secs(60)).is_err());
    drop(a);
    let b = Lease::acquire(fs(), &root, &target, "b", Duration::from_millis(1)).unwrap();
    std::mem::forget(b); // simulate a crashed machine leaving a stale lease
    std::thread::sleep(Duration::from_millis(1100));
    let c = Lease::acquire(fs(), &root, &target, "a", Duration::from_secs(60)).unwrap();
    drop(c);
    assert_eq!(std::fs::read_dir(root.join(".index-library/locks")).unwrap().count(), 0);
}

#[test]
fn manifest_roundtrip() {
    let f = Fixture::new();
    let dir = f.root().join("Nintendo - 3DS/_archive");
    let mut m = Manifest::load(&LocalFs, &dir).unwrap();
    assert!(m.entries.is_empty());
    m.entries.push(ManifestEntry { file: "a.cia".into(), original_path: "Nintendo - 3DS/a.cia".into(), archived_at: 10, delete_after: 20, reason: "decrypted".into(), action: Some("n3ds.decrypt_cia".into()), size: 3, machine: "a".into() });
    m.save(&LocalFs, &dir).unwrap();
    assert_eq!(Manifest::load(&LocalFs, &dir).unwrap(), m);
}
