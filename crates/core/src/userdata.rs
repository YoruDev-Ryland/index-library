//! Your own data about games: play status (status, score, notes, dates…) and collections with a
//! play order. Every change is stored locally and written to this PC's shared journal, so every
//! PC using the library ends up with the same data (newest change wins).
use crate::db::Db;
use crate::parse::norm_title;
use crate::shared::journal::{EntryKind, Journal};
use crate::shared::now_ms;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::BTreeMap;

/// How games, play status and collection entries are named between PCs, phones and the sync
/// server (shared with the server: `index-sync-protocol`).
pub use index_sync_protocol::{GameRef, ItemRef, PlayInfo};

pub const STATUSES: &[&str] = &["want", "playing", "finished", "completed", "onhold", "dropped", "skip"];

pub fn game_ref(db: &Db, game_id: i64) -> anyhow::Result<GameRef> {
    let (system, key, title): (String, String, String) = db.conn().query_row("SELECT system_id, group_key, title FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut q = db.conn().prepare("SELECT DISTINCT json_extract(probe_json,'$.id') FROM files WHERE game_id=?1 AND json_extract(probe_json,'$.id') IS NOT NULL ORDER BY 1 LIMIT 8")?;
    let ids: Vec<String> = q.query_map([game_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(GameRef { system, key, title, ids })
}

/// The local game a reference points at: same key, else a shared id, else the same title.
pub fn resolve(db: &Db, r: &GameRef) -> anyhow::Result<Option<i64>> {
    let c = db.conn();
    if let Some(id) = c.query_row("SELECT id FROM games WHERE system_id=?1 AND group_key=?2", (&r.system, &r.key), |x| x.get(0)).optional()? { return Ok(Some(id)); }
    for i in &r.ids {
        if let Some(id) = c.query_row("SELECT game_id FROM files WHERE system_id=?1 AND json_extract(probe_json,'$.id')=?2 AND game_id IS NOT NULL LIMIT 1", (&r.system, i), |x| x.get(0)).optional()? { return Ok(Some(id)); }
    }
    let want = norm_title(&r.title);
    if want.is_empty() { return Ok(None); }
    let mut q = c.prepare("SELECT id, title FROM games WHERE system_id=?1")?;
    let rows: Vec<(i64, String)> = q.query_map([&r.system], |x| Ok((x.get(0)?, x.get(1)?)))?.collect::<Result<_, _>>()?;
    let hits: Vec<i64> = rows.into_iter().filter(|(_, t)| norm_title(t) == want).map(|(i, _)| i).collect();
    Ok(if hits.len() == 1 { Some(hits[0]) } else { None })
}

/// The local key a reference is stored under: the local game's key when it resolves.
fn local_key(db: &Db, r: &GameRef) -> anyhow::Result<(String, GameRef)> {
    match resolve(db, r)? {
        Some(id) => { let mut l = game_ref(db, id)?; if l.ids.is_empty() { l.ids = r.ids.clone(); } Ok((l.local(), l)) }
        None => Ok((r.local(), r.clone())),
    }
}

// ---- play status ----

fn store_play(db: &Db, r: &GameRef, p: &PlayInfo, ts: i64) -> anyhow::Result<bool> {
    let (key, r) = local_key(db, r)?;
    let n = db.conn().execute(
        "INSERT INTO play(ref, ref_json, status, score, notes, started, finished, hours, favorite, updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(ref) DO UPDATE SET ref_json=excluded.ref_json, status=excluded.status, score=excluded.score, notes=excluded.notes, started=excluded.started,
           finished=excluded.finished, hours=excluded.hours, favorite=excluded.favorite, updated_at=excluded.updated_at WHERE excluded.updated_at >= play.updated_at",
        (key, serde_json::to_string(&r)?, &p.status, p.score, &p.notes, &p.started, &p.finished, p.hours, p.favorite as i64, ts))?;
    Ok(n > 0)
}

pub fn play_of(db: &Db, game_id: i64) -> anyhow::Result<Option<PlayInfo>> {
    let r = game_ref(db, game_id)?;
    db.conn().query_row("SELECT status, score, notes, started, finished, hours, favorite FROM play WHERE ref=?1", [r.local()], |x| Ok(PlayInfo {
        status: x.get(0)?, score: x.get::<_, Option<i64>>(1)?.map(|v| v.clamp(0, 10) as u8), notes: x.get(2)?, started: x.get(3)?, finished: x.get(4)?, hours: x.get(5)?, favorite: x.get::<_, i64>(6)? != 0,
    })).optional().map_err(Into::into)
}

/// Save play info for a game. Dates fill in on their own: started when you begin playing,
/// finished when you finish or complete it (unless you set them).
pub fn set_play(db: &Db, journal: &Journal, game_id: i64, mut p: PlayInfo) -> anyhow::Result<PlayInfo> {
    if let Some(s) = &p.status { if !STATUSES.contains(&s.as_str()) { anyhow::bail!("unknown play status {s}"); } }
    p.score = p.score.map(|v| v.clamp(1, 10));
    p.notes = p.notes.filter(|n| !n.trim().is_empty());
    let old = play_of(db, game_id)?.unwrap_or_default();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if p.status.as_deref() == Some("playing") && old.status.as_deref() != Some("playing") && p.started.is_none() { p.started = Some(today.clone()); }
    if matches!(p.status.as_deref(), Some("finished" | "completed")) && !matches!(old.status.as_deref(), Some("finished" | "completed")) && p.finished.is_none() { p.finished = Some(today); }
    let r = game_ref(db, game_id)?;
    let ts = now_ms();
    store_play(db, &r, &p, ts)?;
    let _ = journal.append(&EntryKind::Play { game: r, info: p.clone() });
    Ok(p)
}

// ---- collections ----

#[derive(Debug, Clone, Serialize)]
pub struct CollectionSummary {
    pub id: String, pub name: String, pub notes: Option<String>,
    /// the system every game in it is for; None when mixed or empty
    pub system: Option<String>,
    pub count: usize, pub done: usize, pub skipped: usize,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionItem {
    pub pos: i64, pub r#ref: String, pub title: String, pub system: String, pub note: Option<String>,
    /// the game in this library, if it's here
    pub game_id: Option<i64>,
    pub cover: Option<String>,
    pub play: Option<PlayInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionDetail { pub id: String, pub name: String, pub notes: Option<String>, pub items: Vec<CollectionItem> }

fn play_by_ref(db: &Db, key: &str) -> anyhow::Result<Option<PlayInfo>> {
    db.conn().query_row("SELECT status, score, notes, started, finished, hours, favorite FROM play WHERE ref=?1", [key], |x| Ok(PlayInfo {
        status: x.get(0)?, score: x.get::<_, Option<i64>>(1)?.map(|v| v.clamp(0, 10) as u8), notes: x.get(2)?, started: x.get(3)?, finished: x.get(4)?, hours: x.get(5)?, favorite: x.get::<_, i64>(6)? != 0,
    })).optional().map_err(Into::into)
}

pub fn collection(db: &Db, id: &str) -> anyhow::Result<CollectionDetail> {
    let (name, notes): (String, Option<String>) = db.conn().query_row("SELECT name, notes FROM collections WHERE id=?1 AND deleted=0", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut q = db.conn().prepare("SELECT pos, ref, ref_json, note FROM collection_items WHERE collection_id=?1 ORDER BY pos")?;
    let rows: Vec<(i64, String, String, Option<String>)> = q.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
    let mut items = Vec::new();
    for (pos, key, rj, note) in rows {
        let gr: GameRef = serde_json::from_str(&rj).unwrap_or_default();
        let gid = resolve(db, &gr)?;
        let (title, cover) = match gid {
            Some(g) => db.conn().query_row("SELECT title, COALESCE(json_extract(art_json,'$.cover'), json_extract(art_json,'$.icon')) FROM games WHERE id=?1", [g], |r| Ok((r.get(0)?, r.get(1)?)))?,
            None => (gr.title.clone(), None),
        };
        let lkey = match gid { Some(g) => game_ref(db, g)?.local(), None => key.clone() };
        items.push(CollectionItem { pos, r#ref: key, title, system: gr.system.clone(), note, game_id: gid, cover, play: play_by_ref(db, &lkey)? });
    }
    Ok(CollectionDetail { id: id.to_string(), name, notes, items })
}

pub fn collections(db: &Db) -> anyhow::Result<Vec<CollectionSummary>> {
    let mut q = db.conn().prepare("SELECT id FROM collections WHERE deleted=0 ORDER BY name COLLATE NOCASE")?;
    let ids: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for id in ids {
        let c = collection(db, &id)?;
        let mut systems: Vec<&str> = c.items.iter().map(|i| i.system.as_str()).collect();
        systems.sort(); systems.dedup();
        let st = |i: &CollectionItem| i.play.as_ref().and_then(|p| p.status.clone()).unwrap_or_default();
        out.push(CollectionSummary {
            system: if systems.len() == 1 { Some(systems[0].to_string()) } else { None },
            count: c.items.len(),
            done: c.items.iter().filter(|i| matches!(st(i).as_str(), "finished" | "completed")).count(),
            skipped: c.items.iter().filter(|i| matches!(st(i).as_str(), "skip" | "dropped")).count(),
            next: c.items.iter().find(|i| !matches!(st(i).as_str(), "finished" | "completed" | "skip" | "dropped")).map(|i| i.title.clone()),
            id: c.id, name: c.name, notes: c.notes,
        });
    }
    Ok(out)
}

fn store_collection(db: &Db, id: &str, name: &str, notes: &Option<String>, deleted: bool, created: i64, ts: i64) -> anyhow::Result<()> {
    db.conn().execute(
        "INSERT INTO collections(id, name, notes, deleted, created_at, updated_at) VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, notes=excluded.notes, deleted=excluded.deleted, updated_at=excluded.updated_at WHERE excluded.updated_at >= collections.updated_at",
        (id, name, notes, deleted as i64, created, ts))?;
    Ok(())
}

pub(crate) fn store_items(db: &Db, id: &str, items: &[ItemRef], ts: i64) -> anyhow::Result<bool> {
    let cur: Option<i64> = db.conn().query_row("SELECT items_updated_at FROM collections WHERE id=?1", [id], |r| r.get(0)).optional()?;
    // the collection itself may arrive later from another PC: keep its items under a placeholder
    if cur.is_none() { store_collection(db, id, "Collection", &None, false, ts, 0)?; }
    if cur.unwrap_or(0) > ts { return Ok(false); }
    let tx = db.conn().unchecked_transaction()?;
    tx.execute("DELETE FROM collection_items WHERE collection_id=?1", [id])?;
    for (i, it) in items.iter().enumerate() {
        let (key, r) = local_key(db, &it.game)?;
        tx.execute("INSERT INTO collection_items(collection_id, pos, ref, ref_json, note) VALUES(?1,?2,?3,?4,?5)", (id, i as i64, key, serde_json::to_string(&r)?, &it.note))?;
    }
    tx.execute("UPDATE collections SET items_updated_at=?1 WHERE id=?2", (ts, id))?;
    tx.commit()?;
    Ok(true)
}

fn items_of(db: &Db, id: &str) -> anyhow::Result<Vec<ItemRef>> {
    let mut q = db.conn().prepare("SELECT ref_json, note FROM collection_items WHERE collection_id=?1 ORDER BY pos")?;
    let v = q.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v.into_iter().map(|(j, note)| ItemRef { game: serde_json::from_str(&j).unwrap_or_default(), note }).collect())
}

pub fn create_collection(db: &Db, journal: &Journal, name: &str, game_ids: &[i64]) -> anyhow::Result<String> {
    let name = name.trim();
    if name.is_empty() { anyhow::bail!("a collection needs a name"); }
    let id = uuid::Uuid::new_v4().to_string();
    let ts = now_ms();
    store_collection(db, &id, name, &None, false, ts, ts)?;
    let _ = journal.append(&EntryKind::Collection { id: id.clone(), name: name.into(), notes: None, deleted: false, created: ts });
    if !game_ids.is_empty() { add_to_collection(db, journal, &id, game_ids)?; }
    Ok(id)
}

pub fn update_collection(db: &Db, journal: &Journal, id: &str, name: &str, notes: Option<String>, deleted: bool) -> anyhow::Result<()> {
    let created: i64 = db.conn().query_row("SELECT created_at FROM collections WHERE id=?1", [id], |r| r.get(0))?;
    let name = if name.trim().is_empty() { db.conn().query_row("SELECT name FROM collections WHERE id=?1", [id], |r| r.get(0))? } else { name.trim().to_string() };
    let notes = notes.filter(|n| !n.trim().is_empty());
    store_collection(db, id, &name, &notes, deleted, created, now_ms())?;
    let _ = journal.append(&EntryKind::Collection { id: id.into(), name, notes, deleted, created });
    Ok(())
}

/// Replace a collection's entries, in play order. `entries` are (ref, note) of entries it
/// already has, so reordering, removing and editing notes all go through here.
pub fn set_items(db: &Db, journal: &Journal, id: &str, entries: &[(String, Option<String>)]) -> anyhow::Result<()> {
    let have: BTreeMap<String, ItemRef> = {
        let mut q = db.conn().prepare("SELECT ref, ref_json, note FROM collection_items WHERE collection_id=?1")?;
        let v = q.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
        v.into_iter().map(|(k, j, n)| (k, ItemRef { game: serde_json::from_str(&j).unwrap_or_default(), note: n })).collect()
    };
    let items: Vec<ItemRef> = entries.iter().filter_map(|(k, note)| have.get(k).map(|i| ItemRef { game: i.game.clone(), note: note.clone().filter(|n| !n.trim().is_empty()) })).collect();
    let ts = now_ms();
    store_items(db, id, &items, ts)?;
    let _ = journal.append(&EntryKind::CollectionItems { id: id.into(), items });
    Ok(())
}

/// Add games to the end of a collection (games already in it stay where they are).
pub fn add_to_collection(db: &Db, journal: &Journal, id: &str, game_ids: &[i64]) -> anyhow::Result<usize> {
    let mut items = items_of(db, id)?;
    let have: std::collections::HashSet<String> = items.iter().map(|i| i.game.local()).collect();
    let mut added = 0;
    for g in game_ids {
        let r = game_ref(db, *g)?;
        if have.contains(&r.local()) || items.iter().any(|i| i.game.local() == r.local()) { continue; }
        items.push(ItemRef { game: r, note: None });
        added += 1;
    }
    if added > 0 {
        store_items(db, id, &items, now_ms())?;
        let _ = journal.append(&EntryKind::CollectionItems { id: id.into(), items });
    }
    Ok(added)
}

/// The collections a game is in.
pub fn collections_of(db: &Db, game_id: i64) -> anyhow::Result<Vec<(String, String, i64)>> {
    let key = game_ref(db, game_id)?.local();
    let mut q = db.conn().prepare("SELECT c.id, c.name, i.pos FROM collection_items i JOIN collections c ON c.id=i.collection_id WHERE i.ref=?1 AND c.deleted=0 ORDER BY c.name")?;
    let v = q.query_map([key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

// ---- other PCs' changes ----

pub fn apply_play(db: &Db, game: &GameRef, info: &PlayInfo, ts: i64) -> anyhow::Result<()> { store_play(db, game, info, ts).map(|_| ()) }
pub fn apply_collection(db: &Db, id: &str, name: &str, notes: &Option<String>, deleted: bool, created: i64, ts: i64) -> anyhow::Result<()> { store_collection(db, id, name, notes, deleted, created, ts) }
pub fn apply_items(db: &Db, id: &str, items: &[ItemRef], ts: i64) -> anyhow::Result<()> { store_items(db, id, items, ts).map(|_| ()) }

// ---- summary for the Overview ----

#[derive(Debug, Clone, Serialize)]
pub struct PlayEntry {
    pub game_id: Option<i64>, pub title: String, pub system: String, pub cover: Option<String>,
    pub status: Option<String>, pub score: Option<i64>, pub started: Option<String>, pub finished: Option<String>, pub hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaySummary {
    /// games per play status
    pub counts: BTreeMap<String, i64>,
    pub favorites: i64,
    pub average_score: Option<f64>,
    pub hours: f64,
    /// what you're playing now, most recently started first
    pub playing: Vec<PlayEntry>,
    /// recently finished, newest first
    pub finished: Vec<PlayEntry>,
}

pub fn play_summary(db: &Db) -> anyhow::Result<PlaySummary> {
    let mut q = db.conn().prepare(
        "SELECT p.status, p.score, p.started, p.finished, p.hours, p.favorite, g.id, COALESCE(g.title, json_extract(p.ref_json,'$.title')),
                COALESCE(g.system_id, json_extract(p.ref_json,'$.system')), COALESCE(json_extract(g.art_json,'$.cover'), json_extract(g.art_json,'$.icon'))
         FROM play p LEFT JOIN games g ON g.system_id || ':' || g.group_key = p.ref")?;
    let rows: Vec<(PlayEntry, bool)> = q.query_map([], |r| Ok((PlayEntry {
        status: r.get(0)?, score: r.get(1)?, started: r.get(2)?, finished: r.get(3)?, hours: r.get(4)?,
        game_id: r.get(6)?, title: r.get::<_, Option<String>>(7)?.unwrap_or_default(), system: r.get::<_, Option<String>>(8)?.unwrap_or_default(), cover: r.get(9)?,
    }, r.get::<_, i64>(5)? != 0)))?.collect::<Result<_, _>>()?;
    let mut counts = BTreeMap::new();
    for (e, _) in &rows { if let Some(s) = &e.status { *counts.entry(s.clone()).or_insert(0) += 1; } }
    let scores: Vec<i64> = rows.iter().filter_map(|(e, _)| e.score).collect();
    let mut playing: Vec<PlayEntry> = rows.iter().filter(|(e, _)| e.status.as_deref() == Some("playing")).map(|(e, _)| e.clone()).collect();
    playing.sort_by(|a, b| b.started.cmp(&a.started));
    let mut finished: Vec<PlayEntry> = rows.iter().filter(|(e, _)| matches!(e.status.as_deref(), Some("finished" | "completed"))).map(|(e, _)| e.clone()).collect();
    finished.sort_by(|a, b| b.finished.cmp(&a.finished));
    finished.truncate(8);
    Ok(PlaySummary {
        favorites: rows.iter().filter(|(_, f)| *f).count() as i64,
        average_score: (!scores.is_empty()).then(|| scores.iter().sum::<i64>() as f64 / scores.len() as f64),
        hours: rows.iter().filter_map(|(e, _)| e.hours).sum(),
        counts, playing, finished,
    })
}
