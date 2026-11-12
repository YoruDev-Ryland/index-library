mod common;
use common::*;
use lo_core::fsx::{LocalFs, Transport};
use lo_core::ops::{apply, undo_batch, Op, OpKind};
use lo_core::parse::ParsedName;
use lo_core::rename::{plan_renames, render_template, to_ops, Problem};
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::Journal;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn rescan(f: &Fixture) { scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap(); }
fn all_games(f: &Fixture) -> Vec<i64> { let mut q = f.db.conn().prepare("SELECT id FROM games").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() }
fn journal(f: &Fixture) -> Journal { Journal::new(Arc::new(LocalFs), f.settings.library_roots.clone(), "test".into()) }
fn names(f: &Fixture, dir: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(f.root().join(dir)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort(); v
}

#[test]
fn template_rendering() {
    let p = ParsedName { title: "Metroid Prime".into(), regions: vec!["USA".into()], ..Default::default() };
    assert_eq!(render_template("{title} ({regions}){ (rev)}", &p), "Metroid Prime (USA)");
    let q = ParsedName { title: "Alien vs Predator".into(), regions: vec!["USA".into(), "Europe".into()], rev: Some("1".into()), tags: vec!["Proto".into(), "1993-12-17".into(), "verified-good".into()], ..Default::default() };
    assert_eq!(render_template("{title}{ ($regions)}{ ($langs)}{ (Rev $rev)}{ ($tags)}", &q), "Alien vs Predator (USA, Europe) (Rev 1) (Proto) (1993-12-17)");
}

#[test]
fn goodtools_names_cleaned_and_decrypted_suffix_removed() {
    let f = Fixture::new();
    f.put("Nintendo - NES/1942 (JU).zip", b"a");
    f.put("Nintendo - 3DS/Mario Kart 7 (USA)-decrypted.cia", b"b");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let dst: Vec<String> = plan.iter().filter(|i| i.problem.is_none()).map(|i| i.dst.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert!(dst.contains(&"1942 (Japan, USA).zip".to_string()), "{dst:?}");
    assert!(dst.contains(&"Mario Kart 7 (USA).cia".to_string()), "{dst:?}");
}

#[test]
fn collisions_flagged() {
    let f = Fixture::new();
    f.put("Nintendo - NES/Game (U).zip", b"a");
    f.put("Nintendo - NES/Game (USA).zip", b"b");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let probs: Vec<_> = plan.iter().map(|i| i.problem.clone()).collect();
    // "Game (USA).zip" is unchanged; the GoodTools one would collide with it
    assert!(probs.iter().any(|p| matches!(p, Some(Problem::TargetExists) | Some(Problem::Collision))), "{plan:?}");
    assert!(plan.iter().all(|i| i.problem.is_some()), "nothing should be applied: {plan:?}");
}

#[test]
fn windows_illegal_names_flagged() {
    use lo_core::rename::windows_problem;
    assert!(windows_problem("a:b.zip").is_some());
    assert!(windows_problem("CON.zip").is_some());
    assert!(windows_problem("con.ZIP").is_some());
    assert!(windows_problem("trailing.").is_some());
    assert!(windows_problem("Atelier Ayesha꞉ X.nsp").is_none());
    assert!(windows_problem("Rock & Roll's.zip").is_none());
}

#[test]
fn case_only_rename_and_undo() {
    let f = Fixture::new();
    let src = f.put("Nintendo - NES/zelda (USA).zip", b"z");
    rescan(&f);
    let j = journal(&f);
    let dst = src.with_file_name("Zelda (USA).zip");
    let r = apply(&f.db, &LocalFs, &j, vec![Op::rename(src.clone(), dst.clone())], "test").unwrap();
    assert!(r.failed.is_empty(), "{r:?}");
    assert_eq!(names(&f, "Nintendo - NES"), vec!["Zelda (USA).zip"]);
    let u = undo_batch(&f.db, &LocalFs, &j, &r.batch_id).unwrap();
    assert!(u.failed.is_empty(), "{u:?}");
    assert_eq!(names(&f, "Nintendo - NES"), vec!["zelda (USA).zip"]);
}

#[test]
fn plan_apply_undo_roundtrip() {
    let f = Fixture::new();
    f.put("Nintendo - NES/1942 (JU).zip", b"a");
    f.put("Nintendo - NES/10-Yard Fight (U).zip", b"b");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let ops = to_ops(&f.db, &LocalFs, &plan).unwrap();
    let j = journal(&f);
    let r = apply(&f.db, &LocalFs, &j, ops, "rename").unwrap();
    assert_eq!(r.done.len(), 2, "{r:?}");
    assert_eq!(names(&f, "Nintendo - NES"), vec!["10-Yard Fight (USA).zip", "1942 (Japan, USA).zip"]);
    assert_eq!(f.count("SELECT count(*) FROM files WHERE name='1942 (Japan, USA).zip'"), 1);
    undo_batch(&f.db, &LocalFs, &j, &r.batch_id).unwrap();
    assert_eq!(names(&f, "Nintendo - NES"), vec!["10-Yard Fight (U).zip", "1942 (JU).zip"]);
}

#[test]
fn file_changed_after_plan_is_refused() {
    let f = Fixture::new();
    f.put("Nintendo - NES/1942 (JU).zip", b"a");
    f.put("Nintendo - NES/10-Yard Fight (U).zip", b"b");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let ops = to_ops(&f.db, &LocalFs, &plan).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(f.root().join("Nintendo - NES/1942 (JU).zip"), b"changed!").unwrap();
    let r = apply(&f.db, &LocalFs, &journal(&f), ops, "rename").unwrap();
    assert_eq!(r.done.len(), 1);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("changed"), "{:?}", r.failed);
}

#[test]
fn undo_refused_when_file_moved_away() {
    let f = Fixture::new();
    let src = f.put("Nintendo - NES/a (U).zip", b"a");
    rescan(&f);
    let j = journal(&f);
    let dst = src.with_file_name("a (USA).zip");
    let r = apply(&f.db, &LocalFs, &j, vec![Op::rename(src.clone(), dst.clone())], "t").unwrap();
    std::fs::remove_file(&dst).unwrap();
    let u = undo_batch(&f.db, &LocalFs, &j, &r.batch_id).unwrap();
    assert_eq!(u.failed.len(), 1);
}

#[test]
fn cue_references_follow_bin_rename() {
    let f = Fixture::new();
    f.put("Sony - PS1/Game (U) (Disc 1).bin", b"bin");
    f.put("Sony - PS1/Game (U) (Disc 1).cue", b"FILE \"Game (U) (Disc 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n");
    rescan(&f);
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let ops = to_ops(&f.db, &LocalFs, &plan).unwrap();
    assert!(ops.iter().any(|o| o.kind == OpKind::WriteFile));
    let r = apply(&f.db, &LocalFs, &journal(&f), ops, "rename").unwrap();
    assert!(r.failed.is_empty(), "{r:?}");
    let cue = std::fs::read_to_string(f.root().join("Sony - PS1/Game (USA) (Disc 1).cue")).unwrap();
    assert!(cue.contains("FILE \"Game (USA) (Disc 1).bin\" BINARY"), "{cue}");
    undo_batch(&f.db, &LocalFs, &journal(&f), &r.batch_id).unwrap();
    let cue = std::fs::read_to_string(f.root().join("Sony - PS1/Game (U) (Disc 1).cue")).unwrap();
    assert!(cue.contains("FILE \"Game (U) (Disc 1).bin\""), "{cue}");
    let _ = LocalFs.exists(f.root());
}

#[test]
fn exact_matches_name_games_and_similarity_matches_are_only_offered() {
    let f = Fixture::new();
    f.put("Nintendo - 3DS/New Super Mario Bros. 2 - Gold Edition (USA).3ds", b"gold");
    f.put("Nintendo - NES/zelda (USA).zip", b"z");
    f.put("Nintendo - NES/Metroid (USA).zip", b"m");
    rescan(&f);
    let id = |t: &str| -> i64 { f.db.conn().query_row("SELECT id FROM games WHERE title LIKE ?1", [format!("%{t}%")], |r| r.get(0)).unwrap() };
    let set = |g: i64, source: &str, title: &str, score: f64| {
        f.db.conn().execute("UPDATE games SET meta_state='matched', meta_json=?1 WHERE id=?2", (serde_json::json!({"source": source, "title": title, "score": score}).to_string(), g)).unwrap();
    };
    // a name-similarity match that is wrong: Gold Edition is its own release
    set(id("Gold Edition"), "steamgriddb", "New Super Mario Bros. 2", 0.9);
    // an exact match by id: its official name (trademark signs and ":" made fit for a file name)
    set(id("zelda"), "titledb-switch", "The Legend of Zelda™: Classic", 1.0);
    let by = |plan: &[lo_core::rename::RenameItem], t: &str| plan.iter().find(|i| i.src.to_string_lossy().contains(t)).unwrap().clone();

    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    let nsmb = by(&plan, "Gold Edition");
    assert!(nsmb.dst.to_string_lossy().contains("Gold Edition"), "a similarity match never renames on its own: {nsmb:?}");
    assert_eq!((nsmb.suggestion.as_deref(), nsmb.suggestion_source.as_deref(), nsmb.using_suggestion), (Some("New Super Mario Bros. 2"), Some("SteamGridDB"), false));
    let zelda = by(&plan, "zelda");
    assert_eq!(zelda.dst.file_name().unwrap().to_string_lossy(), "The Legend of Zelda - Classic (USA).zip");
    assert_eq!((zelda.source.as_str(), zelda.suggestion.is_none()), ("official", true));
    assert_eq!(by(&plan, "Metroid").suggestion, None, "no match, nothing to offer");

    // a match checked to be for another console is never offered as a name
    f.db.conn().execute("UPDATE games SET meta_json=?1 WHERE id=?2", (serde_json::json!({"source": "steamgriddb", "title": "Super Mario Advance 4", "score": 0.9, "console": "other", "platforms": ["Game Boy Advance"]}).to_string(), id("Metroid"))).unwrap();
    let plan = plan_renames(&f.db, &LocalFs, &all_games(&f)).unwrap();
    assert_eq!(by(&plan, "Metroid").suggestion, None);

    // choosing the suggestion for that game uses it
    let plan = lo_core::rename::plan_renames_with(&f.db, &LocalFs, &all_games(&f), &[id("Gold Edition")]).unwrap();
    let nsmb = by(&plan, "Gold Edition");
    assert_eq!((nsmb.dst.file_name().unwrap().to_string_lossy().as_ref(), nsmb.source.as_str(), nsmb.using_suggestion), ("New Super Mario Bros. 2 (USA).3ds", "matched", true));
}

#[test]
fn file_roles_for_grouping_the_preview() {
    use lo_core::util::file_role;
    let r = |s: &str, n: &str, id: Option<&str>| { let (k, l, _) = file_role(s, n, id); (k, l) };
    assert_eq!(r("switch", "Atelier Ryza [0100D1900EC80000][v0].nsp", None), ("game", "Base game".into()));
    assert_eq!(r("switch", "Atelier Ryza [0100D1900EC80800][v65536].nsp", None), ("update", "Update".into()));
    assert_eq!(r("switch", "whatever.nsp", Some("0100D1900EC81003")), ("dlc", "DLC 3".into()));
    assert_eq!(r("psvita", "Persona 4 Golden (Update 1.01).pkg", None), ("update", "Update".into()));
    assert_eq!(r("psp", "Some Game [DLC] Costume.pkg", None), ("dlc", "DLC".into()));
    assert_eq!(r("ps1", "Final Fantasy VII (USA) (Disc 2).chd", None), ("disc", "Disc 2".into()));
    assert_eq!(r("nes", "Tetris (World).nes", None), ("game", "Game".into()));
}
