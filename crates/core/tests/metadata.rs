mod common;
use common::*;
use lo_core::metadata::{http::RateLimiter, igdb, offline, screenscraper, sgdb, similarity, ArtKind};
use std::time::{Duration, Instant};

fn fx(name: &str) -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap() }

#[test]
fn title_similarity() {
    assert!(similarity("Pokemon X", "Pokémon X") >= 0.99);
    assert!(similarity("The Legend of Zelda - Ocarina of Time 3D", "The Legend of Zelda: Ocarina of Time 3D") >= 0.95);
    assert!(similarity("Addams Family, The", "The Addams Family") >= 0.9);
    assert!(similarity("Pokemon X", "Pokemon Y") < 0.9);
    assert!(similarity("Mario Kart 7", "Metroid Prime") < 0.5);
}

/// Real names from a No-Intro library that a lookup used to send to Match review.
#[test]
fn titles_that_are_the_same_game() {
    for (file, listed) in [
        ("Mario and Luigi - Partners in Time", "Mario & Luigi: Partners in Time"),
        ("Q-bert", "Q*bert"), ("M.A.S.H", "M*A*S*H"), ("Krusty s Fun House", "Krusty's Fun House"),
        ("Power League 4", "Power League IV"), ("Yu Yu Hakusho - Spirit Detective", "Yū Yū Hakusho: Spirit Detective"),
    ] { assert_eq!(similarity(file, listed), 1.0, "{file} / {listed}"); }
    for (file, listed) in [("Megaman III", "Mega Man 3"), ("Out Run", "OutRun"), ("DEcapAttack", "Decap Attack")] {
        assert!(similarity(file, listed) >= 0.95, "{file} / {listed}: {}", similarity(file, listed));
    }
    assert!(similarity("Lilo & Stitch", "Disney's Lilo & Stitch") >= 0.95);
    for listed in ["FIFA World Cup Germany 2006", "FIFA World Cup: Germany 2006"] {
        let s = similarity("2006 FIFA World Cup - Germany 2006", listed);
        assert!((0.9..0.95).contains(&s), "{listed}: same words, another order: {s}");
    }
    assert!(similarity("2006 FIFA World Cup - Germany 2006", "2002 FIFA World Cup") < 0.9);
    assert!(similarity("Beauty and the Beast - Belle's Quest", "Disney's Beauty and the Beast: Belle's Quest") >= 0.95);
    for (file, listed) in [("Golvellius", "Golvellius: Valley of Doom"), ("My French Coach - Learn a New Language", "My French Coach"), ("Combat ~ Tank-Plus", "Combat")] {
        assert!((0.9..0.95).contains(&similarity(file, listed)), "{file} / {listed}: a part of the name, confirmed by console only");
    }
}

#[test]
fn compilations_compare_game_by_game() {
    let file = "2 Games in 1 - Scooby-Doo + Scooby-Doo 2 - Monsters Unleashed";
    let pack = "2 Games in 1 Double Pack: Scooby-Doo / Scooby-Doo 2: Monsters Unleashed";
    let half = "Scooby-Doo 2: Monsters Unleashed";
    assert!(similarity(file, pack) >= 0.97, "{}", similarity(file, pack));
    assert!(similarity(file, "2 Games in 1 Double Pack I Scooby-Doo + Scooby-Doo 2: Monsters Unleashed") >= 0.9);
    assert!(similarity(file, half) <= 0.6, "one of the two games is not the pack");
    // searching again with a shorter name ranks the pack first too
    let query = "Scooby-Doo + Scooby-Doo 2 - Monsters Unleashed";
    assert!(similarity(query, pack) > similarity(query, half) + 0.3);
    // a different pair in the same kind of pack
    assert!(similarity(file, "2 Games in 1 Double Pack: Scooby-Doo and the Cyber Chase / Scooby-Doo! Mystery Mayhem") < 0.8);
    assert!(similarity("Crash & Spyro Superpack - Spyro - Season of Ice + Crash Bandicoot - The Huge Adventure",
        "Crash & Spyro Superpack: Crash Bandicoot: The Huge Adventure / Spyro: Season of Ice") >= 0.97, "in either order");
    // a pack's wording in front is set aside, for searching and for comparing
    use lo_core::metadata::unpack;
    assert_eq!(unpack("2 Games in 1 - Dragon Ball Z - The Legacy of Goku I & II"), "Dragon Ball Z - The Legacy of Goku I & II");
    assert_eq!(unpack("2 Games in 1! - Dragon Ball Z - Buu's Fury + Dragon Ball GT - Transformation"), "Dragon Ball Z - Buu's Fury + Dragon Ball GT - Transformation");
    assert_eq!(unpack("2-in-1 Fun Pack - Shrek 2 + Madagascar"), "Shrek 2 + Madagascar");
    assert_eq!(unpack("3 Game Pack! - Mouse Trap + Simon + Operation"), "Mouse Trap + Simon + Operation");
    assert_eq!(unpack("2 Great Games! - Pac-Man World + Ms. Pac-Man - Maze Madness"), "Pac-Man World + Ms. Pac-Man - Maze Madness");
    assert!(similarity("2 Great Games! - Pac-Man World + Ms. Pac-Man - Maze Madness", "2 Great Games! Ms. Pac-Man: Maze Madness / Pac-Man World") >= 0.97, "in the other order");
    assert!(similarity("2 Great Games! - Pac-Man World + Ms. Pac-Man - Maze Madness", "Ms. Pac-Man: Maze Madness + Pac-Man World") >= 0.97);
    // the listing as SteamGridDB writes it: "&" joins the games in a pack
    let pacman = "2 Great Games! - Pac-Man World + Ms. Pac-Man - Maze Madness";
    assert!(similarity(pacman, "2 Great Games! Ms. Pac-Man Maze Madness & Pac-Man World") >= 0.97, "{}", similarity(pacman, "2 Great Games! Ms. Pac-Man Maze Madness & Pac-Man World"));
    assert!(similarity(pacman, "Pac-Man World 2") < 0.6);
    assert!(similarity("2 Games in 1 - Dragon Ball Z - The Legacy of Goku I & II", "2 Games in 1: Dragon Ball Z - The Legacy of Goku I & II") >= 0.97);
    assert_eq!(similarity("Mario & Luigi - Partners in Time", "Mario & Luigi: Partners in Time"), 1.0);
    assert_eq!(unpack("Mario & Luigi - Partners in Time"), "Mario & Luigi - Partners in Time");
    assert_eq!(unpack("Game Boy Advance Video - Shrek 2"), "Game Boy Advance Video - Shrek 2");
    assert!(similarity("2 Games in 1 - Dragon Ball Z - The Legacy of Goku I & II", "Dragon Ball Z: The Legacy of Goku I & II") >= 0.97);
    assert!(similarity("2-Pack - Pinkalicious - Its Party Time! and Silverlicious - Sweet Adventure", "Pinkalicious") < 0.9, "one game of a pack");
    // a slash without spaces is part of one title
    assert_eq!(similarity("Battletoads-Double Dragon", "Battletoads/Double Dragon"), 1.0);
}

#[test]
fn a_label_in_front_or_a_subtitle_after() {
    use lo_core::metadata::search_names;
    let mlb = "2K Sports - Major League Baseball 2K7";
    for listed in ["Major League Baseball 2K7", "2K Sports Major League Baseball 2K7"] {
        assert!(similarity(mlb, listed) >= 0.9, "{listed}: {}", similarity(mlb, listed));
    }
    assert!(similarity(mlb, "Major League Baseball 2K8") < 0.9, "another year of the series");
    assert!(similarity(mlb, "NBA 2K7") < 0.9);
    assert!(similarity("Paws and Claws - Best Friends - Dogs and Cats", "Best Friends: Dogs & Cats") >= 0.9, "a run of sections");
    // searched again without the label, then the first section alone
    assert_eq!(search_names(mlb), ["2K Sports - Major League Baseball 2K7", "Major League Baseball 2K7", "2K Sports", "2K Sports - Major League Baseball", "Sports - Major League Baseball 2K7"]);
    assert_eq!(search_names("Tetris"), ["Tetris"]);
    assert_eq!(search_names("Super Huey UH-IX"), ["Super Huey UH-IX", "Super Huey", "Huey UH-IX"]);
    assert!(search_names("AWS Pro Moves Soccer").contains(&"Pro Moves Soccer".to_string()));
    let s = similarity("AWS Pro Moves Soccer", "Pro Moves Soccer");
    assert!((0.9..0.95).contains(&s), "an acronym label in front: {s}");
    assert!(similarity("Super Mario Bros.", "Mario Bros.") < 0.9, "a word in front is not a label");
    assert!(similarity("NBA Jam", "Jam") < 0.9, "too little left");
    assert!(similarity("Super Huey UH-IX", "Super Huey UH-1X") >= 0.95, "I for 1 in a code");
    assert!(similarity("Mega Man II", "Mega Man 11") < 0.9, "a numeral is not a code");
}

#[test]
fn titles_that_are_different_games() {
    for (file, listed) in [
        ("Sonic The Hedgehog", "Sonic the Hedgehog 3"), ("Alex Kidd in Miracle World", "Alex Kidd in Miracle World 2"),
        ("International Karate", "International Karate 2000"), ("Tinker Bell", "Tinker Bell: 2 Disney Games"),
        ("Bases Loaded II", "Bases Loaded 3"), ("Pokemon - Red Version", "Pokemon - Blue Version"),
        ("NBA 2K7", "NBA 2K8"),
    ] { assert!(similarity(file, listed) < 0.9, "{file} / {listed}: {}", similarity(file, listed)); }
}

#[test]
fn deciding_a_match() {
    use lo_core::metadata::{decide, MatchResult, MetaCandidate};
    let c = |title: &str, source: &str, score: f32, console: Option<&str>| MetaCandidate { title: title.into(), source: source.into(), source_id: title.into(), score, console: console.map(Into::into), ..Default::default() };
    let auto = |v: Vec<MetaCandidate>| match decide(v) { MatchResult::Auto(c) => Some(c.title), _ => None };
    // two listings of the same game agree; they aren't rivals
    assert_eq!(auto(vec![c("Airforce Delta", "igdb", 1.0, Some("match")), c("Airforce Delta", "igdb", 1.0, Some("match")), c("AirForce Delta", "steamgriddb", 0.98, None)]).as_deref(), Some("Airforce Delta"));
    // two different games named alike: you choose
    assert_eq!(auto(vec![c("Spot: The Video Game", "igdb", 0.92, Some("match")), c("Spot: The Cool Adventure", "igdb", 0.92, Some("match"))]), None);
    // a part-of-the-name match needs the console confirmed
    assert_eq!(auto(vec![c("Dragon Ball Z", "steamgriddb", 0.9, None)]), None);
    assert_eq!(auto(vec![c("Golvellius: Valley of Doom", "igdb", 0.92, Some("match"))]).as_deref(), Some("Golvellius: Valley of Doom"));
    // an unchecked listing doesn't outrank one confirmed for the console
    assert_eq!(auto(vec![c("Xevious: The Avenger", "igdb", 0.92, Some("match")), c("Classic NES Series: Xevious", "steamgriddb", 0.9, None)]).as_deref(), Some("Xevious: The Avenger"));
    // another console's game is never the match
    assert_eq!(auto(vec![c("Mulan", "steamgriddb", 0.98, Some("other"))]), None);
}

#[test]
fn rate_limiter_spaces_requests() {
    let rl = RateLimiter::new(Duration::from_millis(1000));
    let t0 = Instant::now();
    assert_eq!(rl.reserve(t0), Duration::ZERO);
    let d = rl.reserve(t0 + Duration::from_millis(200));
    assert_eq!(d, Duration::from_millis(800));
    assert_eq!(rl.reserve(t0 + Duration::from_millis(5000)), Duration::ZERO);
}

#[test]
fn parses_sgdb() {
    let c = sgdb::parse_search(&fx("sgdb_search.json"), "Pokemon X");
    assert_eq!(c[0].source_id, "5247");
    assert!(c[0].score > c[1].score);
    let art = sgdb::parse_art(&fx("sgdb_grids.json"), ArtKind::Cover);
    assert_eq!(art[0].url, "https://cdn2.steamgriddb.com/grid/abc.png");
}

#[test]
fn parses_igdb() {
    let c = igdb::parse_games(&fx("igdb_games.json"), lo_core::systems::by_id("n3ds").unwrap(), "Pokemon X");
    let g = &c[0];
    assert_eq!(g.title, "Pokémon X");
    assert_eq!(g.developer.as_deref(), Some("Game Freak"));
    assert_eq!(g.genres, vec!["Role-playing (RPG)"]);
    assert_eq!(g.release.as_deref(), Some("2013-10-11"));
    assert_eq!(g.art[0].url, "https://images.igdb.com/igdb/image/upload/t_cover_big/co1abc.jpg");
    assert!(g.score > 0.99);
}

#[test]
fn parses_screenscraper() {
    let c = screenscraper::parse_jeu(&fx("ss_jeu.json")).unwrap();
    assert_eq!(c.title, "Pokémon X");
    assert_eq!(c.description.as_deref(), Some("Explore Kalos."));
    assert_eq!(c.score, 1.0, "hash matches are exact");
    assert!(c.art.iter().any(|a| a.kind == ArtKind::Cover && a.url == "https://ss/box.png"));
    assert!(c.art.iter().any(|a| a.kind == ArtKind::Screenshot));
}

#[test]
fn offline_imports() {
    let f = Fixture::new();
    let n = offline::psp_tsv_import(&f.db, std::path::Path::new(&format!("{}/tests/fixtures/psp.tsv", env!("CARGO_MANIFEST_DIR")))).unwrap();
    assert_eq!(n, 1, "rows without a content id are skipped");
    assert_eq!(offline::lookup(&f.db, "psp", "UP0555-NPUF30007_00-BOMBERMAN940EH01").unwrap().unwrap().title, "Bomberman '94 (PC Engine)");
    let n = offline::switch_titledb_import(&f.db, std::path::Path::new(&format!("{}/tests/fixtures/switch_titledb.json", env!("CARGO_MANIFEST_DIR")))).unwrap();
    assert_eq!(n, 1);
    let c = offline::lookup(&f.db, "switch", "0100D9D00EE8C000").unwrap().unwrap();
    assert_eq!(c.description.as_deref(), Some("An alchemy RPG."));
    assert!(c.art.iter().any(|a| a.kind == ArtKind::Icon));
}

#[test]
fn switch_titledb_keeps_the_game_when_listings_share_an_id() {
    // the eShop lists bundles and later editions under the first game's id
    let f = Fixture::new();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("US.en.json");
    std::fs::write(&p, serde_json::json!({
        "70050000001000": {"id": "0100D9D00EE8C000", "name": "Atelier Dusk Trilogy (bundle)", "nsuId": 70050000001000u64, "description": "A 3-game set."},
        "70010000021854": {"id": "0100D9D00EE8C000", "name": "Atelier Ayesha: The Alchemist of Dusk DX", "nsuId": 70010000021854u64, "description": "Ayesha's story."},
        "70010000027019": {"id": "0100D9D00EE8C000", "name": "Atelier Dusk Trilogy Deluxe Pack", "nsuId": 70010000027019u64, "description": "DX versions of these titles..."},
        "70010000099999": {"id": "0100E65002BB8000", "name": "Stardew Valley: Nintendo Switch 2 Edition", "nsuId": 70010000099999u64},
        "70010000001234": {"id": "0100E65002BB8000", "name": "Stardew Valley", "nsuId": 70010000001234u64},
    }).to_string()).unwrap();
    assert_eq!(offline::switch_titledb_import(&f.db, &p).unwrap(), 2);
    let c = offline::lookup(&f.db, "switch", "0100D9D00EE8C000").unwrap().unwrap();
    assert_eq!((c.title.as_str(), c.description.as_deref()), ("Atelier Ayesha: The Alchemist of Dusk DX", Some("Ayesha's story.")));
    assert_eq!(offline::lookup(&f.db, "switch", "0100E65002BB8000").unwrap().unwrap().title, "Stardew Valley");
}

#[test]
fn screenscraper_query_uses_file_name_when_unhashed() {
    let sys = lo_core::systems::by_id("nds").unwrap();
    let q = lo_core::metadata::GameQuery { system: sys, title: "Mario Kart DS".into(), release: None, sha1: None, crc32: None, size: Some(10), file_name: Some("Mario Kart DS (USA).zip".into()), probe_id: None, title_id: None };
    let p = screenscraper::params(&q);
    assert!(p.contains("romnom=Mario%20Kart%20DS%20%28USA%29.zip"), "{p}");
    assert!(p.contains("systemeid=15"));
    assert!(!p.contains("sha1="));
}

#[test]
fn rawg_results_carry_their_consoles() {
    let v = serde_json::json!({"results": [
        {"id": 1, "name": "New Super Mario Bros. 2", "released": "2012-08-19", "background_image": "https://x/hero.jpg",
         "platforms": [{"platform": {"id": 8, "name": "Nintendo 3DS"}}], "genres": [{"name": "Platformer"}]},
        {"id": 2, "name": "Super Mario Bros. 2", "platforms": [{"platform": {"name": "NES"}}, {"platform": {"name": "Game Boy Advance"}}]},
        {"id": 3, "name": "Unknown Thing"}
    ]});
    let n3ds = lo_core::systems::by_id("n3ds").unwrap();
    let c = lo_core::metadata::rawg::parse_search(&v, n3ds, "New Super Mario Bros. 2");
    assert_eq!(c[0].console.as_deref(), Some("match"));
    assert_eq!(c[0].art[0].kind, lo_core::metadata::ArtKind::Hero);
    assert_eq!((c[1].console.as_deref(), c[1].platforms.clone()), (Some("other"), vec!["NES".to_string(), "Game Boy Advance".to_string()]));
    assert_eq!(c[2].console, None, "no consoles listed: not known");
}

#[test]
fn a_listing_is_also_known_by_its_other_names() {
    use lo_core::metadata::{rescore, score_for, MetaCandidate};
    let darius = MetaCandidate { source: "igdb".into(), source_id: "1".into(), title: "Darius II".into(), aliases: vec!["Sagaia".into()], console: Some("match".into()), ..Default::default() };
    assert_eq!(score_for(&["Sagaia"], &darius), 1.0, "Sagaia is Darius II's European name");
    let mut v = vec![darius.clone()];
    rescore(lo_core::systems::by_id("genesis").unwrap(), &["Sagaia"], &mut v);
    assert!(matches!(lo_core::metadata::decide(v), lo_core::metadata::MatchResult::Auto(_)));
    // old listings stored without other names still read
    let old: MetaCandidate = serde_json::from_str(r#"{"source":"igdb","source_id":"1","title":"Darius II","score":0.1,"description":null,"release":null,"genres":[],"developer":null,"publisher":null,"art":[]}"#).unwrap();
    assert!(old.aliases.is_empty());
}

#[test]
fn a_console_s_catalog_finds_games_by_any_of_their_names() {
    use lo_core::metadata::{catalog, igdb::CatalogEntry};
    let db = lo_core::db::Db::open_in_memory().unwrap();
    let e = |id: i64, name: &str, aliases: &[&str]| CatalogEntry { id, name: name.into(), aliases: aliases.iter().map(|s| s.to_string()).collect(), year: None };
    catalog::store(&db, "pce", "igdb", &[e(1, "Psychosis", &["Paranoia"]), e(2, "World Class Baseball", &["Power League"]), e(3, "Power League II", &[]), e(4, "Dragon's Curse", &["Adventure Island"])]).unwrap();
    let top = |names: &[&str]| catalog::best(&db, "pce", names, 3, 0.6).unwrap();
    assert_eq!(top(&["Paranoia"])[0], ("1".to_string(), 1.0));
    let pl = top(&["Power League"]);
    assert_eq!(pl[0], ("2".to_string(), 1.0), "the Japanese name of World Class Baseball, not its sequel: {pl:?}");
    assert_eq!(top(&["Adventure Island"])[0].0, "4");
    assert!(top(&["Bomberman"]).is_empty(), "nothing shares a word");
    // a newer catalog replaces the old one, and is what's matched
    catalog::store(&db, "pce", "igdb", &[e(9, "Bomberman", &[])]).unwrap();
    assert!(top(&["Paranoia"]).is_empty());
    assert_eq!(top(&["Bomberman"])[0].0, "9");
}
