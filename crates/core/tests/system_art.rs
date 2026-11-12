use lo_core::metadata::system_art::{image_for, parse_page_images, set_custom, wiki_title};

#[test]
fn every_system_has_a_wikipedia_article() {
    for s in lo_core::systems::registry() {
        assert!(wiki_title(s.id).is_some(), "no article for {}", s.id);
    }
}

#[test]
fn parses_page_image() {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/wiki_pageimages.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    // results are keyed by the title as requested, following MediaWiki's normalization/redirects
    let m = parse_page_images(&v);
    assert!(m["Nintendo_3DS"].contains("Nintendo-3DS-AquaOpen.png"), "{m:?}");
    let r = serde_json::json!({"query":{"redirects":[{"from":"Sega Genesis","to":"Sega Mega Drive"}],
        "pages":{"1":{"title":"Sega Mega Drive","thumbnail":{"source":"https://x/md.png"}},"-1":{"title":"Nope","missing":""}}}});
    let m = parse_page_images(&r);
    assert_eq!(m.get("Sega Genesis").map(String::as_str), Some("https://x/md.png"));
    assert!(!m.contains_key("Nope"));
}

#[test]
fn custom_image_wins_over_downloaded() {
    let cache = tempfile::tempdir().unwrap();
    let dir = cache.path().join("art/systems");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("n3ds.jpg"), b"wiki").unwrap();
    assert_eq!(image_for(cache.path(), "n3ds").unwrap().file_name().unwrap(), "n3ds.jpg");
    let src = cache.path().join("mine.png");
    std::fs::write(&src, b"mine").unwrap();
    set_custom(cache.path(), "n3ds", Some(&src)).unwrap();
    assert_eq!(image_for(cache.path(), "n3ds").unwrap().file_name().unwrap(), "custom-n3ds.png");
    set_custom(cache.path(), "n3ds", None).unwrap();
    assert_eq!(image_for(cache.path(), "n3ds").unwrap().file_name().unwrap(), "n3ds.jpg");
    assert!(image_for(cache.path(), "gba").is_none());
}
