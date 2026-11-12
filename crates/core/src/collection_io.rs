//! Collections as files: a small JSON format you can export, share, or have an AI write
//! ("make me a collection of …"), and import back. Entries are matched to the library by title
//! id, then title, then a close title on the same system; games not in the library stay in the
//! collection by name and are found later if they arrive.
use crate::db::Db;
use crate::parse::norm_title;
use crate::shared::journal::{EntryKind, Journal};
use crate::systems::{registry, SystemDef};
use crate::userdata::{game_ref, GameRef, ItemRef};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "index-collection";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollectionFile {
    #[serde(default)]
    pub format: String,
    #[serde(default = "one")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub notes: Option<String>,
    pub games: Vec<FileGame>,
}
fn one() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileGame {
    pub title: String,
    /// a system id ("n3ds"), name ("Nintendo 3DS") or common short name ("3DS")
    #[serde(default)]
    pub system: Option<String>,
    /// why it's here, or where it fits ("skip — weak reviews")
    #[serde(default)]
    pub note: Option<String>,
    /// Switch title id, PSP/Vita content id, 3DS title id, or a disc serial, when known
    #[serde(default, alias = "id", alias = "serial")]
    pub title_id: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

/// Short names people (and AIs) use for systems, beyond the app's own names and folder names.
fn aliases(id: &str) -> &'static [&'static str] {
    match id {
        "n3ds" => &["3DS", "N3DS", "Nintendo 3DS", "New 3DS"],
        "nds" => &["DS", "NDS", "Nintendo DS"],
        "gba" => &["GBA", "Game Boy Advance", "Gameboy Advance"],
        "gbc" => &["GBC", "Game Boy Color", "Gameboy Color"],
        "gb" => &["GB", "Game Boy", "Gameboy"],
        "switch" => &["Switch", "NSW", "Nintendo Switch"],
        "gamecube" => &["GameCube", "GC", "NGC", "GCN"],
        "nes" => &["NES", "Famicom", "Nintendo Entertainment System"],
        "snes" => &["SNES", "Super Nintendo", "Super Famicom", "SFC"],
        "virtualboy" => &["Virtual Boy", "VB"],
        "ps1" => &["PS1", "PSX", "PlayStation", "PS One", "PSOne"],
        "psp" => &["PSP", "PlayStation Portable"],
        "psvita" => &["Vita", "PS Vita", "PSV", "PlayStation Vita"],
        "genesis" => &["Genesis", "Mega Drive", "Sega Genesis", "Sega Mega Drive", "MD"],
        "sms" => &["Master System", "SMS", "Sega Master System"],
        "gamegear" => &["Game Gear", "GG", "Sega Game Gear"],
        "sega32x" => &["32X", "Sega 32X"],
        "sg1000" => &["SG-1000", "SG1000"],
        "pce" => &["PC Engine", "TurboGrafx-16", "TurboGrafx", "TG16", "PCE"],
        "ngpc" => &["Neo Geo Pocket Color", "NGPC"],
        "neogeo" => &["Neo Geo", "Neo-Geo", "MVS", "AES"],
        "lynx" => &["Lynx", "Atari Lynx"],
        "atari2600" => &["Atari 2600", "2600", "VCS"],
        "atari5200" => &["Atari 5200", "5200"],
        "atari7800" => &["Atari 7800", "7800"],
        "vectrex" => &["Vectrex"],
        "pc" => &["PC", "Windows", "DOS"],
        _ => &[],
    }
}

fn key(s: &str) -> String { s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase() }

/// The system a name means, if any.
pub fn system_for(name: &str) -> Option<&'static SystemDef> {
    let k = key(name);
    if k.is_empty() { return None; }
    registry().iter().find(|s| key(s.id) == k || key(s.name) == k || s.folders.iter().any(|f| key(f) == k) || aliases(s.id).iter().any(|a| key(a) == k)
        || crate::metadata::consoles::names(s.id).iter().any(|a| key(a) == k))
}

/// The system ids and names the format accepts, for the AI prompt.
pub fn systems_help() -> Vec<(String, String)> {
    registry().iter().map(|s| (s.id.to_string(), s.name.to_string())).collect()
}

/// Read a collection file, also when an AI wrapped it in a ```json fence or added text around it.
pub fn parse(text: &str) -> anyhow::Result<CollectionFile> {
    let t = text.trim();
    let start = t.find('{').ok_or_else(|| anyhow::anyhow!("no collection found — expected JSON starting with {{"))?;
    let end = t.rfind('}').ok_or_else(|| anyhow::anyhow!("the JSON isn't complete"))?;
    let mut f: CollectionFile = serde_json::from_str(&t[start..=end]).map_err(|e| anyhow::anyhow!("not a valid collection: {e}"))?;
    if f.name.trim().is_empty() { f.name = "Imported collection".into(); }
    f.games.retain(|g| !g.title.trim().is_empty());
    if f.games.is_empty() { anyhow::bail!("the collection has no games"); }
    Ok(f)
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate { pub game_id: i64, pub title: String, pub system: String, pub regions: Vec<String> }

#[derive(Debug, Clone, Serialize)]
pub struct PreviewEntry {
    pub title: String,
    /// the system it resolved to, or None when the name wasn't recognised (or none was given)
    pub system: Option<String>,
    pub note: Option<String>,
    /// "id" | "title" | "close" | "none"
    pub how: String,
    pub matched: Option<Candidate>,
    /// other games it could be (other regions or close titles), best first
    pub alternatives: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview { pub name: String, pub notes: Option<String>, pub entries: Vec<PreviewEntry> }

fn candidates_on(db: &Db, system: Option<&str>) -> anyhow::Result<Vec<(i64, String, String, Vec<String>)>> {
    let mut q = db.conn().prepare("SELECT id, title, system_id, parsed_json FROM games WHERE (?1 IS NULL OR system_id=?1)")?;
    let v = q.query_map([system], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?)))?
        .filter_map(|x| x.ok())
        .map(|(id, t, s, p)| (id, t, s, p.and_then(|p| serde_json::from_str::<crate::parse::ParsedName>(&p).ok()).map(|p| p.regions).unwrap_or_default()))
        .collect();
    Ok(v)
}

/// How each entry of a collection file matches the library.
pub fn preview(db: &Db, file: &CollectionFile, region_order: &[String]) -> anyhow::Result<ImportPreview> {
    let mut entries = Vec::new();
    let mut cache: std::collections::HashMap<Option<String>, Vec<(i64, String, String, Vec<String>)>> = std::collections::HashMap::new();
    for g in &file.games {
        let sys = g.system.as_deref().and_then(system_for).map(|s| s.id.to_string());
        let pool = match cache.get(&sys) { Some(p) => p.clone(), None => { let p = candidates_on(db, sys.as_deref())?; cache.insert(sys.clone(), p.clone()); p } };
        let cand = |c: &(i64, String, String, Vec<String>)| Candidate { game_id: c.0, title: c.1.clone(), system: c.2.clone(), regions: c.3.clone() };
        // the region you asked for, then your preferred regions
        let region_rank = |regions: &Vec<String>| -> usize {
            if let Some(want) = &g.region { if regions.iter().any(|r| r.eq_ignore_ascii_case(want)) { return 0; } }
            1 + regions.iter().filter_map(|r| region_order.iter().position(|o| o == r)).min().unwrap_or(region_order.len())
        };
        let mut how = "none";
        let mut found: Vec<Candidate> = Vec::new();
        // 1. a title id or serial read from the files
        if let Some(id) = g.title_id.as_deref().map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty()) {
            let hit: Option<i64> = db.conn().query_row(
                "SELECT game_id FROM files WHERE upper(json_extract(probe_json,'$.id'))=?1 OR upper(json_extract(probe_json,'$.id')) LIKE '%' || ?1 || '%' LIMIT 1", [&id], |r| r.get(0)).ok();
            if let Some(h) = hit {
                let all = if pool.iter().any(|c| c.0 == h) { pool.clone() } else { candidates_on(db, None)? };
                if let Some(c) = all.iter().find(|c| c.0 == h) { how = "id"; found.push(cand(c)); }
            }
        }
        // 2. the same title (ignoring punctuation, accents and trademark signs)
        if found.is_empty() {
            let want = norm_title(&g.title);
            let mut same: Vec<&(i64, String, String, Vec<String>)> = pool.iter().filter(|c| norm_title(&c.1) == want).collect();
            same.sort_by_key(|c| region_rank(&c.3));
            if !same.is_empty() { how = "title"; found = same.into_iter().map(cand).collect(); }
        }
        // 3. a close title on the same system
        if found.is_empty() && sys.is_some() {
            let mut close: Vec<(f32, &(i64, String, String, Vec<String>))> = pool.iter().map(|c| (crate::metadata::similarity(&g.title, &c.1), c)).filter(|(s, _)| *s >= 0.8).collect();
            close.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(region_rank(&a.1 .3).cmp(&region_rank(&b.1 .3))));
            if !close.is_empty() { how = "close"; found = close.into_iter().take(6).map(|(_, c)| cand(c)).collect(); }
        }
        let matched = found.first().cloned();
        entries.push(PreviewEntry {
            title: g.title.trim().to_string(), system: sys.or_else(|| matched.as_ref().map(|m| m.system.clone())), note: g.note.clone().filter(|n| !n.trim().is_empty()),
            how: how.into(), matched, alternatives: found.into_iter().skip(1).collect(),
        });
    }
    Ok(ImportPreview { name: file.name.trim().to_string(), notes: file.notes.clone().filter(|n| !n.trim().is_empty()), entries })
}

/// Create the collection. `choices[i]` is the game chosen for entry i (None: keep it by name).
pub fn import(db: &Db, journal: &Journal, file: &CollectionFile, choices: &[Option<i64>]) -> anyhow::Result<String> {
    let id = crate::userdata::create_collection(db, journal, &file.name, &[])?;
    if file.notes.as_deref().map(|n| !n.trim().is_empty()).unwrap_or(false) {
        crate::userdata::update_collection(db, journal, &id, &file.name, file.notes.clone(), false)?;
    }
    let mut items: Vec<ItemRef> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (i, g) in file.games.iter().enumerate() {
        let gr = match choices.get(i).copied().flatten() {
            Some(gid) => game_ref(db, gid)?,
            None => {
                // not in the library: kept by name, found later by title or id if the game arrives
                let sys = g.system.as_deref().and_then(system_for).map(|s| s.id.to_string()).unwrap_or_default();
                GameRef { system: sys, key: format!("title:{}", norm_title(&g.title)), title: g.title.trim().to_string(), ids: g.title_id.iter().map(|s| s.trim().to_uppercase()).collect() }
            }
        };
        if !seen.insert(gr.local()) { continue; }
        items.push(ItemRef { game: gr, note: g.note.clone().filter(|n| !n.trim().is_empty()) });
    }
    crate::userdata::store_items(db, &id, &items, crate::shared::now_ms())?;
    let _ = journal.append(&EntryKind::CollectionItems { id: id.clone(), items });
    Ok(id)
}

/// A collection as a file.
pub fn export(db: &Db, id: &str) -> anyhow::Result<CollectionFile> {
    let c = crate::userdata::collection(db, id)?;
    let mut games = Vec::new();
    for it in &c.items {
        let r: Option<GameRef> = db.conn().query_row("SELECT ref_json FROM collection_items WHERE collection_id=?1 AND pos=?2", (id, it.pos), |x| x.get::<_, String>(0)).ok().and_then(|j| serde_json::from_str(&j).ok());
        let regions = it.game_id.and_then(|g| db.conn().query_row("SELECT parsed_json FROM games WHERE id=?1", [g], |x| x.get::<_, Option<String>>(0)).ok().flatten())
            .and_then(|p| serde_json::from_str::<crate::parse::ParsedName>(&p).ok()).map(|p| p.regions).unwrap_or_default();
        games.push(FileGame {
            title: it.title.clone(), system: Some(it.system.clone()), note: it.note.clone(),
            title_id: r.and_then(|r| r.ids.into_iter().next()), region: regions.into_iter().next(),
        });
    }
    Ok(CollectionFile { format: FORMAT.into(), version: 1, name: c.name, notes: c.notes, games })
}
