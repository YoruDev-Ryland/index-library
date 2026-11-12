mod common;
use common::*;
use lo_core::fsx::LocalFs;
use lo_core::scan::{scan, ScanOpts};
use std::sync::atomic::AtomicBool;

fn base() -> Fixture {
    let f = Fixture::new();
    f.put("Nintendo - NES/a (U).zip", b"nes");
    f.put("Nintendo - 3DS/x.cia", b"cia");
    f.put("Nintendo - 3DS/readme.txt", b"txt");
    f.put("Nintendo - 3DS/3DS-Decrypt/stray.cia", b"cia");
    f.put("Emulator Software/e.apk", b"apk");
    f.put("Sony - PS1/Crash/Crash.cue", b"cue");
    f.put("Sony - PS1/Crash/Crash.bin", b"bin");
    f
}
fn run(f: &Fixture, full: bool) -> lo_core::scan::ScanReport {
    scan(&f.db, &LocalFs, &f.settings, ScanOpts { full }, &|_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn first_scan_indexes() {
    let f = base();
    let r = run(&f, false);
    assert_eq!(r.files_added, 4, "{r:?}");
    assert_eq!(f.count("SELECT count(*) FROM files WHERE name LIKE '%.apk'"), 0);
    assert_eq!(f.count("SELECT count(*) FROM files WHERE name = 'stray.cia'"), 0);
    assert_eq!(f.count("SELECT count(*) FROM files WHERE system_id='ps1'"), 2);
    assert!(f.count("SELECT count(*) FROM games") >= 1);
    assert_eq!(f.count("SELECT count(*) FROM files WHERE game_id IS NULL"), 0);
}

#[test]
fn rescan_skips_unchanged() {
    let f = base();
    run(&f, false);
    let r = run(&f, false);
    assert_eq!(r.dirs_listed, 1, "{r:?}");
    assert!(r.dirs_skipped >= 3, "{r:?}");
    assert_eq!(r.files_added + r.files_removed + r.files_changed, 0);
    assert_eq!(f.count("SELECT count(*) FROM files"), 4);
}

#[test]
fn rescan_detects_add_remove() {
    let f = base();
    run(&f, false);
    std::thread::sleep(std::time::Duration::from_millis(20));
    f.put("Nintendo - NES/b (U).zip", b"nes2");
    std::fs::remove_file(f.root().join("Nintendo - 3DS/x.cia")).unwrap();
    let r = run(&f, false);
    assert_eq!(r.files_added, 1, "{r:?}");
    assert_eq!(r.files_removed, 1, "{r:?}");
    assert_eq!(f.count("SELECT count(*) FROM games WHERE id NOT IN (SELECT game_id FROM files)"), 0);
}

#[test]
fn nested_change_found_when_parent_unchanged() {
    let f = base();
    run(&f, false);
    std::thread::sleep(std::time::Duration::from_millis(20));
    f.put("Sony - PS1/Crash/Crash (Track 2).bin", b"b2");
    let r = run(&f, false);
    assert_eq!(r.files_added, 1, "{r:?}");
}

#[test]
fn full_rescan_detects_in_place_change() {
    let f = base();
    run(&f, false);
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(f.root().join("Nintendo - NES/a (U).zip"), b"changed content").unwrap();
    let r = run(&f, true);
    assert_eq!(r.files_changed, 1, "{r:?}");
}

#[test]
fn unreadable_dir_is_reported() {
    let f = base();
    run(&f, false);
    std::thread::sleep(std::time::Duration::from_millis(20));
    f.put("Nintendo - 3DS/y.cia", b"y");
    let fs = FailingFs { fail_dir_name: "Nintendo - 3DS".into() };
    let r = scan(&f.db, &fs, &f.settings, ScanOpts { full: false }, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.errors.len(), 1, "{r:?}");
    // existing 3DS file must not be dropped because of the error
    assert_eq!(f.count("SELECT count(*) FROM files WHERE system_id='n3ds'"), 1);
    assert_eq!(f.count("SELECT count(*) FROM files WHERE system_id='nes'"), 1);
}
