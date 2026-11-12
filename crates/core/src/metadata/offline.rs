//! Offline title databases: exact id → name (and for Switch, description + art URLs).
use super::{ArtKind, ArtRef, MetaCandidate};
use crate::db::Db;
use rusqlite::OptionalExtension;
use std::path::Path;

/// NoPayStation-format TSV (PSP_GAMES.tsv): keyed by Content ID.
pub fn psp_tsv_import(db: &Db, path: &Path) -> anyhow::Result<usize> {
    let text = String::from_utf8_lossy(&std::fs::read(path)?).into_owned();
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let col = |n: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(n));
    let (Some(ci), Some(ni)) = (col("Content ID"), col("Name")) else { anyhow::bail!("not a NoPayStation TSV (missing Content ID / Name columns)") };
    let ri = col("Region");
    let ti = col("Title ID");
    let tx = db.conn().unchecked_transaction()?;
    let mut n = 0;
    for l in lines {
        let c: Vec<&str> = l.split('\t').collect();
        let cid = c.get(ci).map(|s| s.trim()).unwrap_or("");
        if cid.is_empty() || cid == "MISSING" { continue; }
        let name = c.get(ni).map(|s| s.trim()).unwrap_or("");
        let extra = serde_json::json!({"title_id": ti.and_then(|i| c.get(i)).map(|s| s.trim())});
        tx.execute("INSERT INTO titledb(source, id, name, region, extra_json) VALUES('psp',?1,?2,?3,?4) ON CONFLICT(source,id) DO UPDATE SET name=excluded.name, region=excluded.region, extra_json=excluded.extra_json",
            (cid, name, ri.and_then(|i| c.get(i)).map(|s| s.trim()), extra.to_string()))?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// NoPayStation's PS Vita game list (PSV_GAMES.tsv), as source "psv": title id → name and region.
pub fn psv_tsv_import(db: &Db, text: &str) -> anyhow::Result<usize> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let col = |n: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(n));
    let (Some(ti), Some(ni)) = (col("Title ID"), col("Name")) else { anyhow::bail!("not a NoPayStation TSV (missing Title ID / Name columns)") };
    let ri = col("Region");
    let tx = db.conn().unchecked_transaction()?;
    tx.execute("DELETE FROM titledb WHERE source='psv'", [])?;
    let mut n = 0;
    for l in lines {
        let c: Vec<&str> = l.split('\t').collect();
        let (tid, name) = (c.get(ti).map(|s| s.trim()).unwrap_or(""), c.get(ni).map(|s| s.trim()).unwrap_or(""));
        if tid.len() != 9 || name.is_empty() { continue; }
        tx.execute("INSERT INTO titledb(source, id, name, region) VALUES('psv',?1,?2,?3) ON CONFLICT(source,id) DO NOTHING",
            (tid, name, ri.and_then(|i| c.get(i)).map(|s| s.trim())))?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// FBNeo's arcade list: set name → title, as source "arcade".
pub fn arcade_import(db: &Db, path: &Path) -> anyhow::Result<usize> {
    let sets = crate::dat::arcade_sets(path)?;
    if sets.is_empty() { anyhow::bail!("no arcade sets found in {}", path.display()); }
    let tx = db.conn().unchecked_transaction()?;
    for (set, title) in &sets {
        tx.execute("INSERT INTO titledb(source, id, name) VALUES('arcade',?1,?2) ON CONFLICT(source,id) DO UPDATE SET name=excluded.name", (set.to_lowercase(), title))?;
    }
    tx.commit()?;
    Ok(sets.len())
}

/// The name an arcade set is known by: "Art of Fighting" for "Art of Fighting / Ryuuko no Ken
/// (NGM-044)(NGH-044)" (after " / " comes the Japanese title; in brackets, the cartridge codes).
/// A set the list doesn't name is usually a variant of one it does ("kof98n", "mslug3nd",
/// "garouo"): its parent is found by dropping the variant's ending ("nd", or a letter or two).
pub fn arcade_title(db: &Db, set: &str) -> anyhow::Result<Option<String>> {
    let set = set.to_lowercase();
    let mut tries = vec![set.clone()];
    if let Some(p) = set.strip_suffix("nd") { tries.push(p.to_string()); }
    for k in 1..=2 { if set.len() > k + 3 { tries.push(set[..set.len() - k].to_string()); } }
    let mut name: Option<String> = None;
    for t in tries.iter().filter(|t| t.is_ascii()) {
        name = db.conn().query_row("SELECT name FROM titledb WHERE source='arcade' AND id=?1", [t], |r| r.get(0)).optional()?;
        if name.is_some() { break; }
    }
    Ok(name.map(|n| {
        let n = n.split(" / ").next().unwrap_or(&n);
        n.find(" (").map_or(n, |i| &n[..i]).trim().to_string()
    }).filter(|n| !n.is_empty()))
}

/// Bumped when the import changes what it keeps, so an already-downloaded file is read again.
pub const SWITCH_IMPORT_VERSION: u32 = 2;

/// blawar/titledb region file (e.g. US.en.json), keyed by eShop listing; stored by title id.
///
/// Several listings can share one title id: the game itself, and later bundles ("Atelier Dusk
/// Trilogy Deluxe Pack" carries Ayesha's id), editions and Switch 2 versions. The game is the one
/// that isn't a bundle (bundle listings are numbered 7005…), and of those the first listed.
pub fn switch_titledb_import(db: &Db, path: &Path) -> anyhow::Result<usize> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let obj = v.as_object().ok_or_else(|| anyhow::anyhow!("not a titledb JSON object"))?;
    let mut best: std::collections::HashMap<String, (&serde_json::Value, (bool, u64))> = std::collections::HashMap::new();
    for (key, e) in obj {
        let (Some(id), Some(_)) = (e["id"].as_str(), e["name"].as_str()) else { continue };
        let rank = listing_rank(key, e);
        let slot = best.entry(id.to_uppercase()).or_insert((e, rank));
        if rank < slot.1 { *slot = (e, rank); }
    }
    let tx = db.conn().unchecked_transaction()?;
    for (id, (e, _)) in &best {
        let extra = serde_json::json!({"description": e["description"], "publisher": e["publisher"], "releaseDate": e["releaseDate"], "iconUrl": e["iconUrl"], "bannerUrl": e["bannerUrl"]});
        tx.execute("INSERT INTO titledb(source, id, name, region, extra_json) VALUES('switch',?1,?2,?3,?4) ON CONFLICT(source,id) DO UPDATE SET name=excluded.name, extra_json=excluded.extra_json",
            (id, e["name"].as_str(), e["region"].as_str(), extra.to_string()))?;
    }
    tx.commit()?;
    Ok(best.len())
}

/// Lower is the game itself: not a bundle, then the earliest listing.
fn listing_rank(key: &str, e: &serde_json::Value) -> (bool, u64) {
    let nsu = e["nsuId"].as_u64().or_else(|| e["nsuId"].as_str().and_then(|s| s.parse().ok())).or_else(|| key.parse().ok()).unwrap_or(u64::MAX);
    (nsu.to_string().starts_with("7005"), nsu)
}

/// Switch games matched by title id whose stored match no longer agrees with the title database
/// (an older import kept a bundle or a later edition): (game id, title id).
pub fn stale_switch_matches(db: &Db) -> anyhow::Result<Vec<(i64, String)>> {
    let mut q = db.conn().prepare(
        "SELECT g.id, t.id FROM games g JOIN titledb t ON t.source='switch' AND t.id=upper(json_extract(g.meta_json,'$.source_id'))
         WHERE g.system_id='switch' AND json_extract(g.meta_json,'$.source')='titledb-switch' AND json_extract(g.meta_json,'$.title') <> t.name")?;
    let v = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn lookup(db: &Db, source: &str, id: &str) -> anyhow::Result<Option<MetaCandidate>> {
    let r: Option<(String, Option<String>)> = db.conn().query_row("SELECT name, extra_json FROM titledb WHERE source=?1 AND id=?2", (source, id.to_uppercase()), |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    Ok(r.map(|(name, extra)| {
        let x: serde_json::Value = extra.and_then(|e| serde_json::from_str(&e).ok()).unwrap_or_default();
        let mut art = Vec::new();
        if let Some(u) = x["iconUrl"].as_str() { art.push(ArtRef { kind: ArtKind::Icon, url: u.into() }); }
        if let Some(u) = x["bannerUrl"].as_str() { art.push(ArtRef { kind: ArtKind::Banner, url: u.into() }); }
        MetaCandidate {
            source: format!("titledb-{source}"), source_id: id.to_string(), title: name, score: 1.0,
            description: x["description"].as_str().map(String::from), publisher: x["publisher"].as_str().map(String::from),
            release: x["releaseDate"].as_i64().map(|d| format!("{}-{:02}-{:02}", d / 10000, d / 100 % 100, d % 100)),
            art, ..Default::default()
        }
    }))
}
