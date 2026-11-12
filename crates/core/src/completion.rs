//! How complete a console's collection is: every game in its No-Intro / Redump list once, its
//! regional releases and re-releases together ("one game, one ROM"), against what's here.
//!
//! Releases are grouped by title without tags. Retool's clone lists
//! (github.com/unexpectedpanda/retool-clonelists-metadata) join titles that are one game under
//! different names, such as "Super Mario - Yoshi Island" and "Super Mario World 2 - Yoshi's Island".
use crate::db::Db;
use crate::parse::{norm_title, parse};
use crate::systems::ParserKind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A clone list is read again after this long.
pub const REFRESH_SECS: u64 = 30 * 86_400;
const CLONELISTS: &str = "https://raw.githubusercontent.com/unexpectedpanda/retool-clonelists-metadata/main/clonelists/";

/// Retool's clone list for a console's list.
pub fn clonelist_file(system_id: &str) -> Option<&'static str> {
    Some(match system_id {
        "atari2600" => "Atari - Atari 2600 (No-Intro).json", "atari5200" => "Atari - Atari 5200 (No-Intro).json",
        "atari7800" => "Atari - Atari 7800 (No-Intro).json", "lynx" => "Atari - Atari Lynx (No-Intro).json",
        "ngpc" => "SNK - NeoGeo Pocket Color (No-Intro).json", "n3ds" => "Nintendo - Nintendo 3DS (No-Intro).json",
        "nds" => "Nintendo - Nintendo DS (No-Intro).json", "gb" => "Nintendo - Game Boy (No-Intro).json",
        "gba" | "gbahacks" => "Nintendo - Game Boy Advance (No-Intro).json", "gbc" => "Nintendo - Game Boy Color (No-Intro).json",
        "gamecube" => "Nintendo - GameCube (Redump).json", "nes" => "Nintendo - Nintendo Entertainment System (No-Intro).json",
        "snes" => "Nintendo - Super Nintendo Entertainment System (No-Intro).json", "virtualboy" => "Nintendo - Virtual Boy (No-Intro).json",
        "pce" => "NEC - PC Engine - TurboGrafx-16 (No-Intro).json", "sega32x" => "Sega - 32X (No-Intro).json",
        "gamegear" => "Sega - Game Gear (No-Intro).json", "genesis" => "Sega - Mega Drive - Genesis (No-Intro).json",
        "sms" => "Sega - Master System - Mark III (No-Intro).json", "sg1000" => "Sega - SG-1000 (No-Intro).json",
        "ps1" => "Sony - PlayStation (Redump).json", "psp" => "Sony - PlayStation Portable (Redump).json",
        "psvita" => "Unofficial - Sony - PlayStation Vita (No-Intro).json", "vectrex" => "GCE - Vectrex (No-Intro).json",
        _ => return None,
    })
}

/// A title in the form titles are compared in: `norm_title`, with articles left out wherever
/// the list's naming puts them ("7th Saga, The" and "The 7th Saga" match).
pub fn title_key(s: &str) -> String {
    static ARTICLES: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(?i)(^|,\s*|\s-\s)(the|a|an)(\s|$)").unwrap());
    let mut t = s.to_string();
    // repeat: each pass can uncover another ("The Legend of Zelda - A Link …")
    loop {
        let n = ARTICLES.replace_all(&t, |c: &regex::Captures| if c[1].contains('-') { " - ".to_string() } else { " ".to_string() }).to_string();
        if n == t { break; }
        t = n;
    }
    norm_title(&t)
}

/// Titles that are one game: each name (in comparison form) → the game's name.
#[derive(Debug, Clone, Default)]
pub struct CloneList { names: HashMap<String, String> }

impl CloneList {
    pub fn parse(json: &str) -> anyhow::Result<CloneList> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let mut names = HashMap::new();
        for g in v["variants"].as_array().into_iter().flatten() {
            let Some(group) = g["group"].as_str() else { continue };
            names.insert(title_key(group), group.to_string());
            for t in g["titles"].as_array().into_iter().flatten() {
                if let Some(s) = t["searchTerm"].as_str() { names.entry(title_key(s)).or_insert_with(|| group.to_string()); }
            }
        }
        Ok(CloneList { names })
    }
    /// The game a title belongs to, by its title alone or with its other tags.
    pub fn group_of(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|n| self.names.get(&title_key(n))).map(String::as_str)
    }
}

/// The clone list for a console, from the cache or downloaded (None: Retool has none, or it
/// couldn't be had and none is cached).
pub fn clonelist(http: &crate::metadata::http::Http, cache_dir: &Path, system_id: &str) -> Option<CloneList> {
    let file = clonelist_file(system_id)?;
    let path: PathBuf = cache_dir.join("clonelists").join(file);
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() < REFRESH_SECS);
    if !fresh {
        let url = format!("{CLONELISTS}{}", file.replace(' ', "%20").replace('&', "%26"));
        if let Ok(Some(bytes)) = http.get_bytes(&url) {
            if CloneList::parse(&String::from_utf8_lossy(&bytes)).is_ok() {
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let _ = std::fs::write(&path, &bytes);
            }
        }
    }
    CloneList::parse(&std::fs::read_to_string(&path).ok()?).ok()
}

/// NoPayStation's PS Vita list, downloaded (monthly) into the title database, for measuring the
/// Vita. Quietly does nothing when it can't be had; a cached copy is used.
pub fn ensure_psv(http: &crate::metadata::http::Http, db: &Db, cache_dir: &Path) {
    let path = cache_dir.join("titledb").join("PSV_GAMES.tsv");
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() < REFRESH_SECS);
    let have: bool = db.conn().query_row("SELECT COUNT(*)>0 FROM titledb WHERE source='psv'", [], |r| r.get(0)).unwrap_or(false);
    if fresh && have { return; }
    if !fresh {
        if let Ok(Some(bytes)) = http.get_bytes("https://nopaystation.com/tsv/PSV_GAMES.tsv") {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, &bytes);
        } else if have { return; }
    }
    if let Ok(t) = std::fs::read(&path) { let _ = crate::metadata::offline::psv_tsv_import(db, &String::from_utf8_lossy(&t)); }
}

/// What counts towards a complete collection.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// only games released in one of these regions ("World" releases count for every region);
    /// empty: every region
    pub regions: Vec<String>,
    /// unlicensed games ("Unl")
    pub unlicensed: bool,
    /// aftermarket and homebrew releases
    pub aftermarket: bool,
    /// what counts as having a game: "digital" (files, the default), "physical" (copies on the
    /// shelf) or "either"
    pub source: String,
    /// only games with a release in one of these languages ("En"); a release without language
    /// tags is in its region's language. Empty: any language
    pub languages: Vec<String>,
}

/// The language a release from this region is in when the list names none.
fn region_language(region: &str) -> Option<&'static str> {
    Some(match region {
        "USA" | "World" | "Europe" | "UK" | "Australia" | "Canada" | "New Zealand" | "Ireland" => "En",
        "Japan" => "Ja", "Korea" => "Ko", "China" | "Taiwan" | "Hong Kong" => "Zh", "France" => "Fr", "Germany" => "De",
        "Spain" | "Mexico" | "Argentina" => "Es", "Italy" => "It", "Brazil" | "Portugal" => "Pt", "Netherlands" => "Nl",
        "Sweden" => "Sv", "Russia" => "Ru", "Denmark" => "Da", "Norway" => "No", "Finland" => "Fi", "Poland" => "Pl",
        _ => return None,
    })
}

/// NoPayStation's region codes as No-Intro names them.
fn nps_region(r: &str) -> &str { match r { "US" => "USA", "EU" => "Europe", "JP" => "Japan", "ASIA" => "Asia", "INT" => "World", x => x } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// a file here is verified as one of its releases
    Have,
    /// a game here has its name, but no file is verified as one of its releases
    Unverified,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub title: String,
    /// the release to look for: the preferred region's
    pub release: String,
    /// every release of the game the list has
    pub releases: Vec<String>,
    pub regions: Vec<String>,
    pub state: State,
    /// the game in this library
    pub game_id: Option<i64>,
    /// your physical copies of it
    pub shelf: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Completion {
    pub system: String,
    /// the list it's measured against ("Nintendo - Super Nintendo Entertainment System 2026…")
    pub list: Option<String>,
    /// Retool's clone list joined differently named releases
    pub clone_list: bool,
    pub total: usize,
    pub have: usize,
    pub unverified: usize,
    pub missing: usize,
    pub entries: Vec<Entry>,
}

/// Tags of releases that aren't games to collect.
const NOT_GAMES: &[&str] = &["beta", "proto", "prototype", "demo", "sample", "kiosk", "pirate", "program", "test program", "debug", "alpha", "bios", "enhancement chip", "tech demo", "sdk", "promo", "preview", "trial", "taikenban"];

fn counts(tags: &[String], o: &Options) -> bool {
    // a tag names a kind when its first word does ("Beta 2", "Proto 1", "Prototype")
    let has = |words: &[&str]| tags.iter().any(|t| {
        let t = t.to_lowercase();
        let first = t.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        words.iter().any(|w| t == *w || first == *w || t.starts_with(&format!("{w} ")))
    });
    !has(NOT_GAMES) && (o.unlicensed || !has(&["unl", "unlicensed"])) && (o.aftermarket || !has(&["aftermarket", "homebrew"]))
}

struct Release { name: String, title: String, regions: Vec<String>, tags: usize, rev: Option<String>, id: Option<String>, in_scope: bool }

/// The releases to measure against: the console's No-Intro / Redump list, or for the PS Vita
/// NoPayStation's list of digital games (most Vita games never had a cartridge dump), with their
/// title ids. Returns (list name, releases as (name, title id)).
fn releases(db: &Db, system_id: &str) -> anyhow::Result<(Option<String>, Vec<(String, Option<String>)>)> {
    let c = db.conn();
    if system_id == "psvita" {
        let mut q = c.prepare("SELECT id, name, COALESCE(region,'') FROM titledb WHERE source='psv'")?;
        let v: Vec<(String, Option<String>)> = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .filter_map(|r| r.ok()).map(|(id, name, region)| (format!("{name} ({})", nps_region(&region)), Some(id))).collect();
        if !v.is_empty() { return Ok((Some("NoPayStation's PS Vita list".into()), v)); }
    }
    let list: Option<String> = c.query_row("SELECT trim(COALESCE(name,'') || ' ' || COALESCE(version,'')) FROM dats WHERE system_id=?1 ORDER BY id DESC LIMIT 1", [system_id], |r| r.get(0)).ok();
    let mut q = c.prepare("SELECT DISTINCT r.game_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1")?;
    let names: Vec<(String, Option<String>)> = q.query_map([system_id], |r| r.get(0))?.collect::<Result<Vec<String>, _>>()?.into_iter().map(|n| (n, None)).collect();
    Ok((list, names))
}

pub fn completion(db: &Db, system_id: &str, region_order: &[String], clones: Option<&CloneList>, o: &Options) -> anyhow::Result<Completion> {
    let c = db.conn();
    let (list, names) = releases(db, system_id)?;

    // every counted release, by game
    let key_of = |title: &str, with_tags: &str| -> (String, Option<String>) {
        match clones.and_then(|cl| cl.group_of(&[title, with_tags])) { Some(g) => (title_key(g), Some(g.to_string())), None => (title_key(title), None) }
    };
    let mut games: BTreeMap<String, (Option<String>, Vec<Release>)> = BTreeMap::new();
    for (name, id) in names {
        let p = parse(&[ParserKind::NoIntro], &name);
        if !counts(&p.tags, o) { continue; }
        let english = |l: &str| p.langs.iter().any(|x| x.eq_ignore_ascii_case(l)) || (p.langs.is_empty() && p.regions.iter().any(|r| region_language(r) == Some(if l.eq_ignore_ascii_case("en") { "En" } else { l })));
        // out-of-scope releases still make the game had; only in-scope ones make it count
        let lang_ok = o.languages.is_empty() || o.languages.iter().any(|l| english(l));
        let in_scope = lang_ok && (o.regions.is_empty() || p.regions.iter().any(|r| r == "World" || o.regions.iter().any(|x| x.eq_ignore_ascii_case(r))));
        let with_tags = format!("{} {}", p.title, p.tags.join(" "));
        let (key, group) = key_of(&p.title, &with_tags);
        let title = p.display_title.clone().unwrap_or_else(|| p.title.clone());
        let e = games.entry(key).or_insert((group, vec![]));
        e.1.push(Release { name, title, regions: p.regions, tags: p.tags.len(), rev: p.rev, id, in_scope });
    }

    // what's here: verified releases, and games known only by name
    let mut verified: HashMap<String, i64> = HashMap::new();
    let mut q = c.prepare("SELECT dat_game, game_id FROM files WHERE system_id=?1 AND dat_status='verified' AND dat_game IS NOT NULL AND game_id IS NOT NULL")?;
    for r in q.query_map([system_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? { let (n, g) = r?; verified.insert(n, g); }
    // games here by title id (the Vita's digital list)
    let mut by_id: HashMap<String, i64> = HashMap::new();
    let mut q = c.prepare("SELECT id, substr(group_key, 5) FROM games g WHERE system_id=?1 AND group_key LIKE 'tid:%' AND EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id)")?;
    for r in q.query_map([system_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? { let (g, t) = r?; by_id.insert(t.to_uppercase(), g); }
    let mut by_name: HashMap<String, i64> = HashMap::new();
    let mut q = c.prepare("SELECT g.id, g.title FROM games g WHERE g.system_id=?1 AND EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id) AND NOT EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id AND f.dat_status='verified')")?;
    for r in q.query_map([system_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))? {
        let (id, title) = r?;
        let t = parse(&[ParserKind::NoIntro], &title);
        by_name.entry(key_of(&t.title, &title).0).or_insert(id);
    }

    // copies on the shelf: by release, and the game they belong to
    let mut q = c.prepare("SELECT c.release, g.id FROM shelf_copies c LEFT JOIN games g ON c.ref = g.system_id || ':' || g.group_key WHERE c.system_id=?1 AND c.deleted=0")?;
    let copies: Vec<(String, Option<i64>)> = q.query_map([system_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let source = o.source.as_str();
    let rank = |r: &Release| r.regions.iter().filter_map(|x| region_order.iter().position(|o| o == x)).min().unwrap_or(region_order.len());
    let mut entries: Vec<Entry> = games.into_iter().filter(|(_, (_, rels))| rels.iter().any(|r| r.in_scope)).map(|(key, (group, mut rels))| {
        rels.sort_by(|a, b| b.in_scope.cmp(&a.in_scope).then(rank(a).cmp(&rank(b))).then(a.tags.cmp(&b.tags)).then(b.rev.cmp(&a.rev)).then(a.name.cmp(&b.name)));
        let mut regions: Vec<String> = rels.iter().flat_map(|r| r.regions.clone()).collect();
        regions.sort_by_key(|x| region_order.iter().position(|o| o == x).unwrap_or(usize::MAX));
        regions.dedup();
        let have = rels.iter().find_map(|r| verified.get(&r.name).copied().or_else(|| r.id.as_ref().and_then(|i| by_id.get(&i.to_uppercase()).copied())));
        let (digital, game_id) = match (have, by_name.get(&key)) {
            (Some(g), _) => (State::Have, Some(g)),
            (None, Some(g)) => (State::Unverified, Some(*g)),
            _ => (State::Missing, None),
        };
        let mine: Vec<&(String, Option<i64>)> = copies.iter().filter(|(rel, g)| rels.iter().any(|r| &r.name == rel) || (g.is_some() && *g == game_id)).collect();
        let shelf = mine.len();
        let shelf_game = mine.iter().find_map(|(_, g)| *g);
        let (state, game_id) = match source {
            "physical" => if shelf > 0 { (State::Have, shelf_game.or(game_id)) } else { (State::Missing, game_id) },
            "either" if shelf > 0 => (State::Have, game_id.or(shelf_game)),
            _ => (digital, game_id),
        };
        Entry { title: group.unwrap_or_else(|| rels[0].title.clone()), release: rels[0].name.clone(), releases: rels.into_iter().map(|r| r.name).collect(), regions, state, game_id, shelf }
    }).collect();
    entries.sort_by(|a, b| title_key(&a.title).cmp(&title_key(&b.title)).then(a.title.cmp(&b.title)));
    let n = |s: State| entries.iter().filter(|e| e.state == s).count();
    Ok(Completion { system: system_id.to_string(), list, clone_list: clones.is_some(), total: entries.len(), have: n(State::Have), unverified: n(State::Unverified), missing: n(State::Missing), entries })
}

/// One console's completion, for the overview.
#[derive(Debug, Clone, Serialize)]
pub struct Summary { pub system: String, pub name: String, pub total: usize, pub have: usize, pub unverified: usize }
