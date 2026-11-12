//! Physical copies on the shelf: kept per copy, in the one library, never lost to a scan.
use lo_core::jobs::JobEvent;
use lo_core::library::Library;
use lo_core::shelf::{self, CopyFields};
use std::sync::Arc;
use std::time::Duration;

/// A library with these SNES files (name → content), scanned.
fn library(files: &[(&str, &[u8])]) -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let (home, root) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let d = root.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&d).unwrap();
    for (n, b) in files { std::fs::write(d.join(n), b).unwrap(); }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    scan(&l);
    (l, home, root)
}
fn scan(l: &Arc<Library>) { l.scan(false); assert!(l.jobs.wait_idle(Duration::from_secs(30))); }
fn cib() -> CopyFields { CopyFields { cart: true, box_: true, manual: true, ..Default::default() } }
fn titles(l: &Library) -> Vec<String> {
    let db = l.db().unwrap();
    let mut q = db.conn().prepare("SELECT title FROM games ORDER BY title").unwrap();
    q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
}

#[test]
fn copies_record_their_parts() {
    let label = |f: CopyFields| shelf::parts_label(&shelf::Copy { cart: f.cart, box_: f.box_, manual: f.manual, sealed: f.sealed, ..Default::default() });
    assert_eq!(label(cib()), "CIB");
    assert_eq!(label(CopyFields { sealed: true, ..Default::default() }), "Sealed");
    assert_eq!(label(CopyFields { cart: true, ..Default::default() }), "Loose");
    assert_eq!(label(CopyFields { cart: true, box_: true, ..Default::default() }), "Boxed");
    assert_eq!(label(CopyFields { box_: true, ..Default::default() }), "Box only");
    assert_eq!(label(CopyFields { manual: true, ..Default::default() }), "Manual only");
}

#[test]
fn a_copy_of_a_game_not_here_makes_a_shelf_only_game() {
    let (l, _h, _r) = library(&[("F-Zero (USA).sfc", b"fz")]);
    let db = l.db().unwrap();
    let c = shelf::add(&db, "snes", "EarthBound (USA)", None, cib()).unwrap();
    assert_eq!(titles(&l), ["EarthBound", "F-Zero"]);
    let gid: i64 = db.conn().query_row("SELECT id FROM games WHERE title='EarthBound'", [], |r| r.get(0)).unwrap();
    let copies = shelf::copies_of(&db, gid).unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].id, c.id);
    assert_eq!(copies[0].release, "EarthBound (USA)");
}

#[test]
fn shelf_only_games_survive_scans_and_regrouping() {
    let (l, _h, root) = library(&[("F-Zero (USA).sfc", b"fz")]);
    let db = l.db().unwrap();
    shelf::add(&db, "snes", "EarthBound (USA)", None, cib()).unwrap();
    scan(&l);
    lo_core::grouping::assign_games(&db, lo_core::systems::by_id("snes").unwrap()).unwrap();
    assert_eq!(titles(&l), ["EarthBound", "F-Zero"]);
    // the NAS unmounted: the files go, the shelf stays
    std::fs::remove_dir_all(root.path().join("Nintendo - SNES")).unwrap();
    scan(&l);
    assert!(titles(&l).contains(&"EarthBound".to_string()));
    let n: i64 = db.conn().query_row("SELECT COUNT(*) FROM shelf_copies WHERE deleted=0", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn two_copies_of_one_release_are_one_game() {
    let (l, _h, _r) = library(&[]);
    let db = l.db().unwrap();
    shelf::add(&db, "snes", "EarthBound (USA)", None, cib()).unwrap();
    shelf::add(&db, "snes", "EarthBound (USA)", None, CopyFields { cart: true, ..Default::default() }).unwrap();
    assert_eq!(titles(&l), ["EarthBound"]);
    let gid: i64 = db.conn().query_row("SELECT id FROM games", [], |r| r.get(0)).unwrap();
    assert_eq!(shelf::copies_of(&db, gid).unwrap().len(), 2);
}

#[test]
fn removed_copies_leave_the_game_when_it_has_nothing_else() {
    let (l, _h, _r) = library(&[]);
    let db = l.db().unwrap();
    let c = shelf::add(&db, "snes", "EarthBound (USA)", None, cib()).unwrap();
    shelf::remove(&db, &c.id).unwrap();
    scan(&l);
    assert!(titles(&l).is_empty());
    let deleted: i64 = db.conn().query_row("SELECT deleted FROM shelf_copies WHERE id=?1", [&c.id], |r| r.get(0)).unwrap();
    assert_eq!(deleted, 1, "kept as removed, so other devices learn it's gone");
}

#[test]
fn editing_a_copy_keeps_it_and_its_game() {
    let (l, _h, _r) = library(&[]);
    let db = l.db().unwrap();
    let c = shelf::add(&db, "snes", "EarthBound (USA)", None, cib()).unwrap();
    let e = shelf::update(&db, &c.id, CopyFields { cart: true, condition: Some("good".into()), location: Some("Shelf 3".into()), price_cents: Some(12000), ..Default::default() }).unwrap();
    assert_eq!((e.condition.as_deref(), e.location.as_deref(), e.price_cents, e.box_), (Some("good"), Some("Shelf 3"), Some(12000), false));
    assert!(e.id == c.id && e.release == c.release);
}

// ---- joining files and copies ----

fn add_file(root: &std::path::Path, name: &str, body: &[u8]) { std::fs::write(root.join("Nintendo - SNES").join(name), body).unwrap(); }
fn game_count(l: &Library) -> i64 { l.db().unwrap().conn().query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0)).unwrap() }

#[test]
fn a_dumped_copy_joins_its_file_game() {
    let (l, _h, root) = library(&[]);
    let db = l.db().unwrap();
    let c = shelf::add(&db, "snes", "Pilotwings (USA)", None, cib()).unwrap();
    let gid: i64 = db.conn().query_row("SELECT id FROM games", [], |r| r.get(0)).unwrap();
    l.set_play(gid, lo_core::userdata::PlayInfo { status: Some("finished".into()), ..Default::default() }).unwrap();
    let coll = l.create_collection("Flight", &[gid]).unwrap();
    // later, the cart is dumped
    add_file(root.path(), "Pilotwings (USA).sfc", b"pw");
    scan(&l);
    assert_eq!(game_count(&l), 1, "one game, not two");
    let gid: i64 = db.conn().query_row("SELECT game_id FROM files", [], |r| r.get(0)).unwrap();
    assert_eq!(shelf::copies_of(&db, gid).unwrap().iter().map(|x| x.id.clone()).collect::<Vec<_>>(), [c.id]);
    assert_eq!(lo_core::userdata::play_of(&db, gid).unwrap().unwrap().status.as_deref(), Some("finished"));
    assert_eq!(l.collection(&coll).unwrap().items[0].game_id, Some(gid));
}

#[test]
fn a_copy_joins_a_file_named_differently_by_title_and_region() {
    let (l, _h, root) = library(&[]);
    let db = l.db().unwrap();
    shelf::add(&db, "snes", "Pilotwings (USA)", None, cib()).unwrap();
    add_file(root.path(), "Pilotwings (USA) [!].sfc", b"pw");
    scan(&l);
    assert_eq!(game_count(&l), 1);
}

#[test]
fn joins_only_the_same_region() {
    let (l, _h, _r) = library(&[("Pilotwings (USA) [a].sfc", b"u"), ("Pilotwings (Japan) [a].sfc", b"j")]);
    let db = l.db().unwrap();
    shelf::add(&db, "snes", "Pilotwings (Europe)", None, cib()).unwrap();
    shelf::add(&db, "snes", "Pilotwings (Japan)", None, cib()).unwrap();
    l.shelf_join("snes").unwrap();
    let rows: Vec<(String, i64)> = db.conn().prepare("SELECT c.release, (SELECT COUNT(*) FROM files f JOIN games g ON g.id=f.game_id WHERE g.system_id || ':' || g.group_key = c.ref) FROM shelf_copies c ORDER BY c.release").unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect();
    assert_eq!(rows, [("Pilotwings (Europe)".to_string(), 0), ("Pilotwings (Japan)".to_string(), 1)]);
    assert_eq!(game_count(&l), 3);
}

#[test]
fn a_copy_joins_through_a_verified_file_whatever_its_name() {
    use sha1::Digest;
    let (l, home, root) = library(&[]);
    let db = l.db().unwrap();
    shelf::add(&db, "snes", "Pilotwings (USA)", None, cib()).unwrap();
    let body = vec![7u8; 2048];
    add_file(root.path(), "pw.sfc", &body);
    scan(&l);
    l.hash(lo_core::library::HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(30)));
    let sha: String = sha1::Sha1::digest(&body).iter().map(|x| format!("{x:02x}")).collect();
    let p = home.path().join("snes.dat");
    std::fs::write(&p, format!("clrmamepro ( name \"x\" version \"1\" )\ngame ( name \"Pilotwings (USA)\" rom ( name \"Pilotwings (USA).sfc\" size 2048 crc {:08x} sha1 {sha} ) )\n", crc32fast::hash(&body))).unwrap();
    lo_core::dat::import_dat(&db, "snes", &p).unwrap();
    l.verify("snes").unwrap();
    assert_eq!(game_count(&l), 1);
}

// ---- in the library ----

use lo_core::library::{GameFilter, GameSort};

fn page(l: &Library, f: GameFilter) -> Vec<(String, i64, Option<String>)> {
    l.games(&f, GameSort::Title, 0, 100).unwrap().rows.into_iter().map(|g| (g.title, g.shelf.copies, g.shelf.best)).collect()
}

#[test]
fn the_library_shows_what_you_have() {
    let (l, _h, _r) = library(&[("F-Zero (USA).sfc", b"fz"), ("Pilotwings (USA).sfc", b"pw")]);
    let db = l.db().unwrap();
    l.shelf_add("snes", "Pilotwings (USA)", None, CopyFields { cart: true, ..Default::default() }).unwrap();
    l.shelf_add("snes", "Pilotwings (USA)", None, cib()).unwrap();
    l.shelf_add("snes", "EarthBound (USA)", None, CopyFields { sealed: true, ..Default::default() }).unwrap();
    let all = page(&l, GameFilter::default());
    assert_eq!(all, [("EarthBound".into(), 1, Some("Sealed".into())), ("F-Zero".into(), 0, None), ("Pilotwings".into(), 2, Some("CIB".into()))]);
    let owned = |o: &str| page(&l, GameFilter { owned: Some(o.into()), ..Default::default() }).into_iter().map(|x| x.0).collect::<Vec<_>>();
    assert_eq!(owned("digital"), ["F-Zero", "Pilotwings"]);
    assert_eq!(owned("physical"), ["EarthBound", "Pilotwings"]);
    assert_eq!(owned("both"), ["Pilotwings"]);
    // the console shows up even if it only had shelf games
    assert!(l.systems().unwrap().iter().any(|s| s.id == "snes" && s.games == 3));
    let eb: i64 = db.conn().query_row("SELECT id FROM games WHERE title='EarthBound'", [], |r| r.get(0)).unwrap();
    let d = l.game_detail(eb).unwrap();
    assert_eq!(d.copies.len(), 1);
    assert!(d.files.is_empty());
}

#[test]
fn search_finds_copies_by_edition_and_location() {
    let (l, _h, _r) = library(&[]);
    l.shelf_add("snes", "EarthBound (USA)", None, CopyFields { cart: true, edition: Some("Player's Choice".into()), location: Some("Shelf 3".into()), ..Default::default() }).unwrap();
    let search = |q: &str| page(&l, GameFilter { search: Some(q.into()), ..Default::default() }).len();
    assert_eq!((search("player's"), search("shelf 3"), search("earth"), search("zelda")), (1, 1, 1, 0));
    assert_eq!(l.shelf_locations().unwrap(), ["Shelf 3"]);
}

#[test]
fn release_search_offers_the_console_s_list() {
    let (l, home, _r) = library(&[]);
    let p = home.path().join("snes.dat");
    std::fs::write(&p, "clrmamepro ( name \"x\" version \"1\" )\ngame ( name \"Pilotwings (USA)\" rom ( name \"a.sfc\" size 1 crc 00000001 ) )\ngame ( name \"Pilotwings (Japan)\" rom ( name \"b.sfc\" size 1 crc 00000002 ) )\ngame ( name \"F-Zero (USA)\" rom ( name \"c.sfc\" size 1 crc 00000003 ) )\n").unwrap();
    l.import_dat("snes", &p).unwrap();
    let hits = l.shelf_releases("snes", "pilot", 10).unwrap();
    let names: Vec<&str> = hits.iter().map(|h| h.release.as_str()).collect();
    assert_eq!(names, ["Pilotwings (USA)", "Pilotwings (Japan)"], "the preferred region first");
    assert_eq!(hits[0].source, "list");
    assert_eq!(hits[0].title, "Pilotwings");
}

#[test]
fn a_shelf_only_game_is_looked_up_by_its_copy_s_release() {
    let (l, home, _r) = library(&[]);
    let p = home.path().join("snes.dat");
    std::fs::write(&p, "clrmamepro ( name \"x\" version \"1\" )\ngame ( name \"Legend of Zelda, The - A Link to the Past (USA)\" rom ( name \"a.sfc\" size 1 crc 00000001 ) )\n").unwrap();
    l.import_dat("snes", &p).unwrap();
    let c = l.shelf_add("snes", "Legend of Zelda, The - A Link to the Past (USA)", None, cib()).unwrap();
    let db = l.db().unwrap();
    let gid: i64 = db.conn().query_row("SELECT id FROM games", [], |r| r.get(0)).unwrap();
    assert_eq!(lo_core::metadata::release_name(&db, gid).unwrap().as_deref(), Some("The Legend of Zelda - A Link to the Past"));
    assert_eq!(lo_core::metadata::boxart::release_of(&db, gid).unwrap(), Some(("snes".to_string(), c.release.clone())));
    // a typed title isn't a list release
    l.shelf_add("snes", "Some Bootleg", None, cib()).unwrap();
    let bid: i64 = db.conn().query_row("SELECT id FROM games WHERE title='Some Bootleg'", [], |r| r.get(0)).unwrap();
    assert_eq!(lo_core::metadata::boxart::release_of(&db, bid).unwrap(), None);
}

// ---- CSV import ----

use lo_core::shelf_csv;

#[test]
fn csv_reads_common_spreadsheet_exports() {
    let t = shelf_csv::parse("\u{feff}Title;Console;Price;Bought;Notes\r\n\"Super Mario World; Deluxe\";SNES;\"$1,234.50\";6/2/2024;a\r\n\r\n;;;;\r\nF-Zero;SNES;12;2024-01-05;\"said \"\"hi\"\"\"\r\n").unwrap();
    assert_eq!(t.headers, ["Title", "Console", "Price", "Bought", "Notes"]);
    assert_eq!(t.rows.len(), 2, "blank rows dropped");
    assert_eq!(t.rows[0][0], "Super Mario World; Deluxe");
    assert_eq!(t.rows[1][4], "said \"hi\"");
    assert_eq!(shelf_csv::price_cents("$1,234.50"), Some(123450));
    assert_eq!(shelf_csv::price_cents("12"), Some(1200));
    assert_eq!(shelf_csv::price_cents("1.234,50 €"), Some(123450));
    assert_eq!(shelf_csv::price_cents("free?"), None);
    assert_eq!(shelf_csv::date("6/2/2024").as_deref(), Some("2024-06-02"));
    assert_eq!(shelf_csv::date("2024-01-05").as_deref(), Some("2024-01-05"));
    assert_eq!(shelf_csv::date("31.12.2023").as_deref(), Some("2023-12-31"));
    assert_eq!(shelf_csv::date("someday"), None);
    let m = shelf_csv::guess_mapping(&t.headers);
    assert_eq!((m.title, m.console, m.price, m.date, m.notes), (Some(0), Some(1), Some(2), Some(3), Some(4)));
}

#[test]
fn csv_rows_match_releases_and_skip_repeats() {
    let (l, home, _r) = library(&[]);
    let p = home.path().join("snes.dat");
    std::fs::write(&p, "clrmamepro ( name \"x\" version \"1\" )\ngame ( name \"Pilotwings (USA)\" rom ( name \"a.sfc\" size 1 crc 00000001 ) )\ngame ( name \"Pilotwings (Japan)\" rom ( name \"b.sfc\" size 1 crc 00000002 ) )\n").unwrap();
    l.import_dat("snes", &p).unwrap();
    let csv = "Game,Platform,Region,Condition,Complete,Location\nPilotwings,Super Nintendo,Japan,Good,CIB,Shelf 2\nPilotwings,SNES,,Very good,loose,\nNo Such Game,SNES,,,,\nMystery,Atari Jaguar,,,,\n";
    let rows = l.shelf_csv_preview(csv, None).unwrap();
    let r0 = &rows[0];
    assert_eq!((r0.system.as_deref(), r0.release.as_deref(), r0.how.as_str()), (Some("snes"), Some("Pilotwings (Japan)"), "close"));
    assert!(r0.fields.cart && r0.fields.box_ && r0.fields.manual);
    assert_eq!((r0.fields.condition.as_deref(), r0.fields.location.as_deref()), (Some("good"), Some("Shelf 2")));
    assert_eq!(rows[1].release.as_deref(), Some("Pilotwings (USA)"), "no region given: the preferred one");
    assert!(rows[1].fields.cart && !rows[1].fields.box_);
    assert_eq!(rows[2].how, "none");
    assert_eq!(rows[3].system, None, "a console Index doesn't know");
    assert_eq!(l.shelf_csv_import(&rows).unwrap(), 2);
    let again = l.shelf_csv_preview(csv, None).unwrap();
    assert!(again[0].duplicate && again[1].duplicate);
    assert_eq!(l.shelf_csv_import(&again).unwrap(), 0);
}

// ---- review fixes ----

#[test]
fn joining_games_doesn_t_make_copies_look_newer() {
    let (l, _h, root) = library(&[]);
    let db = l.db().unwrap();
    let c = shelf::add(&db, "snes", "Pilotwings (USA)", None, cib()).unwrap();
    db.conn().execute("UPDATE shelf_copies SET updated_at=1000 WHERE id=?1", [&c.id]).unwrap();
    add_file(root.path(), "Pilotwings (USA) [!].sfc", b"pw");
    scan(&l);
    assert_eq!(game_count(&l), 1);
    let t: i64 = db.conn().query_row("SELECT updated_at FROM shelf_copies WHERE id=?1", [&c.id], |r| r.get(0)).unwrap();
    assert_eq!(t, 1000, "a join moves a copy; it isn't an edit");
}

#[test]
fn a_copy_from_elsewhere_never_lands_on_another_region() {
    let (l, _h, _r) = library(&[("Foo (Japan).sfc", b"j")]);
    let db = l.db().unwrap();
    let game = lo_core::userdata::GameRef { system: "snes".into(), key: "foo|usa||||".into(), title: "Foo".into(), ids: vec![] };
    shelf::apply(&db, &shelf::Copy { id: "x".into(), game, release: "Foo (USA)".into(), cart: true, ..Default::default() }, 5).unwrap();
    let jp: i64 = db.conn().query_row("SELECT id FROM games WHERE title='Foo' AND EXISTS (SELECT 1 FROM files f WHERE f.game_id=games.id)", [], |r| r.get(0)).unwrap();
    assert!(shelf::copies_of(&db, jp).unwrap().is_empty(), "not on the Japanese game");
    assert_eq!(game_count(&l), 2);
}

#[test]
fn renaming_a_file_keeps_its_copies_and_play_on_one_game() {
    let (l, _h, root) = library(&[("Foo (USA).sfc", b"f"), ("Foo (USA) (Rev 1).sfc", b"g")]);
    let db = l.db().unwrap();
    let gid: i64 = db.conn().query_row("SELECT game_id FROM files WHERE name='Foo (USA).sfc'", [], |r| r.get(0)).unwrap();
    shelf::add(&db, "snes", "Foo (USA)", None, cib()).unwrap();
    l.set_play(gid, lo_core::userdata::PlayInfo { status: Some("playing".into()), ..Default::default() }).unwrap();
    // Cleanup renames it: its group key changes
    let d = root.path().join("Nintendo - SNES");
    std::fs::rename(d.join("Foo (USA).sfc"), d.join("Foo (USA) [!].sfc")).unwrap();
    scan(&l);
    let g: i64 = db.conn().query_row("SELECT game_id FROM files WHERE name='Foo (USA) [!].sfc'", [], |r| r.get(0)).unwrap();
    assert_eq!(shelf::copies_of(&db, g).unwrap().len(), 1, "the copy followed the file");
    assert_eq!(lo_core::userdata::play_of(&db, g).unwrap().unwrap().status.as_deref(), Some("playing"));
    let empty: i64 = db.conn().query_row("SELECT COUNT(*) FROM games g WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id)", [], |r| r.get(0)).unwrap();
    assert_eq!(empty, 0, "no phantom shelf-only game left behind");
}

#[test]
fn switch_copies_take_the_title_id_their_files_group_under() {
    let (l, _h, _r) = library(&[]);
    let db = l.db().unwrap();
    db.conn().execute("INSERT INTO titledb(source, id, name, region) VALUES('switch','0100ABCD12340000','Foo Quest','US')", []).unwrap();
    let (key, _, _) = shelf::key_for(&db, "switch", "Foo Quest (USA)", None).unwrap();
    assert_eq!(key, "tid:0100ABCD1234");
}

#[test]
fn a_one_console_spreadsheet_takes_the_console_you_pick() {
    let (l, _h, _r) = library(&[]);
    let rows = l.shelf_csv_preview_for("Title\nEarthBound\n", None, Some("snes")).unwrap();
    assert_eq!(rows[0].system.as_deref(), Some("snes"));
    assert_eq!(rows[0].how, "typed");
}

#[test]
fn spreadsheet_odds_and_ends() {
    assert_eq!(shelf_csv::date("25/12/2023").as_deref(), Some("2023-12-25"), "a day over 12 is the day");
    let mut f = CopyFields::default();
    shelf_csv::parts("Box, no manual", &mut f);
    assert!(f.cart && f.box_ && !f.manual);
    assert_eq!(lo_core::util::text_of(&[b'P', 0xA3, b'5']), "P£5", "Windows-1252 from Excel");
    assert_eq!(lo_core::util::text_of("Pokémon".as_bytes()), "Pokémon");
}

#[test]
fn a_rename_by_cleanup_keeps_copies_and_play_on_one_game() {
    let (l, _h, root) = library(&[("Bar (USA).sfc", b"f"), ("Bar (USA) (Rev 1).sfc", b"g")]);
    let db = l.db().unwrap();
    let gid: i64 = db.conn().query_row("SELECT game_id FROM files WHERE name='Bar (USA).sfc'", [], |r| r.get(0)).unwrap();
    shelf::add(&db, "snes", "Bar (USA)", None, cib()).unwrap();
    l.set_play(gid, lo_core::userdata::PlayInfo { status: Some("playing".into()), ..Default::default() }).unwrap();
    // Cleanup moves the file and updates the library in place (the file keeps its game)
    let d = root.path().join("Nintendo - SNES");
    std::fs::rename(d.join("Bar (USA).sfc"), d.join("Bar (USA) [b].sfc")).unwrap();
    lo_core::ops::update_index_path(&db, &d.join("Bar (USA).sfc"), &d.join("Bar (USA) [b].sfc")).unwrap();
    lo_core::grouping::assign_games(&db, lo_core::systems::by_id("snes").unwrap()).unwrap();
    let g: i64 = db.conn().query_row("SELECT game_id FROM files WHERE name='Bar (USA) [b].sfc'", [], |r| r.get(0)).unwrap();
    assert_ne!(g, gid, "a new key: a new game");
    assert_eq!(shelf::copies_of(&db, g).unwrap().len(), 1);
    assert_eq!(lo_core::userdata::play_of(&db, g).unwrap().unwrap().status.as_deref(), Some("playing"));
}
