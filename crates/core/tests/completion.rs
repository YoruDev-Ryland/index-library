//! How complete a console's collection is: every game in its No-Intro / Redump list once
//! (regional releases together), against what's here.
use lo_core::completion::{self, CloneList, Options, State};
use lo_core::jobs::JobEvent;
use lo_core::library::{HashScope, Library};
use sha1::Digest;
use std::sync::Arc;
use std::time::Duration;

fn sha1_hex(b: &[u8]) -> String { sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

/// Retool's clone list form: titles that are one game under different names.
const CLONES: &str = r#"{"description":{"name":"Nintendo - Super Nintendo Entertainment System (No-Intro)"},
  "variants":[{"group":"Super Mario World 2 - Yoshi's Island","titles":[{"searchTerm":"Super Mario - Yoshi Island"},{"searchTerm":"Super Mario World 2 - Yoshi's Island"}]}]}"#;

fn setup() -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let body = |n: u8| -> Vec<u8> { (0..4096u32).map(|i| (i as u8).wrapping_mul(n)).collect() };
    // the list: (release name, content)
    let releases: Vec<(&str, Vec<u8>)> = vec![
        ("Super Mario World (USA)", body(3)), ("Super Mario World (Japan)", body(5)),
        ("Super Mario - Yoshi Island (Japan)", body(7)), ("Super Mario World 2 - Yoshi's Island (USA)", body(9)),
        ("F-Zero (USA)", body(11)), ("F-Zero (Europe)", body(13)),
        ("Pilotwings (Japan)", body(15)),
        ("Star Fox (USA) (Beta)", body(17)),
        ("Star Fox 2 (World) (Prototype)", body(23)), ("Stunt Race FX (USA) (Beta 2)", body(25)),
        ("Cheetahmen (USA) (Unl)", body(19)),
        ("[BIOS] Super Famicom Box (Japan)", body(21)),
    ];
    let mut dat = String::from("clrmamepro ( name \"Nintendo - Super Nintendo Entertainment System\" version \"1\" )\n");
    for (n, b) in &releases {
        dat.push_str(&format!("game ( name \"{n}\" rom ( name \"{n}.sfc\" size {} crc {:08x} sha1 {} ) )\n", b.len(), crc32fast::hash(b), sha1_hex(b)));
    }
    let d = root.path().join("Nintendo - SNES");
    std::fs::create_dir_all(&d).unwrap();
    // here: Super Mario World (verified), Yoshi's Island as its Japanese release (verified),
    // and an F-Zero whose dump isn't in the list (here, but not verified)
    std::fs::write(d.join("Super Mario World (USA).sfc"), &releases[0].1).unwrap();
    std::fs::write(d.join("Super Mario - Yoshi Island (Japan).sfc"), &releases[2].1).unwrap();
    std::fs::write(d.join("F-Zero (USA).sfc"), body(99)).unwrap();
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let p = home.path().join("snes.dat");
    std::fs::write(&p, dat).unwrap();
    let db = l.db().unwrap();
    lo_core::dat::import_dat(&db, "snes", &p).unwrap();
    lo_core::dat::verify(&db, "snes").unwrap();
    (l, home, root)
}

fn usa_first() -> Vec<String> { ["USA", "World", "Europe", "Japan"].iter().map(|s| s.to_string()).collect() }

#[test]
fn every_game_once_with_what_is_here() {
    let (l, _h, _r) = setup();
    let db = l.db().unwrap();
    let clones = CloneList::parse(CLONES).unwrap();
    let c = completion::completion(&db, "snes", &usa_first(), Some(&clones), &Options::default()).unwrap();
    let rows: Vec<(String, State, String)> = c.entries.iter().map(|e| (e.title.clone(), e.state, e.release.clone())).collect();
    assert_eq!(rows, vec![
        ("F-Zero".into(), State::Unverified, "F-Zero (USA)".into()),
        ("Pilotwings".into(), State::Missing, "Pilotwings (Japan)".into()),
        ("Super Mario World".into(), State::Have, "Super Mario World (USA)".into()),
        // one game under two names; the release shown is the preferred region's
        ("Super Mario World 2 - Yoshi's Island".into(), State::Have, "Super Mario World 2 - Yoshi's Island (USA)".into()),
    ], "betas, BIOS and unlicensed games aren't counted by default");
    assert_eq!((c.total, c.have, c.unverified, c.missing), (4, 2, 1, 1));
    let smw = c.entries.iter().find(|e| e.title == "Super Mario World").unwrap();
    assert_eq!(smw.releases.len(), 2);
    assert!(smw.game_id.is_some());
}

#[test]
fn only_the_regions_you_collect() {
    let (l, _h, _r) = setup();
    let db = l.db().unwrap();
    let opts = Options { regions: vec!["USA".into()], ..Default::default() };
    let c = completion::completion(&db, "snes", &usa_first(), None, &opts).unwrap();
    // Pilotwings came out in Japan only; without the clone list the two Yoshi's Islands are two
    // games, and only the USA one counts
    assert_eq!(c.total, 3);
    assert!(c.entries.iter().all(|e| e.title != "Pilotwings"));
    assert!(c.entries.iter().all(|e| e.regions.iter().any(|r| r == "USA")));
}

#[test]
fn unlicensed_games_when_asked() {
    let (l, _h, _r) = setup();
    let db = l.db().unwrap();
    let opts = Options { unlicensed: true, ..Default::default() };
    let c = completion::completion(&db, "snes", &usa_first(), Some(&CloneList::parse(CLONES).unwrap()), &opts).unwrap();
    assert_eq!(c.total, 5);
    assert!(c.entries.iter().any(|e| e.title == "Cheetahmen" && e.state == State::Missing));
}

#[test]
fn clone_lists_are_named_by_retool_s_files() {
    assert_eq!(completion::clonelist_file("snes"), Some("Nintendo - Super Nintendo Entertainment System (No-Intro).json"));
    assert_eq!(completion::clonelist_file("atari2600"), Some("Atari - Atari 2600 (No-Intro).json"));
    assert_eq!(completion::clonelist_file("ps1"), Some("Sony - PlayStation (Redump).json"));
    assert_eq!(completion::clonelist_file("switch"), None);
}

#[test]
fn titles_match_whichever_end_the_article_is_on() {
    use lo_core::completion::title_key;
    assert_eq!(title_key("The 7th Saga"), title_key("7th Saga, The"));
    assert_eq!(title_key("The Legend of Zelda - A Link Between Worlds"), title_key("Legend of Zelda, The - A Link Between Worlds"));
    assert_eq!(title_key("A Boy and His Blob"), title_key("Boy and His Blob, A"));
    assert_ne!(title_key("Super Mario World"), title_key("Super Mario World 2"));
}

#[test]
fn the_library_measures_each_console_against_its_list() {
    let (l, _h, _r) = setup();
    // the clone list as if downloaded already
    let p = l.cache_dir.join("clonelists").join(completion::clonelist_file("snes").unwrap());
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, CLONES).unwrap();
    let c = l.completion("snes", &Options::default()).unwrap();
    assert_eq!((c.total, c.have), (4, 2));
    let all = l.completion_summary(&Options::default()).unwrap();
    let snes = all.iter().find(|s| s.system == "snes").expect("every console with a list");
    assert_eq!((snes.total, snes.have, snes.unverified), (4, 2, 1));
    assert_eq!(snes.name, lo_core::systems::by_id("snes").unwrap().name);
}

#[test]
fn english_releases_only_when_asked() {
    let (l, _h, _r) = setup();
    let db = l.db().unwrap();
    let opts = Options { languages: vec!["En".into()], ..Default::default() };
    let c = completion::completion(&db, "snes", &usa_first(), Some(&CloneList::parse(CLONES).unwrap()), &opts).unwrap();
    let titles: Vec<&str> = c.entries.iter().map(|e| e.title.as_str()).collect();
    // Pilotwings is Japanese only; Yoshi's Island counts by its English release
    assert_eq!(titles, ["F-Zero", "Super Mario World", "Super Mario World 2 - Yoshi's Island"]);
    let yoshi = c.entries.iter().find(|e| e.title.starts_with("Super Mario World 2")).unwrap();
    assert_eq!(yoshi.state, State::Have, "had through its Japanese release");
}

#[test]
fn vita_is_measured_by_title_id_against_the_digital_list() {
    let db = lo_core::db::Db::open_in_memory().unwrap();
    let c = db.conn();
    for (id, region, name) in [("PCSE00001", "US", "Gravity Rush"), ("PCSB00002", "EU", "Gravity Rush"), ("PCSG00003", "JP", "Gravity Daze"), ("PCSE00009", "US", "Tearaway")] {
        c.execute("INSERT INTO titledb(source, id, name, region) VALUES('psv',?1,?2,?3)", (id, name, region)).unwrap();
    }
    c.execute("INSERT INTO games(system_id, group_key, title) VALUES('psvita','tid:PCSB00002','Gravity Rush')", []).unwrap();
    let g = c.last_insert_rowid();
    c.execute("INSERT INTO files(path, dir_id, system_id, name, ext, size, mtime, game_id) VALUES('/x/gr.pkg', 1, 'psvita', 'gr.pkg', 'pkg', 1, 1, ?1)", [g]).unwrap();
    let opts = Options { languages: vec!["En".into()], ..Default::default() };
    let r = completion::completion(&db, "psvita", &usa_first(), None, &opts).unwrap();
    let rows: Vec<(&str, State)> = r.entries.iter().map(|e| (e.title.as_str(), e.state)).collect();
    assert_eq!(rows, [("Gravity Rush", State::Have), ("Tearaway", State::Missing)], "had by its European title id; the Japanese title isn't English");
    assert_eq!(r.entries[0].release, "Gravity Rush (USA)");
    assert!(r.list.as_deref().unwrap_or("").contains("NoPayStation"));
}

#[test]
fn physical_completion_counts_copies() {
    let (l, _h, _r) = setup();
    let db = l.db().unwrap();
    l.shelf_add("snes", "Pilotwings (Japan)", None, lo_core::shelf::CopyFields { cart: true, ..Default::default() }).unwrap();
    let clones = CloneList::parse(CLONES).unwrap();
    let state = |src: &str, title: &str| {
        let c = completion::completion(&db, "snes", &usa_first(), Some(&clones), &Options { source: src.into(), ..Default::default() }).unwrap();
        c.entries.iter().find(|e| e.title == title).map(|e| (e.state, e.shelf)).unwrap()
    };
    assert_eq!(state("physical", "Pilotwings"), (State::Have, 1));
    assert_eq!(state("physical", "Super Mario World"), (State::Missing, 0), "files don't count on the shelf");
    assert_eq!(state("physical", "F-Zero").0, State::Missing, "no 'not verified' for copies");
    assert_eq!(state("either", "Pilotwings").0, State::Have);
    assert_eq!(state("either", "Super Mario World").0, State::Have);
    assert_eq!(state("digital", "Pilotwings"), (State::Missing, 1), "the shelf count shows whatever is measured");
}

#[test]
fn a_vita_game_only_on_the_shelf_isn_t_a_file() {
    let db = lo_core::db::Db::open_in_memory().unwrap();
    db.conn().execute("INSERT INTO titledb(source, id, name, region) VALUES('psv','PCSE00001','Gravity Rush','US')", []).unwrap();
    db.conn().execute("INSERT INTO games(system_id, group_key, title) VALUES('psvita','tid:PCSE00001','Gravity Rush')", []).unwrap();
    let r = completion::completion(&db, "psvita", &usa_first(), None, &Options::default()).unwrap();
    assert_eq!(r.entries[0].state, State::Missing, "no files: not had digitally");
}
