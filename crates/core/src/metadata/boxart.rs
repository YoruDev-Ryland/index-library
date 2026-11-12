//! The box scan of the exact release a file was identified as: libretro's thumbnails, named by
//! the No-Intro / Redump name ("Super Mario World (USA)"). Grid covers use it when there is one;
//! banners and heroes still come from SteamGridDB.
use super::http::Http;
use crate::db::Db;
use rusqlite::OptionalExtension;
use std::path::{Path, PathBuf};

/// A miss is asked about again after this long (libretro adds scans over time).
const MISS_SECS: u64 = 30 * 86_400;
/// Scans are stored no taller than this: plenty for the grid and the game drawer.
const MAX_HEIGHT: u32 = 800;

/// libretro's thumbnail folder for a console: the name of its No-Intro / Redump list.
pub fn folder(system_id: &str) -> Option<&'static str> {
    crate::dat::libretro_dat(system_id).and_then(|r| r.split_once('/').map(|(_, n)| n))
}

/// libretro's file name for a release: these characters become "_".
pub fn file_name(release: &str) -> String {
    release.chars().map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c }).collect()
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(), _ => format!("%{b:02X}") }).collect()
}

pub fn url(system_id: &str, release: &str) -> Option<String> {
    Some(format!("https://thumbnails.libretro.com/{}/Named_Boxarts/{}.png", enc(folder(system_id)?), enc(&file_name(release))))
}

fn stem(cache_dir: &Path, system_id: &str, release: &str) -> PathBuf {
    cache_dir.join("art").join("libretro").join(system_id).join(file_name(release))
}

/// The release a game's file was identified as: (console, list name), e.g. ("snes", "Super
/// Mario World (USA)"). None when no file of the game is verified.
pub fn release_of(db: &Db, game_id: i64) -> anyhow::Result<Option<(String, String)>> {
    Ok(db.conn().query_row(
        "SELECT system_id, dat_game FROM files WHERE game_id=?1 AND dat_status='verified' AND dat_game IS NOT NULL ORDER BY size DESC LIMIT 1",
        [game_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?
        // a game you own physically: the release your copy is
        .or(match crate::shelf::list_release(db, game_id)? {
            Some(r) => db.conn().query_row("SELECT system_id FROM games WHERE id=?1", [game_id], |x| x.get::<_, String>(0)).optional()?.map(|s| (s, r)),
            None => None,
        }))
}

/// The scan already on disk for a release, if any (no network).
pub fn cached(cache_dir: &Path, system_id: &str, release: &str) -> Option<PathBuf> {
    let s = stem(cache_dir, system_id, release);
    ["jpg", "png"].iter().map(|e| s.with_extension(e)).find(|p| p.exists())
}

/// A miss remembered recently: don't ask again yet.
fn missed(cache_dir: &Path, system_id: &str, release: &str) -> bool {
    std::fs::metadata(stem(cache_dir, system_id, release).with_extension("none")).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < MISS_SECS)
}

/// The names libretro may have the scan under: the release, then without a disc number (one scan
/// for every disc of a game).
fn names(release: &str) -> Vec<String> {
    let mut v = vec![release.to_string()];
    let re = regex::Regex::new(r"\s*\(Disc \d+\)").unwrap();
    let no_disc = re.replace_all(release, "").to_string();
    if no_disc != release { v.push(no_disc); }
    v
}

/// Get the scan for a release: from disk, or from libretro (stored smaller). None when libretro
/// has none.
pub fn fetch(http: &Http, cache_dir: &Path, system_id: &str, release: &str) -> anyhow::Result<Option<PathBuf>> {
    if let Some(p) = cached(cache_dir, system_id, release) { return Ok(Some(p)); }
    if missed(cache_dir, system_id, release) { return Ok(None); }
    let s = stem(cache_dir, system_id, release);
    if let Some(d) = s.parent() { std::fs::create_dir_all(d)?; }
    for name in names(release) {
        let Some(u) = url(system_id, &name) else { return Ok(None) };
        if let Some(bytes) = http.get_bytes(&u)? {
            return Ok(Some(store(&bytes, &s)?));
        }
    }
    std::fs::write(s.with_extension("none"), b"")?;
    Ok(None)
}

/// Save a scan no taller than MAX_HEIGHT: as JPEG, or PNG when much of it is see-through.
pub fn store(bytes: &[u8], stem: &Path) -> anyhow::Result<PathBuf> {
    let (data, ext) = encode(bytes, MAX_HEIGHT)?;
    let (path, tmp) = (stem.with_extension(ext), stem.with_extension(format!("{ext}.part")));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// An image no taller than `max_height`: JPEG (q85), or PNG when much of it is see-through.
/// Returns the bytes and their extension.
pub fn encode(bytes: &[u8], max_height: u32) -> anyhow::Result<(Vec<u8>, &'static str)> {
    let img = image::load_from_memory(bytes)?;
    let img = if img.height() > max_height { img.resize(u32::MAX, max_height, image::imageops::FilterType::Triangle) } else { img };
    // a few see-through pixels at the corners don't need a PNG; a cut-out shape does
    let clear = img.color().has_alpha() && { let px = img.to_rgba8(); px.pixels().filter(|p| p[3] < 250).count() * 50 > px.pixels().len() };
    let mut out = std::io::Cursor::new(Vec::new());
    if clear { img.write_to(&mut out, image::ImageFormat::Png)?; Ok((out.into_inner(), "png")) }
    else { image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img.to_rgb8())?; Ok((out.into_inner(), "jpg")) }
}

/// Make a scan a game's cover, keeping its other art.
pub fn set_cover(db: &Db, game_id: i64, path: &Path) -> anyhow::Result<()> {
    db.conn().execute("UPDATE games SET art_json=json_set(COALESCE(art_json,'{}'), '$.cover', ?1) WHERE id=?2", (crate::util::p2s(path), game_id))?;
    Ok(())
}

/// Is this path one of the stored scans?
pub fn is_scan(cache_dir: &Path, path: &str) -> bool { Path::new(path).starts_with(cache_dir.join("art").join("libretro")) }
