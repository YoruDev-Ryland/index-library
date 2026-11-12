//! Background images for systems in the sidebar: the console's photo from its Wikipedia
//! article (public API, no key), cached locally. A user-chosen image overrides it.
use super::http::Http;
use std::path::{Path, PathBuf};

pub fn wiki_title(system_id: &str) -> Option<&'static str> {
    Some(match system_id {
        "atari2600" => "Atari 2600", "atari5200" => "Atari 5200", "atari7800" => "Atari 7800", "lynx" => "Atari Lynx",
        "neogeo" => "Neo Geo (system)", "ngpc" => "Neo Geo Pocket Color", "n3ds" => "Nintendo 3DS", "nds" => "Nintendo DS",
        "gb" => "Game Boy", "gba" => "Game Boy Advance", "gbc" => "Game Boy Color", "gamecube" => "GameCube",
        "nes" => "Nintendo Entertainment System", "snes" => "Super Nintendo Entertainment System", "switch" => "Nintendo Switch",
        "virtualboy" => "Virtual Boy", "pc" => "Personal computer", "pce" => "TurboGrafx-16", "sega32x" => "32X",
        "gamegear" => "Game Gear", "genesis" => "Sega Genesis", "sms" => "Master System", "sg1000" => "SG-1000",
        "ps1" => "PlayStation (console)", "psp" => "PlayStation Portable", "psvita" => "PlayStation Vita",
        "vectrex" => "Vectrex", "gbahacks" => "Game Boy Advance",
        _ => return None,
    })
}

/// Lead image URLs from a MediaWiki `prop=pageimages` response, keyed by the titles as they
/// were requested (MediaWiki reports normalized/redirected titles separately).
pub fn parse_page_images(v: &serde_json::Value) -> std::collections::HashMap<String, String> {
    let q = &v["query"];
    let mut by_title: std::collections::HashMap<String, String> = q["pages"].as_object().map(|pages| pages.values()
        .filter_map(|p| Some((p["title"].as_str()?.to_string(), p["thumbnail"]["source"].as_str()?.to_string()))).collect()).unwrap_or_default();
    // walk normalized → redirects backwards so the original spelling also resolves
    for key in ["redirects", "normalized"] {
        for m in q[key].as_array().into_iter().flatten() {
            if let (Some(from), Some(to)) = (m["from"].as_str(), m["to"].as_str()) {
                if let Some(u) = by_title.get(to).cloned() { by_title.insert(from.to_string(), u); }
            }
        }
    }
    by_title
}

fn dir(cache_dir: &Path) -> PathBuf { cache_dir.join("art").join("systems") }

fn find(dir: &Path, stem: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path())
        .find(|p| p.file_stem().map(|s| s == stem).unwrap_or(false))
}

/// The image to show for a system, if one is available locally (custom first).
pub fn image_for(cache_dir: &Path, system_id: &str) -> Option<PathBuf> {
    let d = dir(cache_dir);
    find(&d, &format!("custom-{system_id}")).or_else(|| find(&d, system_id))
}

/// Use `src` as the system's image (copied into the cache so the app may display it), or
/// `None` to go back to the downloaded one.
pub fn set_custom(cache_dir: &Path, system_id: &str, src: Option<&Path>) -> anyhow::Result<()> {
    let d = dir(cache_dir);
    std::fs::create_dir_all(&d)?;
    while let Some(old) = find(&d, &format!("custom-{system_id}")) { std::fs::remove_file(old)?; }
    if let Some(src) = src {
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
        std::fs::copy(src, d.join(format!("custom-{system_id}.{ext}")))?;
    }
    Ok(())
}

/// Look up image URLs for many systems with one API request.
pub fn lookup_urls(http: &Http, system_ids: &[String]) -> anyhow::Result<Vec<(String, Option<String>)>> {
    let titles: Vec<&str> = system_ids.iter().filter_map(|id| wiki_title(id)).collect();
    if titles.is_empty() { return Ok(vec![]); }
    let joined: Vec<String> = titles.iter().map(|t| super::sgdb::urlencode(t)).collect();
    let url = format!("https://en.wikipedia.org/w/api.php?action=query&format=json&redirects=1&prop=pageimages&pithumbsize=640&pilimit=50&titles={}", joined.join("%7C"));
    let found = parse_page_images(&http.get_uncached_json(&url)?);
    Ok(system_ids.iter().map(|id| (id.clone(), wiki_title(id).and_then(|t| found.get(t).cloned()))).collect())
}

/// Download one system image, waiting and retrying when the image server asks us to slow down.
pub fn download(http: &Http, cache_dir: &Path, system_id: &str, url: &str) -> anyhow::Result<PathBuf> {
    let mut wait = std::time::Duration::from_secs(5);
    for attempt in 0..4 {
        match http.fetch_art(cache_dir, "systems", system_id, url) {
            Ok(p) => return Ok(p),
            Err(e) if attempt < 3 && format!("{e:#}").contains("429") => { std::thread::sleep(wait); wait *= 2; }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}
