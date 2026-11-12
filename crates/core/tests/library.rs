mod common;
use lo_core::jobs::JobEvent;
use lo_core::library::{GameFilter, GameSort, Library};
use std::sync::Arc;
use std::time::Duration;

fn lib() -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let libroot = tempfile::tempdir().unwrap();
    for (p, d) in [("Nintendo - NES/1942 (JU).zip", &b"a"[..]), ("Nintendo - NES/Zelda (USA).zip", b"bb"), ("Nintendo - 3DS/Mario Kart 7 (USA)-decrypted.cia", &lo_core::probe::testdata_cia(1, false, true))] {
        let full = libroot.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, d).unwrap();
    }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![libroot.path().to_path_buf()];
    l.save_settings(s).unwrap();
    (l, home, libroot)
}

#[test]
fn scan_then_query_games() {
    let (l, _h, _r) = lib();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let all = l.games(&GameFilter::default(), GameSort::Title, 0, 100).unwrap();
    assert_eq!(all.total, 3);
    let nes = l.games(&GameFilter { system: Some("nes".into()), ..Default::default() }, GameSort::Title, 0, 100).unwrap();
    assert_eq!(nes.total, 2);
    let s = l.games(&GameFilter { search: Some("zel".into()), ..Default::default() }, GameSort::Title, 0, 100).unwrap();
    assert_eq!(s.rows[0].title, "Zelda");
    let dec = l.games(&GameFilter { status_key: Some("decrypted".into()), status_value: Some("true".into()), ..Default::default() }, GameSort::Title, 0, 100).unwrap();
    assert_eq!(dec.total, 1, "probes ran after the scan job");
    let detail = l.game_detail(dec.rows[0].id).unwrap();
    assert_eq!(detail.files.len(), 1);
    let sys = l.systems().unwrap();
    assert!(sys.iter().any(|x| x.id == "nes" && x.games == 2));
}

#[test]
fn rename_preview_apply_undo_via_facade() {
    let (l, _h, r) = lib();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let ids: Vec<i64> = l.games(&GameFilter::default(), GameSort::Title, 0, 100).unwrap().rows.iter().map(|g| g.id).collect();
    let plan = l.plan_renames(&ids).unwrap();
    let ok: Vec<_> = plan.into_iter().filter(|i| i.problem.is_none()).collect();
    assert_eq!(ok.len(), 2, "{ok:?}");
    let res = l.apply_renames(&ok).unwrap();
    assert!(res.failed.is_empty());
    assert!(r.path().join("Nintendo - 3DS/Mario Kart 7 (USA).cia").exists());
    assert_eq!(l.history().unwrap()[0].ops, 2);
    l.undo(&res.batch_id).unwrap();
    assert!(r.path().join("Nintendo - NES/1942 (JU).zip").exists());
}

#[test]
fn run_action_refuses_when_requirements_missing() {
    let (l, _h, _r) = lib();
    let mut s = l.settings();
    s.tools.insert("chdman".into(), "/nonexistent/chdman".into());
    l.save_settings(s).unwrap();
    let e = l.run_action("compress.ps1_chd", &[1]).unwrap_err().to_string();
    assert!(e.contains("chdman"), "{e}");
}

#[test]
fn looking_up_info_without_sources_fails_with_guidance() {
    let (l, _h, _r) = lib();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let ids = l.game_ids_for(&GameFilter::default()).unwrap();
    let id = l.match_metadata(&ids);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let j = l.jobs.get(id).unwrap();
    assert_eq!(j.state, lo_core::jobs::JobState::Failed);
    assert!(j.error.unwrap_or_default().contains("Settings"));
}

#[test]
fn review_can_be_dismissed_and_research_needs_a_source() {
    let (l, _h, _r) = lib();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let id = l.game_ids_for(&GameFilter::default()).unwrap()[0];
    let db = l.db().unwrap();
    lo_core::metadata::set_review(&db, id, &[lo_core::metadata::MetaCandidate { source: "steamgriddb".into(), source_id: "1".into(), title: "Wrong Game".into(), score: 0.7, ..Default::default() }]).unwrap();
    assert_eq!(l.review_list().unwrap().len(), 1);
    l.reject_metadata(id).unwrap();
    assert!(l.review_list().unwrap().is_empty());
    let state: Option<String> = db.conn().query_row("SELECT meta_state FROM games WHERE id=?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(state.as_deref(), Some("none"));
    let e = l.search_metadata(id, "Some Other Name").unwrap_err().to_string();
    assert!(e.contains("Settings"), "{e}");
}

#[test]
fn info_lookup_logs_each_game_and_works_with_only_an_offline_title_list() {
    let (l, _h, r) = lib();
    let p = r.path().join("Nintendo - Switch/Atelier Ayesha [0100D9D00EE8C000]/Atelier Ayesha [0100D9D00EE8C000][v0].nsp");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"nsp").unwrap();
    l.import_titledb("switch", std::path::Path::new(&format!("{}/tests/fixtures/switch_titledb.json", env!("CARGO_MANIFEST_DIR")))).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let ids = l.game_ids_for(&GameFilter { system: Some("switch".into()), ..Default::default() }).unwrap();
    let id = l.match_metadata(&ids);
    assert!(l.jobs.wait_idle(Duration::from_secs(10)));
    let j = l.jobs.get(id).unwrap();
    assert_eq!(j.state, lo_core::jobs::JobState::Done, "{:?}", j.error);
    assert!(j.log.iter().any(|x| x.contains("Atelier") && x.contains("Switch titledb")), "{:?}", j.log);
}
