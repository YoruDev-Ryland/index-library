//! RAWG (rawg.io): a free key; names, release dates, genres, wide artwork, and the consoles each
//! game came out on — so matches can be checked against the right system.
use super::{http::Http, similarity, sgdb::urlencode, ArtKind, ArtRef, GameQuery, MetaCandidate};
use crate::config::Settings;
use crate::db::Db;

pub fn configured(s: &Settings) -> bool { !s.keys.rawg.trim().is_empty() }

pub fn parse_search(v: &serde_json::Value, system: &crate::systems::SystemDef, title: &str) -> Vec<MetaCandidate> {
    v["results"].as_array().map(|a| a.iter().filter_map(|g| {
        let name = g["name"].as_str()?.to_string();
        let platforms: Vec<String> = g["platforms"].as_array().map(|p| p.iter().filter_map(|x| x["platform"]["name"].as_str().map(String::from)).collect()).unwrap_or_default();
        let art = g["background_image"].as_str().map(|u| vec![ArtRef { kind: ArtKind::Hero, url: u.to_string() }]).unwrap_or_default();
        Some(MetaCandidate {
            source: "rawg".into(), source_id: g["id"].as_i64()?.to_string(), score: similarity(title, &name),
            release: g["released"].as_str().map(String::from),
            genres: g["genres"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str().map(String::from)).collect()).unwrap_or_default(),
            console: super::consoles::verdict(system, &platforms), platforms, art, title: name, ..Default::default()
        })
    }).collect()).unwrap_or_default()
}

/// Every game RAWG lists for the title (any console); each carries its consoles and verdict.
pub fn search_any(db: &Db, http: &Http, s: &Settings, system: &crate::systems::SystemDef, title: &str) -> anyhow::Result<Vec<MetaCandidate>> {
    let key = s.keys.rawg.trim();
    let params = format!("search={}&page_size=12", urlencode(title));
    let url = format!("https://api.rawg.io/api/games?key={}&{params}", urlencode(key));
    let v = http.get_json(db, "rawg", &url, &format!("search:{params}"), &[])?;
    Ok(parse_search(&v, system, title))
}

/// Matches for this game: only those released on its console.
pub fn search(db: &Db, http: &Http, s: &Settings, q: &GameQuery) -> anyhow::Result<Vec<MetaCandidate>> {
    Ok(search_any(db, http, s, q.system, &q.title)?.into_iter().filter(|c| c.console.as_deref() == Some("match")).collect())
}

/// Description, developer and publisher for a chosen RAWG game.
pub fn details(db: &Db, http: &Http, s: &Settings, c: &mut MetaCandidate) -> anyhow::Result<()> {
    let url = format!("https://api.rawg.io/api/games/{}?key={}", c.source_id, urlencode(s.keys.rawg.trim()));
    let v = http.get_json(db, "rawg", &url, &format!("game:{}", c.source_id), &[])?;
    if c.description.is_none() { c.description = v["description_raw"].as_str().map(String::from).filter(|d| !d.is_empty()); }
    if c.developer.is_none() { c.developer = v["developers"][0]["name"].as_str().map(String::from); }
    if c.publisher.is_none() { c.publisher = v["publishers"][0]["name"].as_str().map(String::from); }
    Ok(())
}
