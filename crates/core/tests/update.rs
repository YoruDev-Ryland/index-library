//! Is there a newer Index? The GitHub API first; its download servers when the API says no.
use lo_core::update::{check_with, newer};

const API: &str = "https://api.github.com/repos/YoruDev-Ryland/index-library/releases/latest";
const LATEST_JSON: &str = "https://github.com/YoruDev-Ryland/index-library/releases/latest/download/latest.json";

fn api_body(tag: &str) -> String {
    format!(r#"{{"tag_name":"{tag}","name":"Index {tag}","body":"- Shelf\n- Updates","html_url":"https://github.com/YoruDev-Ryland/index-library/releases/tag/{tag}","prerelease":false,"draft":false}}"#)
}

#[test]
fn only_newer_versions_are_offered() {
    assert!(newer("v0.2.0", "0.1.0"));
    assert!(newer("0.10.0", "0.9.3"));
    assert!(!newer("v0.1.0", "0.1.0"));
    assert!(!newer("0.0.9", "0.1.0"));
    assert!(!newer("0.2.0-beta.1", "0.2.0"));
    assert!(newer("0.2.0", "0.2.0-beta.1"));
    assert!(!newer("garbage", "0.1.0"));
}

#[test]
fn api_release_gives_version_notes_and_page() {
    let fetch = |url: &str| -> anyhow::Result<(u16, String)> { assert_eq!(url, API); Ok((200, api_body("v0.2.0"))) };
    let a = check_with("0.1.0", &fetch).unwrap().expect("0.2.0 is newer");
    assert_eq!((a.version.as_str(), a.via), ("0.2.0", "api"));
    assert_eq!(a.notes.as_deref(), Some("- Shelf\n- Updates"));
    assert!(a.url.ends_with("/tag/v0.2.0"));
    assert!(check_with("0.2.0", &fetch).unwrap().is_none(), "up to date");
}

#[test]
fn rate_limited_api_falls_back_to_latest_json() {
    let fetch = |url: &str| -> anyhow::Result<(u16, String)> {
        match url {
            API => Ok((403, r#"{"message":"API rate limit exceeded for 1.2.3.4."}"#.into())),
            LATEST_JSON => Ok((200, r#"{"version":"0.2.0","notes":"Shelf","pub_date":"2026-11-13T00:00:00Z","platforms":{}}"#.into())),
            u => panic!("unexpected {u}"),
        }
    };
    let a = check_with("0.1.0", &fetch).unwrap().expect("found through latest.json");
    assert_eq!((a.version.as_str(), a.via, a.notes.as_deref()), ("0.2.0", "latest.json", Some("Shelf")));
    assert!(a.url.ends_with("/releases/tag/v0.2.0"));
    // offline: both fail, and that's an error to show quietly, not a crash
    let off = |_: &str| -> anyhow::Result<(u16, String)> { anyhow::bail!("offline") };
    assert!(check_with("0.1.0", &off).is_err());
}

#[test]
fn only_index_s_own_pages_are_opened() {
    use lo_core::update::is_release_page;
    assert!(is_release_page("https://github.com/YoruDev-Ryland/index-library/releases/tag/v0.2.0"));
    assert!(!is_release_page("https://github.com/YoruDev-Ryland/index-library.evil.com/x"));
    assert!(!is_release_page("http://github.com/YoruDev-Ryland/index-library/releases"));
    assert!(!is_release_page("file:///etc/passwd"));
}
