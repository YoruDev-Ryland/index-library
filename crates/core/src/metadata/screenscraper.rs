//! ScreenScraper: exact matches by ROM hash, console box art and screenshots.
use super::{http::Http, sgdb::urlencode, ArtKind, ArtRef, GameQuery, MetaCandidate};
use crate::config::Settings;
use crate::db::Db;

pub fn configured(s: &Settings) -> bool {
    let k = &s.keys;
    !k.screenscraper_devid.trim().is_empty() && !k.screenscraper_devpassword.trim().is_empty()
}

fn pick<'a>(arr: &'a serde_json::Value, key: &str, prefs: &[&str]) -> Option<&'a str> {
    let a = arr.as_array()?;
    for p in prefs { if let Some(x) = a.iter().find(|x| x[key].as_str() == Some(p)) { return x["text"].as_str(); } }
    a.first().and_then(|x| x["text"].as_str())
}

pub fn parse_jeu(v: &serde_json::Value) -> Option<MetaCandidate> {
    let j = &v["response"]["jeu"];
    if j.is_null() { return None; }
    let regions = ["us", "wor", "eu", "ss", "jp"];
    let mut art = Vec::new();
    if let Some(m) = j["medias"].as_array() {
        for (t, kind) in [("box-2D", ArtKind::Cover), ("ss", ArtKind::Screenshot), ("wheel", ArtKind::Logo), ("fanart", ArtKind::Hero)] {
            let found = regions.iter().find_map(|r| m.iter().find(|x| x["type"].as_str() == Some(t) && x["region"].as_str() == Some(r)))
                .or_else(|| m.iter().find(|x| x["type"].as_str() == Some(t)));
            if let Some(url) = found.and_then(|x| x["url"].as_str()) { art.push(ArtRef { kind, url: url.to_string() }); }
        }
    }
    Some(MetaCandidate {
        source: "screenscraper".into(), source_id: j["id"].as_str().map(String::from).or_else(|| j["id"].as_i64().map(|i| i.to_string()))?,
        title: pick(&j["noms"], "region", &regions)?.to_string(), score: 1.0,
        description: pick(&j["synopsis"], "langue", &["en"]).map(String::from),
        release: pick(&j["dates"], "region", &regions).map(String::from),
        genres: j["genres"].as_array().map(|g| g.iter().filter_map(|x| pick(&x["noms"], "langue", &["en"]).map(String::from)).collect()).unwrap_or_default(),
        developer: j["developpeur"]["text"].as_str().map(String::from), publisher: j["editeur"]["text"].as_str().map(String::from),
        art,
        // ScreenScraper searches within the console it's asked about
        platforms: j["systeme"]["text"].as_str().map(|t| vec![t.to_string()]).unwrap_or_default(),
        console: Some("match".into()), chosen: false, aliases: vec![],
    })
}

/// Query string (without credentials). Hashes when known; ScreenScraper also matches by file name + size.
pub fn params(q: &GameQuery) -> String {
    let mut params = format!("softname=index-library&output=json&romtype=rom&systemeid={}", q.system.ss_system.unwrap_or(0));
    if let Some(h) = &q.sha1 { params.push_str(&format!("&sha1={h}")); }
    if let Some(c) = &q.crc32 { params.push_str(&format!("&crc={c}")); }
    if let Some(sz) = q.size { params.push_str(&format!("&romtaille={sz}")); }
    if let Some(n) = &q.file_name { params.push_str(&format!("&romnom={}", urlencode(n))); }
    params
}

pub fn lookup(db: &Db, http: &Http, s: &Settings, q: &GameQuery) -> anyhow::Result<Option<MetaCandidate>> {
    let k = &s.keys;
    let params = params(q);
    let auth = format!("devid={}&devpassword={}&ssid={}&sspassword={}", urlencode(k.screenscraper_devid.trim()), urlencode(k.screenscraper_devpassword.trim()), urlencode(k.screenscraper_user.trim()), urlencode(k.screenscraper_password.trim()));
    let url = format!("https://api.screenscraper.fr/api2/jeuInfos.php?{auth}&{params}");
    let v = http.get_json(db, "screenscraper", &url, &params, &[])?;
    let hashed = q.sha1.is_some() || q.crc32.is_some();
    // a hash match is exact; a file-name match is only as good as the titles agree
    Ok(parse_jeu(&v).map(|mut c| { if !hashed { c.score = super::similarity(&c.title, &q.title).max(0.85); } c }))
}

/// Name search (jeuRecherche): every game ScreenScraper lists for the title on this system.
pub fn search(db: &Db, http: &Http, s: &Settings, q: &GameQuery) -> anyhow::Result<Vec<MetaCandidate>> {
    let k = &s.keys;
    let params = format!("softname=index-library&output=json&systemeid={}&recherche={}", q.system.ss_system.unwrap_or(0), urlencode(&q.title));
    let auth = format!("devid={}&devpassword={}&ssid={}&sspassword={}", urlencode(k.screenscraper_devid.trim()), urlencode(k.screenscraper_devpassword.trim()), urlencode(k.screenscraper_user.trim()), urlencode(k.screenscraper_password.trim()));
    let url = format!("https://api.screenscraper.fr/api2/jeuRecherche.php?{auth}&{params}");
    let v = http.get_json(db, "screenscraper", &url, &format!("search:{params}"), &[])?;
    let games = v["response"]["jeux"].as_array().cloned().unwrap_or_default();
    Ok(games.iter().filter_map(|j| parse_jeu(&serde_json::json!({"response": {"jeu": j}}))).map(|mut c| { c.score = super::similarity(&c.title, &q.title); c }).collect())
}
