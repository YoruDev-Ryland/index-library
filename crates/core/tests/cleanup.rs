mod common;
use common::*;
use lo_core::dupes::{find_dupes, DupeReason};
use lo_core::fsx::LocalFs;
use lo_core::junk::{find_junk, JunkKind};
use lo_core::probe::testdata_ncsd;
use lo_core::scan::{scan, ScanOpts};
use std::sync::atomic::AtomicBool;

fn fixture() -> Fixture {
    let f = Fixture::new();
    f.put("Nintendo - 3DS/Bravely Default.3ds", &testdata_ncsd(5, false));
    f.put("Nintendo - 3DS/Bravely Default-decrypted.3ds", &testdata_ncsd(5, true));
    f.put("Nintendo - 3DS/3dscia.com.url", b"[InternetShortcut]");
    // game-sized, so it's a stray game even though the folder holds a script
    let cia = f.put("Nintendo - 3DS/3DS-Decrypt/Pokemon - Alpha Sapphire (USA).cia", b"cia");
    std::fs::OpenOptions::new().write(true).open(&cia).unwrap().set_len(20 << 20).unwrap();
    f.put("Nintendo - 3DS/3DS-Decrypt/decrypt_cia.py", b"py");
    f.put("Hacked Pokemon Roms/Pokemon - Gaia (FireRed Hack).gba", b"gaia-rom");
    f.put("Hacked Pokemon Roms/Pokemon - Gaia (FireRed Hack).zip", b"gaia-zip");
    f.put("Nintendo - NES/Zelda (USA).zip", b"z");
    f.put("Nintendo - NES/Zelda (Japan).zip", b"zj");
    f.put("Nintendo - NES/leftover.zip.lo-part", b"");
    f.put("Nintendo - NES/stray disc.iso", b"r");
    f.put("Nintendo - Gamecube/MoH (USA) (Disc 1).iso", b"d1");
    f.put("Nintendo - Gamecube/MoH (USA) (Disc 2).iso", b"d2");
    std::fs::create_dir_all(f.root().join("Nintendo - NES/empty")).unwrap();
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, lo_core::systems::by_id("n3ds").unwrap(), &AtomicBool::new(false)).unwrap();
    f
}
fn name_of(f: &Fixture, id: i64) -> String { f.db.conn().query_row("SELECT name FROM files WHERE id=?1", [id], |r| r.get(0)).unwrap() }

#[test]
fn enc_dec_twins_keep_decrypted() {
    let f = fixture();
    let d = find_dupes(&f.db, Some("n3ds")).unwrap();
    let g = d.iter().find(|g| g.reason == DupeReason::EncDec).expect("EncDec group");
    assert_eq!(g.files.len(), 2);
    assert_eq!(name_of(&f, g.keep), "Bravely Default-decrypted.3ds");
}

#[test]
fn same_title_diff_format_and_region() {
    let f = fixture();
    let d = find_dupes(&f.db, None).unwrap();
    assert!(d.iter().any(|g| g.reason == DupeReason::SameTitleDiffFormat && g.system_id == "gbahacks"), "{d:?}");
    let r = d.iter().find(|g| g.reason == DupeReason::SameTitleDiffRegion).expect("region group");
    assert_eq!(name_of(&f, r.keep), "Zelda (USA).zip");
}

#[test]
fn multi_disc_is_not_a_duplicate() {
    let f = fixture();
    assert!(find_dupes(&f.db, Some("gamecube")).unwrap().is_empty());
}

#[test]
fn junk_found() {
    let f = fixture();
    let j = find_junk(&f.db, &LocalFs, &f.settings).unwrap();
    let has = |k: JunkKind, n: &str| j.iter().any(|x| x.kind == k && x.path.to_string_lossy().ends_with(n));
    assert!(has(JunkKind::UrlShortcut, "3dscia.com.url"), "{j:?}");
    assert!(has(JunkKind::RomInToolFolder, "Pokemon - Alpha Sapphire (USA).cia"), "{j:?}");
    assert!(has(JunkKind::TempLeftover, "leftover.zip.lo-part"), "{j:?}");
    assert!(has(JunkKind::EmptyDir, "empty"), "{j:?}");
    assert!(has(JunkKind::ForeignSystem, "stray disc.iso"), "{j:?}");
    assert!(!j.iter().any(|x| x.path.to_string_lossy().ends_with("decrypt_cia.py")), "tool files themselves are not junk");
}

#[test]
fn stats_totals() {
    let f = fixture();
    let s = lo_core::stats::stats(&f.db, &f.settings).unwrap();
    let n3ds = s.systems.iter().find(|x| x.id == "n3ds").unwrap();
    assert_eq!((n3ds.games, n3ds.files), (1, 2));
    assert_eq!(n3ds.status_counts.get("decrypted=false"), Some(&1), "one encrypted copy left => game not fully decrypted");
    assert_eq!(s.systems.iter().map(|x| x.files).sum::<u64>(), f.count("SELECT count(*) FROM files") as u64);
    assert!(s.dupe_bytes > 0);
}

#[test]
fn base_and_update_in_separate_folders_are_not_dupes() {
    let f = Fixture::new();
    f.put("Sony - PS Vita/Games/Escape Plan.pkg", b"base");
    f.put("Sony - PS Vita/Updates/Escape Plan.pkg", b"update");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(find_dupes(&f.db, Some("psvita")).unwrap().is_empty());
}

#[test]
fn archived_psp_and_switch_games_are_games_not_junk() {
    let f = Fixture::new();
    f.put("Sony - PSP/EBoots/40 Winks (E) PSN eboot.zip", b"z");
    f.put("Sony - PSP/Games/Crimson Gem Saga.zip", b"z");
    f.put("Nintendo - Switch/Fire Emblem Engage [0100A6301214E000][v0][US].nsp.part1.rar", b"r");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    assert_eq!(f.count("SELECT count(*) FROM files"), 3);
    let j = find_junk(&f.db, &LocalFs, &f.settings).unwrap();
    assert!(j.is_empty(), "{j:?}");
}

#[test]
fn both_decrypted_by_header_is_not_an_enc_dec_pair() {
    // a "-decrypted" suffix next to a plain name, but the headers say both are decrypted
    let f = Fixture::new();
    f.put("Nintendo - 3DS/Pokemon - Alpha Sapphire (USA)-decrypted.3ds", &testdata_ncsd(9, true));
    f.put("Nintendo - 3DS/Pokemon - Alpha Sapphire (USA).3ds", &testdata_ncsd(9, true));
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, lo_core::systems::by_id("n3ds").unwrap(), &AtomicBool::new(false)).unwrap();
    let d = find_dupes(&f.db, Some("n3ds")).unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_ne!(d[0].reason, DupeReason::EncDec, "{d:?}");
    assert_eq!(name_of(&f, d[0].keep), "Pokemon - Alpha Sapphire (USA).3ds", "equally decrypted: keep the clean name");
}

#[test]
fn tidy_name_kept_and_bios_files_are_separate_games() {
    let f = Fixture::new();
    f.put("Nintendo - Gameboy Color/Croc .gbc", b"croc-a");
    f.put("Nintendo - Gameboy Color/Croc.gbc", b"croc-b");
    f.put("Sega - 32X/[BIOS] 32X M68000 (USA).zip", b"m");
    f.put("Sega - 32X/[BIOS] 32X SH-2 Master (USA).zip", b"ma");
    f.put("Sega - 32X/[BIOS] 32X SH-2 Slave (USA).zip", b"sl");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let d = find_dupes(&f.db, None).unwrap();
    assert!(d.iter().all(|g| g.system_id != "sega32x"), "{d:?}");
    let g = d.iter().find(|g| g.system_id == "gbc").expect("croc group");
    assert_eq!(name_of(&f, g.keep), "Croc.gbc");
    let extra = g.files.iter().position(|f| *f != g.keep).unwrap();
    assert_eq!(g.proofs[extra], lo_core::dupes::Proof::Unchecked, "not fingerprinted yet");
    f.db.conn().execute("UPDATE files SET sha1=name", []).unwrap();
    let d = find_dupes(&f.db, Some("gbc")).unwrap();
    let extra = d[0].files.iter().position(|f| *f != d[0].keep).unwrap();
    assert_eq!(d[0].proofs[extra], lo_core::dupes::Proof::Different);
    assert_eq!(d[0].proven_bytes, 0, "different dumps are never counted as safe to archive");
}

#[test]
fn one_game_one_rom_keeps_the_whole_best_release() {
    let f = Fixture::new();
    f.put("Nintendo - Gamecube/MoH (USA) (Disc 1).iso", b"u1");
    f.put("Nintendo - Gamecube/MoH (USA) (Disc 2).iso", b"u2");
    f.put("Nintendo - Gamecube/MoH (Europe) (Disc 1).iso", b"e1");
    f.put("Nintendo - Gamecube/MoH (Europe) (Disc 2).iso", b"e2");
    f.put("Nintendo - Gamecube/MoH (Japan).iso", b"j");
    f.put("Nintendo - Gamecube/Only In Japan (Japan).iso", b"oj");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let d = find_dupes(&f.db, None).unwrap();
    assert_eq!(d.len(), 1, "{d:?}");
    let g = &d[0];
    assert_eq!(g.reason, DupeReason::SameTitleDiffRegion);
    let kept: Vec<String> = std::iter::once(g.keep).chain(g.keep_also.iter().copied()).map(|id| name_of(&f, id)).collect();
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept.iter().all(|n| n.contains("(USA)")), "{kept:?}");
    assert_eq!(g.files.len(), 5);
    // all regions kept when 1G1R is off
    let s = lo_core::config::Settings::default();
    assert!(lo_core::dupes::find_dupes_with(&f.db, None, &s.region_order, false).unwrap().is_empty());
}

#[test]
fn keeps_the_copy_that_runs_as_it_is() {
    let f = Fixture::new();
    // PSP: a compressed iso runs and is smallest; an archive has to be unpacked first
    f.put("Sony - PSP/Patapon (USA).rar", b"rar");
    f.put("Sony - PSP/Patapon (USA).iso", b"iso-iso-iso");
    f.put("Sony - PSP/Patapon (USA).cso", b"cso");
    // NES: emulators open a zip directly, and it's smaller than the raw ROM
    f.put("Nintendo - NES/Zelda (USA).7z", b"7");
    f.put("Nintendo - NES/Zelda (USA).nes", b"rom");
    f.put("Nintendo - NES/Zelda (USA).zip", b"z");
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let d = find_dupes(&f.db, None).unwrap();
    let keep = |sys: &str| name_of(&f, d.iter().find(|g| g.system_id == sys).unwrap_or_else(|| panic!("{sys}: {d:?}")).keep);
    assert_eq!(keep("psp"), "Patapon (USA).cso");
    assert_eq!(keep("nes"), "Zelda (USA).zip");
}

fn zip_of(name: &str, data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
    w.write_all(data).unwrap();
    w.finish().unwrap().into_inner()
}

#[test]
fn unpack_and_compare_gives_a_definite_answer() {
    let f = Fixture::new();
    f.put("Sony - PSP/Same Game (USA).iso", b"the-same-iso");
    f.put("Sony - PSP/Same Game (USA).zip", &zip_of("Same Game (USA).iso", b"the-same-iso"));
    f.put("Sony - PSP/Other Game (USA).iso", b"one-dump");
    f.put("Sony - PSP/Other Game (USA).zip", &zip_of("Other Game (USA).iso", b"another-dump"));
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    use lo_core::dupes::Proof;
    let proof = |title: &str| { let d = find_dupes(&f.db, Some("psp")).unwrap(); let g = d.into_iter().find(|g| g.title == title).unwrap(); g.files.iter().zip(&g.proofs).find(|(id, _)| name_of(&f, **id).ends_with(".zip")).map(|(_, p)| *p).unwrap() };
    assert_eq!(proof("Same Game"), Proof::Unchecked);

    // what "Unpack and compare" does: fingerprint plain files, unpack and fingerprint archives
    let ids: Vec<i64> = { let mut q = f.db.conn().prepare("SELECT id FROM files").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    let plain: Vec<i64> = ids.iter().copied().filter(|id| !name_of(&f, *id).ends_with(".zip")).collect();
    lo_core::hash::hash_files(&f.db, &LocalFs, &plain, &lo_core::hash::Throttle::new(lo_core::hash::HashLevel::Maximum), &|_| {}, &AtomicBool::new(false)).unwrap();
    for (id, path, _) in lo_core::deep::needs_unpack(&f.db, &ids).unwrap() {
        let d = lo_core::deep::unpack_digest(&LocalFs, &f.settings, &path, &AtomicBool::new(false), &Default::default()).unwrap();
        lo_core::deep::save(&f.db, id, &d).unwrap();
    }
    assert!(lo_core::deep::needs_unpack(&f.db, &ids).unwrap().is_empty(), "nothing left to unpack");
    assert_eq!(proof("Same Game"), Proof::Identical);
    assert_eq!(proof("Other Game"), Proof::Different);
}
