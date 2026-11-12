mod common;
use common::*;
use lo_core::fsx::LocalFs;
use lo_core::scan::{scan, ScanOpts};
use std::sync::atomic::AtomicBool;

fn run(f: &Fixture) {
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
}
fn games_of(f: &Fixture, sys: &str) -> Vec<(String, i64)> {
    let mut q = f.db.conn().prepare("SELECT g.title, count(f.id) FROM games g JOIN files f ON f.game_id=g.id WHERE g.system_id=?1 GROUP BY g.id ORDER BY g.title").unwrap();
    q.query_map([sys], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
}

#[test]
fn enc_dec_twins_are_one_game() {
    let f = Fixture::new();
    f.put("Nintendo - 3DS/Bravely Default.3ds", b"a");
    f.put("Nintendo - 3DS/Bravely Default-decrypted.3ds", b"b");
    f.put("Nintendo - 3DS/Mario Kart 7 (USA)-decrypted.cia", b"c");
    run(&f);
    assert_eq!(games_of(&f, "n3ds"), vec![("Bravely Default".into(), 2), ("Mario Kart 7".into(), 1)]);
}

#[test]
fn multi_disc_and_cue_bin_folders() {
    let f = Fixture::new();
    f.put("Sony - PS1/Game (USA) (Disc 1).cue", b"");
    f.put("Sony - PS1/Game (USA) (Disc 1).bin", b"");
    f.put("Sony - PS1/Game (USA) (Disc 2).cue", b"");
    f.put("Sony - PS1/Game (USA) (Disc 2).bin", b"");
    f.put("Sony - PS1/Crash Bandicoot/Crash.cue", b"");
    f.put("Sony - PS1/Crash Bandicoot/Crash (Track 1).bin", b"");
    run(&f);
    assert_eq!(games_of(&f, "ps1"), vec![("Crash Bandicoot".into(), 2), ("Game".into(), 4)]);
}

#[test]
fn proto_and_retail_stay_separate() {
    let f = Fixture::new();
    f.put("Atari - Lynx/Alien vs Predator (USA) (Proto).zip", b"");
    f.put("Atari - Lynx/Alien vs Predator (USA).zip", b"");
    run(&f);
    assert_eq!(games_of(&f, "lynx").len(), 2);
}

#[test]
fn switch_base_update_dlc_grouped() {
    let f = Fixture::new();
    f.put("Nintendo - Switch/Atelier [0100D9D00EE8C000]/Atelier [0100D9D00EE8C000][v0].nsp", b"");
    f.put("Nintendo - Switch/Atelier [0100D9D00EE8C000]/Atelier [0100D9D00EE8C800][v65536].nsp", b"");
    f.put("Nintendo - Switch/Atelier [0100D9D00EE8C000]/Atelier DLC [0100D9D00EE8D001][v0].nsp", b"");
    run(&f);
    assert_eq!(games_of(&f, "switch"), vec![("Atelier".into(), 3)]);
}

#[test]
fn game_ids_stable_across_rescans_and_status_survives() {
    let f = Fixture::new();
    f.put("Nintendo - NES/a (U).zip", b"");
    run(&f);
    let id: i64 = f.count("SELECT game_id FROM files");
    f.db.conn().execute("INSERT INTO status VALUES(?1,'x','1',0)", [id]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    f.put("Nintendo - NES/b (U).zip", b"");
    run(&f);
    assert_eq!(f.count("SELECT game_id FROM files WHERE name='a (U).zip'"), id);
    assert_eq!(f.count("SELECT count(*) FROM status"), 1);
}

#[test]
fn probes_set_decrypted_status() {
    let f = Fixture::new();
    f.put("Nintendo - 3DS/Enc (USA).3ds", &lo_core::probe::testdata_ncsd(1, false));
    f.put("Nintendo - 3DS/Dec (USA).3ds", &lo_core::probe::testdata_ncsd(2, true));
    run(&f);
    let s = lo_core::systems::by_id("n3ds").unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, s, &AtomicBool::new(false)).unwrap();
    let v = |t: &str| -> String { f.db.conn().query_row("SELECT s.value FROM status s JOIN games g ON g.id=s.game_id WHERE g.title=?1 AND s.key='decrypted'", [t], |r| r.get(0)).unwrap() };
    assert_eq!(v("Enc"), "false");
    assert_eq!(v("Dec"), "true");
    // second run probes nothing new
    assert_eq!(lo_core::probe::run_probes(&f.db, &LocalFs, s, &AtomicBool::new(false)).unwrap(), 0);
}

fn pkg(content_id: &str) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    b[0..4].copy_from_slice(b"\x7FPKG");
    b[0x30..0x30 + content_id.len()].copy_from_slice(content_id.as_bytes());
    b
}

#[test]
fn vita_dlc_and_updates_group_under_their_game() {
    let f = Fixture::new();
    f.put("Sony - PS Vita/Games/10 Second Ninja X.pkg", &pkg("UP4395-PCSE00890_00-10SECNINJAVITAUS"));
    f.put("Sony - PS Vita/Updates/10 Second Ninja X.pkg", &pkg("UP4395-PCSE00890_00-10SECNINJAVITAUS"));
    f.put("Sony - PS Vita/DLC/Bonus Costume Pack.pkg", &pkg("UP4395-PCSE00890_00-NINJADLC00000001"));
    f.put("Sony - PS Vita/DLC/2000AD Costumes.pkg", &pkg("UP9000-PCSA00017_00-LBPDLC2KADCK0001"));
    run(&f);
    let s = lo_core::systems::by_id("psvita").unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, s, &AtomicBool::new(false)).unwrap();
    lo_core::grouping::assign_games(&f.db, s).unwrap();
    // the DLC whose game isn't in the library stays on its own
    assert_eq!(games_of(&f, "psvita"), vec![("10 Second Ninja X".into(), 3), ("2000AD Costumes".into(), 1)]);
}
