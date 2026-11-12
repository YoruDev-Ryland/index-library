//! Regression tests for the final-review findings (C1, I1–I11).
mod common;
use common::*;
use lo_core::db::Db;
use lo_core::fsx::LocalFs;
use lo_core::ops::{apply, undo_batch, Op};
use lo_core::rename::{plan_renames, to_ops};
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::{sync, Journal};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn rescan(f: &Fixture) -> lo_core::scan::ScanReport { scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap() }
fn j(f: &Fixture, m: &str) -> Journal { Journal::new(Arc::new(LocalFs), f.settings.library_roots.clone(), m.into()) }
fn games(f: &Fixture) -> Vec<i64> { let mut q = f.db.conn().prepare("SELECT id FROM games").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() }

// C1: removing an "empty" folder must never delete contents
#[test]
fn c1_empty_dir_removal_refuses_non_empty_folder() {
    let f = Fixture::new();
    let dir = f.root().join("Nintendo - NES/empty");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("arrived-later.zip"), b"game").unwrap();
    let r = apply(&f.db, &LocalFs, &j(&f, "m"), vec![Op::remove_empty_dir(dir.clone())], "t").unwrap();
    assert_eq!(r.failed.len(), 1);
    assert!(dir.join("arrived-later.zip").exists());
    // and a delete op never removes a directory recursively
    let r = apply(&f.db, &LocalFs, &j(&f, "m"), vec![Op::delete(dir.clone())], "t").unwrap();
    assert_eq!(r.failed.len(), 1);
    assert!(dir.join("arrived-later.zip").exists());
}

// I1: an unmounted (empty) root must not wipe the index
#[test]
fn i1_empty_root_does_not_wipe_index() {
    let f = Fixture::new();
    f.put("Nintendo - NES/a (U).zip", b"a");
    rescan(&f);
    let gid: i64 = f.count("SELECT game_id FROM files");
    f.db.conn().execute("INSERT INTO status VALUES(?1,'decrypted','true',0)", [gid]).unwrap();
    std::fs::rename(f.root().join("Nintendo - NES"), f.dir.path().parent().unwrap().join(format!("moved-{}", gid))).ok();
    for e in std::fs::read_dir(f.root()).unwrap() { let p = e.unwrap().path(); if p.is_dir() { std::fs::remove_dir_all(p).unwrap(); } }
    let r = rescan(&f);
    assert_eq!(r.errors.len(), 1, "{r:?}");
    assert_eq!(f.count("SELECT count(*) FROM files"), 1);
    assert_eq!(f.count("SELECT count(*) FROM status"), 1);
}

// I2: a cue is not rewritten when its track rename was refused
#[test]
fn i2_cue_untouched_when_bin_rename_refused() {
    let f = Fixture::new();
    f.put("Sony - PS1/Game (U) (Disc 1).bin", b"bin");
    let cue = f.put("Sony - PS1/Game (U) (Disc 1).cue", b"FILE \"Game (U) (Disc 1).bin\" BINARY\n");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &games(&f)).unwrap();
    let ops = to_ops(&f.db, &LocalFs, &plan).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(f.root().join("Sony - PS1/Game (U) (Disc 1).bin"), b"bin-changed").unwrap();
    apply(&f.db, &LocalFs, &j(&f, "m"), ops, "rename").unwrap();
    let new_cue = cue.with_file_name("Game (USA) (Disc 1).cue");
    let text = std::fs::read_to_string(if new_cue.exists() { new_cue } else { cue }).unwrap();
    assert!(text.contains("FILE \"Game (U) (Disc 1).bin\""), "{text}");
}

// I3: case-only rename never overwrites a distinct file (case-sensitive FS)
#[cfg(target_os = "linux")]
#[test]
fn i3_case_only_rename_does_not_clobber() {
    let f = Fixture::new();
    let a = f.put("Nintendo - NES/foo.zip", b"lower");
    let b = f.put("Nintendo - NES/Foo.zip", b"upper");
    let r = apply(&f.db, &LocalFs, &j(&f, "m"), vec![Op::rename(a.clone(), b.clone())], "t").unwrap();
    assert_eq!(r.failed.len(), 1);
    assert_eq!(std::fs::read(&a).unwrap(), b"lower");
    assert_eq!(std::fs::read(&b).unwrap(), b"upper");
}

// I4: an archived original whose replacement never arrived is pinned (never expires)
#[test]
fn i4_pinned_archive_never_expires() {
    let f = Fixture::new();
    let p = f.put("Nintendo - 3DS/Only Copy.3ds", b"x");
    rescan(&f);
    let jr = j(&f, "m");
    let e = lo_core::archive::archive_file(&f.db, &LocalFs, &jr, &p, "decrypting", None, 7).unwrap();
    lo_core::archive::pin(&f.db, &LocalFs, e.id, "restore failed — this is the only copy").unwrap();
    assert!(lo_core::archive::expired(&f.db, i64::MAX - 1).unwrap().is_empty());
    let other = Db::open_in_memory().unwrap();
    lo_core::archive::rebuild_from_manifests(&other, &LocalFs, &f.settings.library_roots).unwrap();
    assert!(lo_core::archive::expired(&other, i64::MAX - 1).unwrap().is_empty(), "pin is in the manifest too");
}

// I5: purging expired items re-reads the manifest (another PC may have extended the hold)
#[test]
fn i5_purge_expired_respects_hold_extended_elsewhere() {
    let f = Fixture::new();
    let p = f.put("Nintendo - 3DS/G.cia", b"x");
    rescan(&f);
    let jr = j(&f, "m");
    let e = lo_core::archive::archive_file(&f.db, &LocalFs, &jr, &p, "x", None, 0).unwrap();
    // another PC extends the hold by editing the shared manifest
    let dir = e.path.parent().unwrap().to_path_buf();
    let mut m = lo_core::shared::manifest::Manifest::load(&LocalFs, &dir).unwrap();
    m.entries[0].delete_after = lo_core::db::now() + 86_400 * 30;
    m.save(&LocalFs, &dir).unwrap();
    let r = lo_core::archive::purge_expired(&f.db, &LocalFs, &jr, &[e.id]).unwrap();
    assert_eq!(r.deleted, 0);
    assert!(e.path.exists());
}

// I6 + I11: another PC's archive op removes the file from the index; its renames can be undone here
#[test]
fn i6_i11_journal_ops_from_other_pc() {
    let f = Fixture::new();
    let a = f.put("Nintendo - NES/a (U).zip", b"a");
    let b = f.put("Nintendo - NES/b (U).zip", b"b");
    rescan(&f);
    let other = Db::open_in_memory().unwrap();
    scan(&other, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let ja = j(&f, "pc-a");
    let r = apply(&f.db, &LocalFs, &ja, vec![Op::rename(a.clone(), a.with_file_name("a (USA).zip"))], "Rename 1 files").unwrap();
    lo_core::archive::archive_file(&f.db, &LocalFs, &ja, &b, "dup", None, 7).unwrap();
    sync(&other, &LocalFs, &f.settings.library_roots, "pc-b").unwrap();
    let n: i64 = other.conn().query_row("SELECT count(*) FROM files WHERE path LIKE '%_archive%'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "archived file must not appear as a library file");
    let hist = lo_core::ops::history(&other, 50).unwrap();
    assert!(hist.iter().any(|h| h.batch_id == r.batch_id), "{hist:?}");
    let jb = j(&f, "pc-b");
    let u = undo_batch(&other, &LocalFs, &jb, &r.batch_id).unwrap();
    assert!(u.failed.is_empty(), "{u:?}");
    assert!(a.exists());
}

// I7: zero-byte files and cue/bin tracks are not "identical file" duplicates
#[test]
fn i7_same_hash_skips_empty_files_and_tracks() {
    let f = Fixture::new();
    f.put("Sony - PS1/A (USA)/A (Track 2).bin", b"same-audio");
    f.put("Sony - PS1/A (USA)/A.cue", b"FILE \"A (Track 2).bin\" BINARY\n");
    f.put("Sony - PS1/A (Europe)/A (Track 2).bin", b"same-audio");
    f.put("Sony - PS1/A (Europe)/A.cue", b"FILE \"A (Track 2).bin\" BINARY\n");
    f.put("Nintendo - NES/x.nes", b"");
    f.put("Nintendo - NES/y.nes", b"");
    rescan(&f);
    let ids: Vec<i64> = { let mut q = f.db.conn().prepare("SELECT id FROM files").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    lo_core::hash::hash_files(&f.db, &LocalFs, &ids, &lo_core::hash::Throttle::new(lo_core::hash::HashLevel::Maximum), &|_| {}, &AtomicBool::new(false)).unwrap();
    let d = lo_core::dupes::find_dupes(&f.db, None).unwrap();
    assert!(!d.iter().any(|g| g.reason == lo_core::dupes::DupeReason::SameHash), "{d:?}");
}

// I8: DAT renames use each ROM's own name for raw files in multi-file sets
#[test]
fn i8_dat_rename_uses_rom_names() {
    let f = Fixture::new();
    f.put("Sony - PS1/g1.bin", b"track1");
    f.put("Sony - PS1/g2.bin", b"track2");
    let crc = |d: &[u8]| format!("{:08x}", crc32fast::hash(d));
    let dat = format!(r#"<datafile><header><name>PS1</name></header><game name="Game (USA)">
      <rom name="Game (USA) (Track 1).bin" size="6" crc="{}"/><rom name="Game (USA) (Track 2).bin" size="6" crc="{}"/></game></datafile>"#, crc(b"track1"), crc(b"track2"));
    let datp = f.put("ps1.dat", dat.as_bytes());
    rescan(&f);
    let ids: Vec<i64> = { let mut q = f.db.conn().prepare("SELECT id FROM files").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    lo_core::hash::hash_files(&f.db, &LocalFs, &ids, &lo_core::hash::Throttle::new(lo_core::hash::HashLevel::Maximum), &|_| {}, &AtomicBool::new(false)).unwrap();
    lo_core::dat::import_dat(&f.db, "ps1", &datp).unwrap();
    lo_core::dat::verify(&f.db, "ps1").unwrap();
    let plan = plan_renames(&f.db, &LocalFs, &games(&f)).unwrap();
    let mut dst: Vec<String> = plan.iter().filter(|i| i.problem.is_none()).map(|i| i.dst.file_name().unwrap().to_string_lossy().into_owned()).collect();
    dst.sort();
    assert_eq!(dst, vec!["Game (USA) (Track 1).bin", "Game (USA) (Track 2).bin"], "{plan:?}");
}

// I9: .m3u playlists follow renamed discs
#[test]
fn i9_m3u_follows_renamed_cues() {
    let f = Fixture::new();
    f.put("Sony - PS1/Game (U) (Disc 1).cue", b"FILE \"x.bin\" BINARY\n");
    f.put("Sony - PS1/Game (U) (Disc 2).cue", b"FILE \"y.bin\" BINARY\n");
    let m3u = f.put("Sony - PS1/Game (U).m3u", b"Game (U) (Disc 1).cue\nGame (U) (Disc 2).cue\n");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &games(&f)).unwrap();
    let ops = to_ops(&f.db, &LocalFs, &plan).unwrap();
    apply(&f.db, &LocalFs, &j(&f, "m"), ops, "rename").unwrap();
    let final_m3u = if m3u.exists() { m3u } else { m3u.with_file_name("Game (USA).m3u") };
    let text = std::fs::read_to_string(final_m3u).unwrap();
    assert!(text.contains("Game (USA) (Disc 1).cue") && text.contains("Game (USA) (Disc 2).cue"), "{text}");
}
