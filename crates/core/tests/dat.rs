mod common;
use common::*;
use lo_core::dat::{import_dat, missing, verify};
use lo_core::fsx::LocalFs;
use lo_core::hash::{hash_files, HashLevel, Throttle};
use lo_core::scan::{scan, ScanOpts};
use std::sync::atomic::AtomicBool;

const DAT: &str = r#"<?xml version="1.0"?>
<!DOCTYPE datafile PUBLIC "-//Logiqx//DTD ROM Management Datafile//EN" "http://www.logiqx.com/dtds/datafile.dtd">
<datafile>
  <header><name>Nintendo - Nintendo Entertainment System</name><version>20240101</version></header>
  <game name="Hello Quest (USA)"><description>Hello Quest (USA)</description>
    <rom name="Hello Quest (USA).nes" size="5" crc="3610A686" sha1="AAF4C61DDCC5E8A2DABEDE0F3B482CD9AEA9434D" md5="5d41402abc4b2a76b9719d911017c592"/></game>
  <game name="Bad Dump Game (USA)"><rom name="Bad Dump Game (USA).nes" size="3" crc="00000000" sha1="0000000000000000000000000000000000000000"/></game>
  <game name="Missing Game (Japan)"><rom name="Missing Game (Japan).nes" size="3" crc="11111111" sha1="1111111111111111111111111111111111111111"/></game>
</datafile>"#;

#[test]
fn verify_marks_verified_bad_unknown_and_missing() {
    let f = Fixture::new();
    f.put("Nintendo - NES/hello (U).nes", b"hello");
    f.put("Nintendo - NES/Bad Dump Game (USA).nes", b"xyz");
    f.put("Nintendo - NES/Homebrew.nes", b"hb");
    let dat = f.put("nes.dat", DAT.as_bytes());
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let ids: Vec<i64> = { let mut q = f.db.conn().prepare("SELECT id FROM files").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    hash_files(&f.db, &LocalFs, &ids, &Throttle::new(HashLevel::Maximum), &|_| {}, &AtomicBool::new(false)).unwrap();
    let s = import_dat(&f.db, "nes", &dat).unwrap();
    assert_eq!(s.games, 3);
    assert_eq!(s.name, "Nintendo - Nintendo Entertainment System");
    let r = verify(&f.db, "nes").unwrap();
    assert_eq!((r.verified, r.bad, r.unknown), (1, 1, 1), "{r:?}");
    let st = |n: &str| -> (String, Option<String>) { f.db.conn().query_row("SELECT dat_status, dat_game FROM files WHERE name=?1", [n], |r| Ok((r.get(0)?, r.get(1)?))).unwrap() };
    assert_eq!(st("hello (U).nes"), ("verified".into(), Some("Hello Quest (USA)".into())));
    assert_eq!(st("Bad Dump Game (USA).nes").0, "bad");
    assert_eq!(st("Homebrew.nes").0, "unknown");
    let canon: Option<String> = f.db.conn().query_row("SELECT g.canonical_id FROM games g JOIN files f ON f.game_id=g.id WHERE f.name='hello (U).nes'", [], |r| r.get(0)).unwrap();
    assert_eq!(canon.as_deref(), Some("Hello Quest (USA)"));
    assert_eq!(missing(&f.db, "nes").unwrap(), vec!["Bad Dump Game (USA)".to_string(), "Missing Game (Japan)".to_string()]);
}

#[test]
fn reimport_replaces_previous_dat() {
    let f = Fixture::new();
    let dat = f.put("nes.dat", DAT.as_bytes());
    import_dat(&f.db, "nes", &dat).unwrap();
    import_dat(&f.db, "nes", &dat).unwrap();
    assert_eq!(f.count("SELECT count(*) FROM dat_roms"), 3);
}
