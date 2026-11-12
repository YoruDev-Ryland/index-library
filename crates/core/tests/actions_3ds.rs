mod common;
use common::*;
use lo_core::actions::{by_id, execute, ExecReport};
use lo_core::fsx::LocalFs;
use lo_core::probe::testdata_cia;
use lo_core::scan::{scan, ScanOpts};
use lo_core::shared::journal::Journal;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// A stand-in for decrypt_cia.py: copies ./decrypted.bin to <input stem>-decrypted.cia,
/// or misbehaves according to ./mode.
const FAKE_CIA: &str = r#"
import sys, os, shutil
here = os.path.dirname(os.path.abspath(__file__))
mode = open(os.path.join(here, "mode")).read().strip()
src = sys.argv[1]
out = os.path.splitext(src)[0] + "-decrypted.cia"
print("decrypting", os.path.basename(src))
if mode == "fail":
    print("boom", file=sys.stderr); sys.exit(1)
if mode == "noop":
    shutil.copy(src, out); sys.exit(0)
shutil.copy(os.path.join(here, "decrypted.bin"), out)
"#;

struct T { f: Fixture, tools: tempfile::TempDir, j: Journal }

fn setup(mode: &str, original_name: &str) -> T {
    let mut f = Fixture::new();
    f.put(&format!("Nintendo - 3DS/{original_name}"), &testdata_cia(0x0004000000030800, false, false));
    let tools = tempfile::tempdir().unwrap();
    std::fs::write(tools.path().join("decrypt_cia.py"), FAKE_CIA).unwrap();
    std::fs::write(tools.path().join("decrypted.bin"), testdata_cia(0x0004000000030800, false, true)).unwrap();
    std::fs::write(tools.path().join("mode"), mode).unwrap();
    f.settings.tools.insert("3ds-decrypt".into(), tools.path().to_path_buf());
    f.settings.temp_dir = Some(tools.path().join("tmp"));
    scan(&f.db, &LocalFs, &f.settings, ScanOpts::default(), &|_| {}, &AtomicBool::new(false)).unwrap();
    let s = lo_core::systems::by_id("n3ds").unwrap();
    lo_core::probe::run_probes(&f.db, &LocalFs, s, &AtomicBool::new(false)).unwrap();
    let j = Journal::new(Arc::new(LocalFs), f.settings.library_roots.clone(), "m".into());
    T { f, tools, j }
}
fn run(t: &T) -> ExecReport {
    let ids: Vec<i64> = { let mut q = t.f.db.conn().prepare("SELECT id FROM files").unwrap(); q.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect() };
    let a = by_id("n3ds.decrypt_cia").unwrap();
    execute(&t.f.db, &LocalFs, &t.f.settings, &t.j, a, &ids, None, &AtomicBool::new(false)).unwrap()
}
fn ls(t: &T, d: &str) -> Vec<String> {
    let p = t.f.root().join(d);
    if !p.exists() { return vec![]; }
    let mut v: Vec<String> = std::fs::read_dir(p).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort(); v
}
fn tmp_empty(t: &T) -> bool { let p = t.tools.path().join("tmp"); !p.exists() || std::fs::read_dir(p).unwrap().count() == 0 }

#[test]
fn applies_only_to_encrypted_cia() {
    let t = setup("ok", "Mario Kart 7 (USA).cia");
    let a = by_id("n3ds.decrypt_cia").unwrap();
    let rows = lo_core::actions::file_rows(&t.f.db, &[1]).unwrap();
    assert!(a.applies(&rows[0]));
    assert!(by_id("n3ds.decrypt_3ds").unwrap().applies(&rows[0]) == false);
}

#[test]
fn decrypt_success_same_name() {
    let t = setup("ok", "Mario Kart 7 (USA).cia");
    let r = run(&t);
    assert_eq!((r.ok, r.failed.len()), (1, 0), "{r:?}");
    assert_eq!(ls(&t, "Nintendo - 3DS"), vec!["Mario Kart 7 (USA).cia", "_archive"]);
    assert!(ls(&t, "Nintendo - 3DS/_archive").contains(&"Mario Kart 7 (USA).cia".to_string()));
    let out = std::fs::read(t.f.root().join("Nintendo - 3DS/Mario Kart 7 (USA).cia")).unwrap();
    assert_eq!(out, testdata_cia(0x0004000000030800, false, true));
    let v: String = t.f.db.conn().query_row("SELECT value FROM status WHERE key='decrypted'", [], |r| r.get(0)).unwrap();
    assert_eq!(v, "true");
    assert!(tmp_empty(&t));
    // the new file is in the index, attached to the same game
    assert_eq!(t.f.count("SELECT count(*) FROM files WHERE name='Mario Kart 7 (USA).cia' AND game_id IS NOT NULL"), 1);
}

#[test]
fn script_failure_leaves_original_untouched() {
    let t = setup("fail", "Mario Kart 7 (USA).cia");
    let r = run(&t);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("boom") || r.failed[0].1.contains("exit"), "{:?}", r.failed);
    assert_eq!(ls(&t, "Nintendo - 3DS"), vec!["Mario Kart 7 (USA).cia"]);
    assert_eq!(std::fs::read(t.f.root().join("Nintendo - 3DS/Mario Kart 7 (USA).cia")).unwrap(), testdata_cia(0x0004000000030800, false, false));
    assert!(tmp_empty(&t));
}

#[test]
fn still_encrypted_output_is_rejected() {
    let t = setup("noop", "Mario Kart 7 (USA).cia");
    let r = run(&t);
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].1.contains("still encrypted"), "{:?}", r.failed);
    assert_eq!(ls(&t, "Nintendo - 3DS"), vec!["Mario Kart 7 (USA).cia"]);
    assert!(tmp_empty(&t));
}

#[test]
fn suffix_names_get_clean_output() {
    let t = setup("ok", "Pokemon X (USA) Decrypted.cia");
    let r = run(&t);
    assert_eq!(r.ok, 1, "{r:?}");
    assert_eq!(ls(&t, "Nintendo - 3DS"), vec!["Pokemon X (USA).cia", "_archive"]);
}
