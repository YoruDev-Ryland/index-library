//! Tools in game folders move into the system's _Tools folder, as whole units.
mod common;
use lo_core::jobs::JobEvent;
use lo_core::junk::JunkKind;
use lo_core::library::Library;
use std::sync::Arc;
use std::time::Duration;

fn lib(files: &[(&str, u64)]) -> (Arc<Library>, tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    for (p, size) in files {
        let full = root.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, p.as_bytes()).unwrap();
        if *size > 0 { std::fs::OpenOptions::new().write(true).open(&full).unwrap().set_len(*size).unwrap(); }
    }
    let l = Library::open_at(home.path(), Arc::new(|_e: JobEvent| {})).unwrap();
    let mut s = l.settings();
    s.library_roots = vec![root.path().to_path_buf()];
    l.save_settings(s).unwrap();
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    (l, home, root)
}

const FILES: &[(&str, u64)] = &[
    ("Sony - PSP/Games/Alpha (USA).iso", 0),
    ("Sony - PSP/Games/Beta (USA).iso", 0),
    // a program with its own small data files, one of them a game-type file
    ("Sony - PSP/PSX2PSP v1.4.2/PSX2PSP.exe", 0),
    ("Sony - PSP/PSX2PSP v1.4.2/Files/popstation.dll", 0),
    ("Sony - PSP/PSX2PSP v1.4.2/Files/BASE.PBP", 0),
    // a script with the list it works on
    ("Sony - PSP/rename.py", 0),
    ("Sony - PSP/PSP_GAMES.tsv", 0),
    // a folder of games that happens to hold a patcher: not a tool folder
    ("Sony - PSP/Hacks/One (USA).iso", 0),
    ("Sony - PSP/Hacks/Two (USA).iso", 0),
    ("Sony - PSP/Hacks/Three (USA).iso", 0),
    ("Sony - PSP/Hacks/Four (USA).iso", 0),
    ("Sony - PSP/Hacks/patcher.exe", 0),
];

#[test]
fn finds_tool_units_and_moves_them_whole() {
    let (l, _h, root) = lib(FILES);
    let p = |rel: &str| root.path().join(rel);
    let j = l.junk().unwrap();
    let of = |k: JunkKind| j.iter().filter(|x| x.kind == k).collect::<Vec<_>>();
    assert_eq!(of(JunkKind::ToolFolder).iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![p("Sony - PSP/PSX2PSP v1.4.2")], "{j:#?}");
    let files = of(JunkKind::ToolFiles);
    assert_eq!(files.len(), 1, "{j:#?}");
    assert_eq!(files[0].members, vec![p("Sony - PSP/PSP_GAMES.tsv"), p("Sony - PSP/rename.py")]);
    assert_eq!(of(JunkKind::Executable).iter().map(|x| x.path.clone()).collect::<Vec<_>>(), vec![p("Sony - PSP/Hacks/patcher.exe")], "games beside a program stay put");
    assert!(!j.iter().any(|x| x.path.to_string_lossy().contains("BASE.PBP") || x.path.to_string_lossy().contains("popstation")), "a program folder's own files aren't listed one by one");

    let mut paths = vec![p("Sony - PSP/PSX2PSP v1.4.2")];
    paths.extend(files[0].members.iter().cloned());
    let r = l.move_to_tools(&paths).unwrap();
    assert!(r.failed.is_empty(), "{:?}", r.failed);
    assert!(p("Sony - PSP/_Tools/PSX2PSP v1.4.2/Files/BASE.PBP").exists());
    assert!(p("Sony - PSP/_Tools/rename.py").exists() && p("Sony - PSP/_Tools/PSP_GAMES.tsv").exists());

    // _Tools is never scanned as games, and Cleanup leaves it alone
    l.scan(false);
    assert!(l.jobs.wait_idle(Duration::from_secs(20)));
    let n: i64 = l.db().unwrap().conn().query_row("SELECT count(*) FROM files WHERE path LIKE '%BASE.PBP'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
    let j = l.junk().unwrap();
    assert!(!j.iter().any(|x| x.path.starts_with(p("Sony - PSP/_Tools"))), "{j:#?}");

    // one undo puts everything back
    l.undo(&r.batch_id).unwrap();
    assert!(p("Sony - PSP/PSX2PSP v1.4.2/PSX2PSP.exe").exists() && p("Sony - PSP/rename.py").exists());
}

#[test]
fn a_folder_holding_games_is_never_moved_into_tools() {
    let (l, _h, root) = lib(&[("Sony - PSP/Games/Alpha (USA).iso", 0), ("Sony - PSP/Mixed/tool.exe", 0), ("Sony - PSP/Mixed/Big Game (USA).iso", 80 << 20)]);
    let r = l.move_to_tools(&[root.path().join("Sony - PSP/Mixed")]).unwrap();
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("Big Game"), "{:?}", r.failed);
    assert!(root.path().join("Sony - PSP/Mixed/Big Game (USA).iso").exists());
}
