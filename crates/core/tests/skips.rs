//! Suggestions you've said no to stay gone until you bring them back.
mod common;
use lo_core::jobs::JobEvent;
use lo_core::library::{GameFilter, GameSort, HashScope, Library};
use lo_core::metadata::MetaCandidate;
use std::sync::Arc;
use std::time::Duration;

fn lib(files: &[(&str, &str)]) -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    for (p, body) in files {
        let full = root.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
    }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    (l, home, root)
}

fn id_of(l: &Library, title: &str) -> i64 {
    l.games(&GameFilter { search: Some(title.into()), ..Default::default() }, GameSort::Title, 0, 10).unwrap().rows[0].id
}

#[test]
fn match_review_skips_stay_until_you_look_the_game_up() {
    let (l, _h, _r) = lib(&[("Nintendo - NES/Kid Icarus (USA).nes", "a"), ("Nintendo - NES/Metroid (USA).nes", "b")]);
    let (icarus, metroid) = (id_of(&l, "Icarus"), id_of(&l, "Metroid"));
    let cand = |t: &str| MetaCandidate { source: "steamgriddb".into(), source_id: "1".into(), title: t.into(), score: 0.7, ..Default::default() };
    let db = l.db().unwrap();
    lo_core::metadata::set_review(&db, icarus, &[cand("Kid Icarus: Uprising")]).unwrap();
    lo_core::metadata::set_review(&db, metroid, &[cand("Metroid Prime")]).unwrap();
    assert_eq!(l.review_list().unwrap().len(), 2);

    // Skip: gone from review, and remembered
    let item = l.review_list().unwrap().into_iter().find(|i| i.game_id == icarus).unwrap();
    l.skip("match", &item.skip_key, &item.title, Some(&item.system_id)).unwrap();
    assert_eq!(l.review_list().unwrap().iter().map(|i| i.game_id).collect::<Vec<_>>(), [metroid]);
    // No match: remembered too, so bulk lookups leave it alone
    l.reject_metadata(metroid).unwrap();
    assert_eq!(l.skipped("match").unwrap().len(), 2);

    // looking a game up yourself brings it back
    l.look_up(&[icarus]).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    assert_eq!(l.skipped("match").unwrap().len(), 1, "Kid Icarus is no longer skipped; Metroid still is");
    // and bringing one back by hand works the same way
    let k = l.skipped("match").unwrap()[0].key.clone();
    l.unskip("match", &k).unwrap();
    assert!(l.skipped("match").unwrap().is_empty());
}

#[test]
fn ignored_cleanup_kept_duplicates_and_kept_names_stay_gone() {
    let (l, _h, root) = lib(&[
        ("Nintendo - NES/Zelda (USA).nes", "same bytes"),
        ("Nintendo - NES/Zelda copy (USA).nes", "same bytes"),
        ("Nintendo - NES/Site.url", "[InternetShortcut]"),
        ("Nintendo - NES/1942 (JU).zip", "t"),
    ]);
    // cleanup: ignore the shortcut
    let junk = l.junk().unwrap();
    let url = junk.iter().find(|j| j.path.ends_with("Site.url")).unwrap().path.clone();
    l.skip("junk", &url.to_string_lossy(), "Site.url", Some("nes")).unwrap();
    assert!(l.junk().unwrap().iter().all(|j| j.path != url));

    // duplicates: keep both copies of Zelda
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let groups = l.dupes(None).unwrap();
    let g = groups.iter().find(|g| g.title.contains("Zelda")).expect("the two Zeldas are duplicates");
    l.skip("dupe", &g.skip_key, &g.title, Some("nes")).unwrap();
    assert!(l.dupes(None).unwrap().iter().all(|x| !x.title.contains("Zelda")));
    // a third copy is a new situation, so the group is suggested again
    std::fs::write(root.path().join("Nintendo - NES/Zelda (USA) (Alt).nes"), "same bytes").unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.hash(HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    assert!(l.dupes(None).unwrap().iter().any(|x| x.title.contains("Zelda")), "a changed group comes back");

    // renames: keep "1942 (JU)" as it is
    let g1942 = id_of(&l, "1942");
    let plan = l.plan_renames_with(&[g1942], &[]).unwrap();
    let it = plan.iter().find(|i| i.problem.is_none()).expect("1942 (JU) would be renamed");
    l.skip("rename", &it.skip_key, "1942 (JU).zip", Some("nes")).unwrap();
    assert!(l.plan_renames_with(&[g1942], &[]).unwrap().iter().all(|i| i.skip_key != it.skip_key));
}

#[test]
fn a_match_you_picked_is_kept_until_you_choose_again() {
    let (l, _h, _r) = lib(&[("Nintendo - Switch/Atelier Ayesha [0100D9D00EE8C000][v0].nsp", "x")]);
    l.import_titledb("switch", std::path::Path::new(&format!("{}/tests/fixtures/switch_titledb.json", env!("CARGO_MANIFEST_DIR")))).unwrap();
    let g = id_of(&l, "Ayesha");
    let meta = |l: &Library| { let d = l.game_detail(g).unwrap(); (d.row.meta_state.clone(), d.meta.map(|m| (m["title"].as_str().unwrap_or_default().to_string(), m["chosen"].as_bool().unwrap_or(false)))) };

    // you pick a match yourself
    let mine = MetaCandidate { source: "steamgriddb".into(), source_id: "42".into(), title: "Atelier Ayesha (my pick)".into(), score: 0.6, ..Default::default() };
    l.choose_metadata(g, mine).unwrap();
    assert_eq!(meta(&l).1, Some(("Atelier Ayesha (my pick)".into(), true)));

    // a bulk lookup leaves it alone, and so does a refresh
    l.match_metadata(&[g]);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    l.look_up(&[g]).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    assert_eq!(meta(&l).1, Some(("Atelier Ayesha (my pick)".into(), true)), "your pick is kept");

    // "Wrong match? Choose again": back to review with the options, even a confident one
    l.choose_again(g).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let r = l.review_list().unwrap();
    assert_eq!(r.len(), 1);
    assert!(r[0].candidates.iter().any(|c| c.title == "Atelier Ayesha: The Alchemist of Dusk DX"));
}

#[test]
fn settling_review_matches_the_sure_ones_and_leaves_yours_to_you() {
    let (l, _h, _r) = lib(&[("Sega - Master System/Gangster Town (USA).sms", "a"), ("Sega - Master System/Psychic World (USA).sms", "b"), ("Sega - Master System/Spot (USA).sms", "c")]);
    let (gangster, psychic, spot) = (id_of(&l, "Gangster"), id_of(&l, "Psychic"), id_of(&l, "Spot"));
    // stored before Master System games listed as "Mark III" counted as this console
    let mark3 = |t: &str| MetaCandidate { source: "igdb".into(), source_id: t.into(), title: t.into(), score: 1.0, console: Some("other".into()), platforms: vec!["Sega Master System/Mark III".into()], ..Default::default() };
    let db = l.db().unwrap();
    lo_core::metadata::set_review(&db, gangster, &[mark3("Gangster Town")]).unwrap();
    lo_core::metadata::set_review(&db, psychic, &[mark3("Psychic World")]).unwrap();
    db.conn().execute("UPDATE games SET meta_json=json_set(meta_json,'$.again',1) WHERE id=?1", [psychic]).unwrap(); // "Choose again"
    let two = |t: &str| MetaCandidate { source: "igdb".into(), source_id: t.into(), title: t.into(), score: 0.92, console: Some("match".into()), ..Default::default() };
    lo_core::metadata::set_review(&db, spot, &[two("Spot: The Video Game"), two("Spot: The Cool Adventure")]).unwrap();

    let (sure, left) = l.settle_review(&|_, _, _| true).unwrap();
    assert_eq!((sure, left), (1, 1));
    assert_eq!(l.game_detail(gangster).unwrap().row.meta_state.as_deref(), Some("matched"));
    let waiting: Vec<i64> = l.review_list().unwrap().iter().map(|i| i.game_id).collect();
    assert!(waiting.contains(&psychic) && waiting.contains(&spot), "yours to choose, and two different games: {waiting:?}");
}
