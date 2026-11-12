#![cfg(unix)]
mod common;
use common::*;
use lo_core::actions::{by_id, execute};
use lo_core::fsx::LocalFs;
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::Journal;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn shim(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}
fn prep(f: &mut Fixture, tools: &tempfile::TempDir) {
    f.settings.temp_dir = Some(tools.path().join("tmp"));
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    for s in lo_core::systems::registry() { lo_core::probe::run_probes(&f.db, &LocalFs, s, &AtomicBool::new(false)).unwrap(); }
}
fn ids(f: &Fixture, ext: &str) -> Vec<i64> {
    let mut q = f.db.conn().prepare("SELECT id FROM files WHERE ext=?1").unwrap();
    q.query_map([ext], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
}
fn ls(f: &Fixture, d: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(f.root().join(d)).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort(); v
}
fn journal(f: &Fixture) -> Journal { Journal::new(Arc::new(LocalFs), f.settings.library_roots.clone(), "m".into()) }

#[test]
fn ps1_cue_bin_to_chd() {
    let mut f = Fixture::new();
    f.put("Sony - PS1/Game (USA).cue", b"FILE \"Game (USA) (Track 1).bin\" BINARY\nFILE \"Game (USA) (Track 2).bin\" BINARY\n");
    f.put("Sony - PS1/Game (USA) (Track 1).bin", b"t1");
    f.put("Sony - PS1/Game (USA) (Track 2).bin", b"t2");
    let tools = tempfile::tempdir().unwrap();
    // fake chdman: verifies the bins were copied next to the cue, writes the output
    let c = shim(tools.path(), "chdman", r#"[ "$1" = verify ] && exit 0; in="$3"; out="$5"; d=$(dirname "$in"); [ -f "$d/Game (USA) (Track 2).bin" ] || { echo missing bin >&2; exit 2; }; echo CHD > "$out""#);
    f.settings.tools.insert("chdman".into(), c);
    prep(&mut f, &tools);
    let r = execute(&f.db, &LocalFs, &f.settings, &journal(&f), by_id("compress.ps1_chd").unwrap(), &ids(&f, "cue"), None, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.ok, 1, "{r:?}");
    assert_eq!(ls(&f, "Sony - PS1"), vec!["Game (USA).chd", "_archive"]);
    assert_eq!(ls(&f, "Sony - PS1/_archive").len(), 4, "cue + 2 bins + manifest");
    assert_eq!(f.count("SELECT count(*) FROM status WHERE key='compressed' AND value='true'"), 1);
}

#[test]
fn missing_tool_reported_by_requirements() {
    let mut s = lo_core::config::Settings::default();
    s.tools.insert("chdman".into(), "/nonexistent/chdman".into());
    let req = by_id("compress.ps1_chd").unwrap().requires(&s);
    assert!(req.iter().any(|r| !r.ok && r.hint.contains("chdman")), "{req:?}");
}

#[test]
fn psp_pkg_to_cso_named_from_title_db() {
    let mut f = Fixture::new();
    let mut pkg = vec![0u8; 0x200];
    pkg[0..4].copy_from_slice(b"\x7FPKG");
    pkg[0x30..0x30 + 36].copy_from_slice(b"UP0555-NPUF30007_00-BOMBERMAN940EH01");
    f.put("Sony - PSP/Games/Bomberman 94.pkg", &pkg);
    let tools = tempfile::tempdir().unwrap();
    let p = shim(tools.path(), "pkg2zip", r#"mkdir -p pspemu/ISO && echo CSO > "pspemu/ISO/Bomberman [NPUF30007].cso""#);
    f.settings.tools.insert("pkg2zip".into(), p);
    prep(&mut f, &tools);
    f.db.conn().execute("INSERT INTO titledb(source, id, name, region) VALUES('psp','UP0555-NPUF30007_00-BOMBERMAN940EH01','Bomberman ''94 (PC Engine)','US')", []).unwrap();
    let r = execute(&f.db, &LocalFs, &f.settings, &journal(&f), by_id("psp.pkg_to_cso").unwrap(), &ids(&f, "pkg"), None, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.ok, 1, "{r:?}");
    assert!(ls(&f, "Sony - PSP").contains(&"Bomberman '94 (PC Engine) (USA).cso".to_string()), "{:?}", ls(&f, "Sony - PSP"));
    assert!(ls(&f, "Sony - PSP/_archive").contains(&"Bomberman 94.pkg".to_string()));
    assert!(ls(&f, "Sony - PSP/Games").is_empty());
}

#[test]
fn psp_eboot_output_is_reported_unsupported() {
    let mut f = Fixture::new();
    let mut pkg = vec![0u8; 0x200];
    pkg[0..4].copy_from_slice(b"\x7FPKG");
    f.put("Sony - PSP/Games/Mini.pkg", &pkg);
    let tools = tempfile::tempdir().unwrap();
    let p = shim(tools.path(), "pkg2zip", r#"mkdir -p pspemu/PSP/GAME/X && echo E > pspemu/PSP/GAME/X/EBOOT.PBP"#);
    f.settings.tools.insert("pkg2zip".into(), p);
    prep(&mut f, &tools);
    let r = execute(&f.db, &LocalFs, &f.settings, &journal(&f), by_id("psp.pkg_to_cso").unwrap(), &ids(&f, "pkg"), None, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("EBOOT"), "{:?}", r.failed);
    assert_eq!(ls(&f, "Sony - PSP/Games"), vec!["Mini.pkg"]);
}

#[test]
fn i10_chd_output_failing_verification_keeps_original() {
    let mut f = Fixture::new();
    f.put("Sony - PS1/Game (USA).cue", b"FILE \"Game (USA).bin\" BINARY\n");
    f.put("Sony - PS1/Game (USA).bin", b"t1");
    let tools = tempfile::tempdir().unwrap();
    let c = shim(tools.path(), "chdman", r#"[ "$1" = verify ] && { echo "verify failed: bad hunk" >&2; exit 1; }; echo CHD > "$5""#);
    f.settings.tools.insert("chdman".into(), c);
    prep(&mut f, &tools);
    let r = execute(&f.db, &LocalFs, &f.settings, &journal(&f), by_id("compress.ps1_chd").unwrap(), &ids(&f, "cue"), None, &AtomicBool::new(false)).unwrap();
    assert_eq!(r.failed.len(), 1, "{r:?}");
    assert_eq!(ls(&f, "Sony - PS1"), vec!["Game (USA).bin", "Game (USA).cue"]);
}
