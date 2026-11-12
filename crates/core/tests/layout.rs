//! Folder layout: each system follows the layout most of its games use; outliers are moved in line.
mod common;
use lo_core::jobs::JobEvent;
use lo_core::layout::{Layout, Problem};
use lo_core::library::Library;
use std::sync::Arc;
use std::time::Duration;

fn lib(files: &[&str]) -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    for (i, p) in files.iter().enumerate() {
        let full = root.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        // same content for files that share a name, so spare copies are real duplicates
        let name = full.file_name().unwrap().to_string_lossy().into_owned();
        std::fs::write(&full, format!("{name}{}", if name.contains("v65536") { 0 } else { i })).unwrap();
    }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    (l, home, root)
}

/// Fingerprint everything, so identical copies are proven identical.
fn fingerprint(l: &Arc<Library>) {
    l.hash(lo_core::library::HashScope::All).unwrap();
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
}

const FILES: &[&str] = &[
    // Switch: one folder per game, except a split game and a loose one
    "Nintendo - Switch/Game A [0100000000010000]/Game A [0100000000010000][v0].nsp",
    "Nintendo - Switch/Game B [0100000000020000]/Game B [0100000000020000][v0].nsp",
    "Nintendo - Switch/Game C [0100000000030000]/Game C [0100000000030000][v0].nsp",
    "Nintendo - Switch/Game A/Game A [0100000000010800][v65536].nsp",
    "Nintendo - Switch/Game A copy/Game A [0100000000010800][v65536].nsp",
    "Nintendo - Switch/Game D [0100000000040000][v0].nsp",
    // NES: loose, except one game in its own folder
    "Nintendo - NES/Zelda (USA).zip",
    "Nintendo - NES/Metroid (USA).zip",
    "Nintendo - NES/Kid Icarus/Kid Icarus (USA).zip",
    // PSP: shared folders; updates beside games are normal, a loose game is not
    "Sony - PSP/Games/Alpha (USA).iso",
    "Sony - PSP/Games/Beta (USA).iso",
    "Sony - PSP/Games/Delta (USA).iso",
    "Sony - PSP/Updates/Alpha (USA).pbp",
    "Sony - PSP/Gamma (USA).iso",
];

#[test]
fn detects_each_systems_layout_and_flags_outliers() {
    let (l, _h, root) = lib(FILES);
    // not fingerprinted yet: nothing is proven a copy, so both update files are kept (moved in)
    let r = l.layout(None).unwrap();
    let a = r.iter().find(|s| s.system_id == "switch").unwrap().items.iter().find(|i| i.title == "Game A").unwrap().clone();
    assert_eq!((a.moves.len(), a.duplicates.len()), (2, 0), "{a:?}");
    assert!(a.moves.iter().any(|m| m.dst.to_string_lossy().ends_with("[v65536] (2).nsp")), "a name clash keeps both: {a:?}");
    fingerprint(&l);
    let r = l.layout(None).unwrap();
    let sys = |id: &str| r.iter().find(|s| s.system_id == id).unwrap_or_else(|| panic!("{id} in {r:?}"));

    let sw = sys("switch");
    assert_eq!(sw.layout, Layout::PerGame);
    let a = sw.items.iter().find(|i| i.title == "Game A").expect("split game");
    assert_eq!(a.problem, Problem::Split);
    assert_eq!(a.target.as_deref(), Some(root.path().join("Nintendo - Switch/Game A [0100000000010000]").as_path()));
    assert_eq!((a.moves.len(), a.duplicates.len()), (1, 1), "{a:?}");
    let d = sw.items.iter().find(|i| i.title == "Game D").expect("loose game");
    assert_eq!(d.problem, Problem::NeedsFolder);
    assert_eq!(d.target.as_deref(), Some(root.path().join("Nintendo - Switch/Game D [0100000000040000]").as_path()), "new folders follow the [id] naming");
    assert_eq!(sw.items.len(), 2);

    let nes = sys("nes");
    assert_eq!(nes.layout, Layout::Loose);
    assert_eq!(nes.items.len(), 1);
    assert_eq!(nes.items[0].problem, Problem::InOwnFolder);

    let psp = sys("psp");
    assert_eq!(psp.layout, Layout::Grouped);
    assert_eq!(psp.items.len(), 1, "{:?}", psp.items);
    assert_eq!((psp.items[0].problem, psp.items[0].moves.len()), (Problem::NotFiled, 0));
}

#[test]
fn fix_moves_files_archives_spares_and_removes_emptied_folders() {
    let (l, _h, root) = lib(FILES);
    fingerprint(&l);
    let p = |rel: &str| root.path().join(rel);
    let ids: Vec<i64> = l.layout(None).unwrap().iter().flat_map(|s| s.items.iter().map(|i| i.game_id)).collect();
    let res = l.fix_layout(&ids).unwrap();
    assert!(res.failed.is_empty(), "{:?}", res.failed);
    assert!(p("Nintendo - Switch/Game A [0100000000010000]/Game A [0100000000010800][v65536].nsp").exists());
    assert!(!p("Nintendo - Switch/Game A").exists(), "emptied folder removed");
    assert!(!p("Nintendo - Switch/Game A copy").exists(), "spare copy archived, its folder removed");
    assert!(p("Nintendo - Switch/Game D [0100000000040000]/Game D [0100000000040000][v0].nsp").exists());
    assert!(p("Nintendo - NES/Kid Icarus (USA).zip").exists() && !p("Nintendo - NES/Kid Icarus").exists());
    assert!(p("Sony - PSP/Gamma (USA).iso").exists(), "no automatic fix for an unfiled game");
    assert!(l.layout(None).unwrap().iter().all(|s| s.items.iter().all(|i| i.problem == Problem::NotFiled)));
    // one undo puts the moves back (the spare copy stays in the archive, restorable from there)
    l.undo(&res.batch_id).unwrap();
    let upd = "Game A [0100000000010800][v65536].nsp";
    assert!(p(&format!("Nintendo - Switch/Game A/{upd}")).exists() != p(&format!("Nintendo - Switch/Game A copy/{upd}")).exists());
    assert_eq!(l.archive_list().unwrap().len(), 1);
    assert!(p("Nintendo - NES/Kid Icarus/Kid Icarus (USA).zip").exists());
}

#[test]
fn chosen_layout_overrides_detection() {
    let (l, _h, _root) = lib(FILES);
    let mut s = l.settings();
    s.folder_layouts.insert("nes".into(), "leave".into());
    s.folder_layouts.insert("switch".into(), "loose".into());
    l.save_settings(s).unwrap();
    let r = l.layout(None).unwrap();
    assert!(r.iter().find(|s| s.system_id == "nes").unwrap().items.is_empty());
    let sw = r.iter().find(|s| s.system_id == "switch").unwrap();
    assert!(sw.chosen && sw.layout == Layout::Loose && sw.detected == Layout::PerGame);
    assert!(sw.items.iter().all(|i| i.problem == Problem::InOwnFolder || i.problem == Problem::Split));
}
