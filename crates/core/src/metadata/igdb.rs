//! IGDB (via Twitch client credentials): descriptions, dates, genres, companies, covers.
use super::{http::Http, similarity, ArtKind, ArtRef, GameQuery, MetaCandidate};
use crate::config::Settings;
use crate::db::Db;
use std::sync::Mutex;

static TOKEN: Mutex<Option<(String, i64)>> = Mutex::new(None);

pub fn configured(s: &Settings) -> bool { !s.keys.igdb_client_id.trim().is_empty() && !s.keys.igdb_client_secret.trim().is_empty() }

fn token(http: &Http, s: &Settings) -> anyhow::Result<String> {
    let mut t = TOKEN.lock().unwrap();
    if let Some((tok, exp)) = t.as_ref() { if *exp > crate::db::now() + 60 { return Ok(tok.clone()); } }
    let url = format!("https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials", s.keys.igdb_client_id.trim(), s.keys.igdb_client_secret.trim());
    let v = http.post_form_uncached(&url)?;
    let tok = v["access_token"].as_str().ok_or_else(|| anyhow::anyhow!("IGDB: no access token (check client id/secret)"))?.to_string();
    *t = Some((tok.clone(), crate::db::now() + v["expires_in"].as_i64().unwrap_or(3600)));
    Ok(tok)
}

pub fn parse_games(v: &serde_json::Value, system: &crate::systems::SystemDef, title: &str) -> Vec<MetaCandidate> {
    v.as_array().map(|a| a.iter().filter_map(|g| {
        let name = g["name"].as_str()?.to_string();
        let aliases: Vec<String> = g["alternative_names"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str().map(String::from)).filter(|a| *a != name).collect()).unwrap_or_default();
        let companies = g["involved_companies"].as_array();
        let company = |dev: bool| companies.and_then(|c| c.iter().find(|x| x["developer"].as_bool() == Some(dev)).and_then(|x| x["company"]["name"].as_str().map(String::from)));
        Some(MetaCandidate {
            source: "igdb".into(), source_id: g["id"].as_i64()?.to_string(),
            score: std::iter::once(&name).chain(&aliases).map(|n| similarity(title, n)).fold(0.0, f32::max),
            description: g["summary"].as_str().map(String::from),
            release: g["first_release_date"].as_i64().and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|d| d.format("%Y-%m-%d").to_string()),
            genres: g["genres"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str().map(String::from)).collect()).unwrap_or_default(),
            developer: company(true), publisher: company(false),
            art: g["cover"]["image_id"].as_str().map(|id| vec![ArtRef { kind: ArtKind::Cover, url: format!("https://images.igdb.com/igdb/image/upload/t_cover_big/{id}.jpg") }]).unwrap_or_default(),
            platforms: g["platforms"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str().map(String::from)).collect()).unwrap_or_default(),
            console: None, chosen: false, aliases,
            title: name,
        }).map(|mut c| { c.console = super::consoles::verdict(system, &c.platforms); c })
    }).collect()).unwrap_or_default()
}

pub fn search(db: &Db, http: &Http, s: &Settings, q: &GameQuery) -> anyhow::Result<Vec<MetaCandidate>> {
    let tok = token(http, s)?;
    let esc = q.title.replace('\\', "").replace('"', "");
    let platform = q.system.igdb_platform.map(|p| format!(" where platforms = ({p});")).unwrap_or_default();
    let body = format!("search \"{esc}\"; fields name,alternative_names.name,summary,first_release_date,genres.name,involved_companies.company.name,involved_companies.developer,cover.image_id,platforms.name;{platform} limit 8;");
    let v = http.post_json(db, "igdb", "https://api.igdb.com/v4/games", &body, &[("Client-ID", s.keys.igdb_client_id.trim().to_string()), ("Authorization", format!("Bearer {tok}"))])?;
    Ok(parse_games(&v, q.system, &q.title))
}

const FULL: &str = "name,alternative_names.name,summary,first_release_date,genres.name,involved_companies.company.name,involved_companies.developer,cover.image_id,platforms.name";

fn headers(http: &Http, s: &Settings) -> anyhow::Result<Vec<(&'static str, String)>> {
    Ok(vec![("Client-ID", s.keys.igdb_client_id.trim().to_string()), ("Authorization", format!("Bearer {}", token(http, s)?))])
}

/// The full listings of these games (from the console's catalog), scored against `title`.
pub fn by_ids(db: &Db, http: &Http, s: &Settings, system: &crate::systems::SystemDef, ids: &[String], title: &str) -> anyhow::Result<Vec<MetaCandidate>> {
    if ids.is_empty() { return Ok(vec![]); }
    let body = format!("fields {FULL}; where id = ({}); limit {};", ids.join(","), ids.len());
    let v = http.post_json(db, "igdb", "https://api.igdb.com/v4/games", &body, &headers(http, s)?)?;
    Ok(parse_games(&v, system, title))
}

/// One entry of a console's catalog: id, name, other names, year of first release.
pub struct CatalogEntry { pub id: i64, pub name: String, pub aliases: Vec<String>, pub year: Option<i32> }

/// Every game IGDB lists for a console, 500 at a time (`progress` gets the count so far).
pub fn catalog(http: &Http, s: &Settings, platform: u32, progress: &dyn Fn(usize) -> bool) -> anyhow::Result<Vec<CatalogEntry>> {
    let h = headers(http, s)?;
    let mut out: Vec<CatalogEntry> = Vec::new();
    let mut last = 0i64;
    loop {
        let body = format!("fields name,alternative_names.name,first_release_date; where platforms = ({platform}) & id > {last}; sort id asc; limit 500;");
        let v = http.post_json_uncached("https://api.igdb.com/v4/games", &body, &h)?;
        let page = v.as_array().cloned().unwrap_or_default();
        for g in &page {
            let (Some(id), Some(name)) = (g["id"].as_i64(), g["name"].as_str()) else { continue };
            let aliases = g["alternative_names"].as_array().map(|x| x.iter().filter_map(|y| y["name"].as_str().map(String::from)).filter(|a| a != name).collect()).unwrap_or_default();
            let year = g["first_release_date"].as_i64().and_then(|t| chrono::DateTime::from_timestamp(t, 0)).map(|d| d.format("%Y").to_string().parse().unwrap_or(0));
            out.push(CatalogEntry { id, name: name.to_string(), aliases, year });
            last = last.max(id);
        }
        if !progress(out.len()) { anyhow::bail!("cancelled"); }
        if page.len() < 500 { break; }
    }
    Ok(out)
}

/// Every game IGDB lists for the title, on any console (to check which consoles a match is for).
pub fn search_any(db: &Db, http: &Http, s: &Settings, system: &crate::systems::SystemDef, title: &str) -> anyhow::Result<Vec<MetaCandidate>> {
    let tok = token(http, s)?;
    let esc = title.replace('\\', "").replace('"', "");
    let body = format!("search \"{esc}\"; fields name,alternative_names.name,platforms.name; limit 10;");
    let v = http.post_json(db, "igdb", "https://api.igdb.com/v4/games", &body, &[("Client-ID", s.keys.igdb_client_id.trim().to_string()), ("Authorization", format!("Bearer {tok}"))])?;
    Ok(parse_games(&v, system, title))
}
