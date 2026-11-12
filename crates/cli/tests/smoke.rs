#[test]
fn cli_smoke_scan_then_stats() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("Nintendo - NES")).unwrap();
    std::fs::write(root.path().join("Nintendo - NES/1942 (JU).zip"), b"x").unwrap();
    let h = home.path().to_string_lossy().to_string();
    let run = |a: &[&str]| -> String {
        let mut args = vec!["ix".to_string(), "--home".into(), h.clone()];
        args.extend(a.iter().map(|s| s.to_string()));
        let mut out = Vec::new();
        lo_cli::run(&args, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    run(&["config", "--add-root", &root.path().to_string_lossy()]);
    assert!(run(&["scan"]).contains("+1"));
    assert!(run(&["stats"]).contains("NES"));
    let preview = run(&["rename"]);
    assert!(preview.contains("1942 (Japan, USA).zip"), "{preview}");
    assert!(root.path().join("Nintendo - NES/1942 (JU).zip").exists(), "preview must not rename");
}
