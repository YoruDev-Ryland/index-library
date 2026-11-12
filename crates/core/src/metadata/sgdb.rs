//! SteamGridDB: artwork (covers, heroes, logos, icons). API v2, bearer key.
use super::{http::Http, similarity, ArtKind, ArtRef, MetaCandidate};
use crate::config::Settings;
use crate::db::Db;

const BASE: &str = "https://www.steamgriddb.com/api/v2";

pub fn configured(s: &Settings) -> bool { !s.keys.steamgriddb.trim().is_empty() }

fn auth(s: &Settings) -> Vec<(&'static str, String)> { vec![("Authorization", format!("Bearer {}", s.keys.steamgriddb.trim()))] }

pub fn parse_search(v: &serde_json::Value, title: &str) -> Vec<MetaCandidate> {
    v["data"].as_array().map(|a| a.iter().filter_map(|g| {
        let name = g["name"].as_str()?.to_string();
        Some(MetaCandidate { source: "steamgriddb".into(), source_id: g["id"].as_i64()?.to_string(), score: similarity(title, &name) * 0.98, title: name, ..Default::default() })
    }).collect()).unwrap_or_default()
}

pub fn parse_art(v: &serde_json::Value, kind: ArtKind) -> Vec<ArtRef> {
    v["data"].as_array().map(|a| a.iter().filter_map(|x| Some(ArtRef { kind, url: x["url"].as_str()?.to_string() })).collect()).unwrap_or_default()
}

pub fn search(db: &Db, http: &Http, s: &Settings, title: &str) -> anyhow::Result<Vec<MetaCandidate>> {
    let url = format!("{BASE}/search/autocomplete/{}", urlencode(title));
    Ok(parse_search(&http.get_json(db, "steamgriddb", &url, &url, &auth(s))?, title))
}

pub fn art(db: &Db, http: &Http, s: &Settings, game_id: &str, kind: ArtKind) -> anyhow::Result<Vec<ArtRef>> {
    let path = match kind { ArtKind::Cover => "grids/game/{}?dimensions=600x900,342x482,660x930", ArtKind::Hero => "heroes/game/{}", ArtKind::Logo => "logos/game/{}", ArtKind::Icon => "icons/game/{}", _ => return Ok(vec![]) };
    let url = format!("{BASE}/{}", path.replace("{}", game_id));
    Ok(parse_art(&http.get_json(db, "steamgriddb", &url, &url, &auth(s))?, kind))
}

pub fn urlencode(s: &str) -> String {
    s.bytes().map(|b| match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(), _ => format!("%{b:02X}") }).collect()
}

/// First portrait grid thumbnail for a SteamGridDB game id (small; for previews).
pub fn thumb(db: &Db, http: &Http, s: &Settings, game_id: &str) -> anyhow::Result<Option<String>> {
    let url = format!("{BASE}/grids/game/{game_id}?dimensions=600x900,342x482,660x930");
    let v = http.get_json(db, "steamgriddb", &url, &url, &auth(s))?;
    Ok(v["data"].as_array().and_then(|a| a.iter().find_map(|x| x["thumb"].as_str().or(x["url"].as_str()).map(String::from))))
}
