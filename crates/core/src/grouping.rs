//! Assign files to games according to the system's GroupKind.
use crate::db::Db;
use crate::parse::{norm_title, parse, ParsedName};
use crate::systems::{GroupKind, SystemDef};
use crate::util::{full_stem, stem_of, switch_base_id};
use std::collections::HashMap;
use std::path::Path;

struct Assign { file_id: i64, old_game: Option<i64>, key: String, title: String, parsed: ParsedName, rank: u8 }

pub fn release_key(p: &ParsedName) -> String {
    format!("{}|{}|{}|{}|{}", p.title.to_lowercase(), p.regions.join(",").to_lowercase(),
        p.rev.clone().unwrap_or_default(), p.version.clone().unwrap_or_default(), p.tags.iter().filter(|t| !t.to_lowercase().starts_with("track")).cloned().collect::<Vec<_>>().join(",").to_lowercase())
}

/// Switch: one game per application id. The id comes from the file header (most reliable), else
/// the file name, else the folder name. Files with no id join a game when their folder holds that
/// game's files, or when their title matches a game's title ("i-am-setsuna" ↔ "I Am Setsuna").
/// Keys keep the "tid:<first 12 hex digits>" form so per-game state survives regrouping.
fn switch_assign(system: &SystemDef, files: Vec<(i64, String, i64, Option<i64>, Option<String>)>, first_level: &dyn Fn(i64) -> Option<String>) -> Vec<Assign> {
    struct W { file_id: i64, old_game: Option<i64>, folder: Option<String>, parsed: ParsedName, key: Option<String>, title: String, rank: (u8, u8) }
    let hex_ok = |t: &str| t.len() == 16 && t.starts_with("01") && t.chars().all(|c| c.is_ascii_hexdigit());
    let key_of = |tid: &str| switch_base_id(tid).map(|b| format!("tid:{}", &format!("{b:016X}")[..12]));
    let mut ws = Vec::with_capacity(files.len());
    for (file_id, name, dir_id, old_game, probe_id) in files {
        let p = parse(system.parsers, &full_stem(&name));
        let folder = first_level(dir_id);
        let fp = folder.as_ref().map(|f| parse(system.parsers, &Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        let header_tid = probe_id.filter(|t| hex_ok(t));
        let name_tid = p.title_id.clone().filter(|t| hex_ok(t));
        let folder_tid = fp.as_ref().and_then(|f| f.title_id.clone()).filter(|t| hex_ok(t));
        let tid = header_tid.clone().or(name_tid.clone()).or(folder_tid.clone());
        let not_base = tid.as_deref().and_then(|t| u64::from_str_radix(t, 16).ok()).map(|v| (v & 0x1FFF != 0) as u8).unwrap_or(1);
        let slug = p.flags.iter().any(|f| f == "slug");
        // which name best describes the game: a "Title [id]" folder, then a "Title [id]" file,
        // then a plain folder, then any file name
        let (title, rank) = match (&fp, &folder_tid) {
            (Some(f), Some(_)) if !f.title.is_empty() => (f.title.clone(), 0),
            _ if name_tid.is_some() && !slug && !p.title.is_empty() => (p.title.clone(), 1),
            _ if fp.as_ref().map(|f| !f.title.is_empty()).unwrap_or(false) => (fp.as_ref().unwrap().title.clone(), 2),
            _ => (p.title.clone(), if slug { 4 } else { 3 }),
        };
        ws.push(W { file_id, old_game, key: tid.as_deref().and_then(key_of), folder, parsed: p, title, rank: (rank, not_base) });
    }
    // a folder whose identified files all belong to one game takes its unidentified files along
    let mut folder_keys: HashMap<String, Vec<String>> = HashMap::new();
    for w in ws.iter() { if let (Some(f), Some(k)) = (&w.folder, &w.key) { folder_keys.entry(f.clone()).or_default().push(k.clone()); } }
    for v in folder_keys.values_mut() { v.sort(); v.dedup(); }
    for w in ws.iter_mut().filter(|w| w.key.is_none()) {
        if let Some(k) = w.folder.as_ref().and_then(|f| folder_keys.get(f)).filter(|v| v.len() == 1) { w.key = Some(k[0].clone()); }
    }
    // a copy under another name: same title as exactly one identified game
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for w in ws.iter() {
        if let Some(k) = &w.key {
            for t in [&w.title, &w.parsed.title] { let n = norm_title(t); if !n.is_empty() { by_name.entry(n).or_default().push(k.clone()); } }
        }
    }
    for v in by_name.values_mut() { v.sort(); v.dedup(); }
    for w in ws.iter_mut().filter(|w| w.key.is_none()) {
        let cands = [norm_title(&w.parsed.title), norm_title(&w.title)];
        if let Some(k) = cands.iter().find_map(|n| by_name.get(n).filter(|v| v.len() == 1)) { w.key = Some(k[0].clone()); }
    }
    ws.into_iter().map(|w| {
        let key = w.key.clone().unwrap_or_else(|| match &w.folder { Some(f) => format!("dir:{f}"), None => release_key(&w.parsed) });
        Assign { file_id: w.file_id, old_game: w.old_game, key, title: w.title, parsed: w.parsed, rank: w.rank.0 * 2 + w.rank.1 }
    }).collect()
}

pub fn assign_games(db: &Db, system: &SystemDef) -> anyhow::Result<()> {
    let c = db.conn();
    let tx = c.unchecked_transaction()?;
    // dir id -> (parent id, path)
    let dirs: HashMap<i64, (Option<i64>, String)> = {
        let mut q = tx.prepare("SELECT id, parent_id, path FROM dirs WHERE system_id=?1")?;
        let m = q.query_map([system.id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?.collect::<Result<_, _>>()?;
        m
    };
    // first-level folder under the system root, if the file is nested
    let first_level = |mut d: i64| -> Option<String> {
        let mut chain = vec![d];
        while let Some((Some(p), _)) = dirs.get(&d) { d = *p; chain.push(d); }
        if chain.len() >= 2 { dirs.get(&chain[chain.len() - 2]).map(|(_, p)| p.clone()) } else { None }
    };
    let files: Vec<(i64, String, i64, Option<i64>, Option<String>)> = {
        let mut q = tx.prepare("SELECT id, name, dir_id, game_id, json_extract(probe_json,'$.id') FROM files WHERE system_id=?1")?;
        let v = q.query_map([system.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        v
    };
    let mut out = Vec::with_capacity(files.len());
    let files = if system.group == GroupKind::SwitchTitle { out = switch_assign(system, files, &first_level); Vec::new() } else { files };
    for (file_id, name, dir_id, old_game, probe_id) in files {
        let p = parse(system.parsers, &stem_of(&name));
        let folder = first_level(dir_id);
        let folder_parsed = folder.as_ref().map(|f| parse(system.parsers, Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_str()));
        let (key, title, parsed, rank) = match system.group {
            GroupKind::SwitchTitle => unreachable!(),
            GroupKind::PkgTitle => {
                // content id "UP4395-PCSE00890_00-…" → title id "PCSE00890"
                let tid = probe_id.as_deref().and_then(|c| c.split('-').nth(1)).and_then(|t| t.split('_').next()).filter(|t| t.len() >= 9);
                let sub = folder.as_deref().and_then(|f| Path::new(f).file_name()).map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                // the copy in Games/ names the group; updates and DLC only if nothing better exists
                let rank = if sub.starts_with("games") { 0 } else if sub.contains("dlc") { 3 } else if sub.contains("update") { 2 } else { 1 };
                match tid {
                    Some(t) => (format!("tid:{t}"), p.display().to_string(), p, rank),
                    None => (release_key(&p), p.display().to_string(), p, rank),
                }
            }
            GroupKind::CueBin | GroupKind::FolderPerGame if folder.is_some() => {
                let fp = folder_parsed.unwrap();
                (format!("dir:{}", folder.unwrap()), fp.display().to_string(), fp, 0)
            }
            _ => {
                let rank = if p.flags.iter().any(|f| f == "decrypted-suffix") { 1 } else { 0 };
                (release_key(&p), p.display().to_string(), p, rank)
            }
        };
        out.push(Assign { file_id, old_game, key, title, parsed, rank });
    }
    // Representative per key: lowest rank wins for the title.
    out.sort_by(|a, b| a.key.cmp(&b.key).then(a.rank.cmp(&b.rank)));
    let mut key_ids: HashMap<String, i64> = HashMap::new();
    // where each old game's files went: a game whose files all moved to one other game is that
    // game now (its copies, play status and Collection entries follow it)
    let mut moved: HashMap<i64, std::collections::HashSet<i64>> = HashMap::new();
    for a in &out {
        let gid = if let Some(id) = key_ids.get(&a.key) { *id } else {
            let title = if a.title.is_empty() { a.key.clone() } else { a.title.clone() };
            tx.execute("INSERT INTO games(system_id, group_key, title, parsed_json) VALUES(?1,?2,?3,?4)
                        ON CONFLICT(system_id, group_key) DO UPDATE SET title=excluded.title, parsed_json=excluded.parsed_json",
                (system.id, &a.key, &title, serde_json::to_string(&a.parsed)?))?;
            let id: i64 = tx.query_row("SELECT id FROM games WHERE system_id=?1 AND group_key=?2", (system.id, &a.key), |r| r.get(0))?;
            key_ids.insert(a.key.clone(), id);
            id
        };
        if let Some(old) = a.old_game { moved.entry(old).or_default().insert(gid); }
        if a.old_game != Some(gid) {
            if let Some(old) = a.old_game {
                tx.execute("INSERT OR IGNORE INTO status(game_id, key, value, updated_at) SELECT ?1, key, value, updated_at FROM status WHERE game_id=?2", (gid, old))?;
            }
            tx.execute("UPDATE files SET game_id=?1 WHERE id=?2", (gid, a.file_id))?;
        }
    }
    // a game goes when it has neither files nor a copy on the shelf
    tx.execute("DELETE FROM status WHERE game_id IN (SELECT id FROM games WHERE system_id=?1 AND id NOT IN (SELECT game_id FROM files WHERE game_id IS NOT NULL))", [system.id])?;
    tx.execute("DELETE FROM games WHERE system_id=?1 AND id NOT IN (SELECT game_id FROM files WHERE game_id IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM shelf_copies c WHERE c.deleted=0 AND c.ref = games.system_id || ':' || games.group_key)", [system.id])?;
    tx.commit()?;
    for (old, to) in moved {
        if to.len() != 1 || to.contains(&old) { continue; }
        let to = *to.iter().next().unwrap();
        // still here only because of its shelf copies, and with no files left
        let lonely: bool = c.query_row("SELECT EXISTS (SELECT 1 FROM games WHERE id=?1) AND NOT EXISTS (SELECT 1 FROM files WHERE game_id=?1)", [old], |r| r.get(0))?;
        if lonely { crate::shelf::merge(db, old, to)?; }
    }
    Ok(())
}
