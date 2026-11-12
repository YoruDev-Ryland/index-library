//! What this library knows, as records, and which of them the server hasn't had yet. Every
//! record is derived from the database as it is now, so whatever changed it (a lookup, a pick,
//! an edit, another PC's journal) is sent without each of those having to say so.
use super::portable::game_ref_out;
use super::record::{hex, Body, Display, Record};
use crate::config::Settings;
use crate::db::Db;
use crate::userdata::{GameRef, ItemRef, PlayInfo};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The "folder" of games you only own physically: always listed, so their removal is reported.
pub const SHELF_ROOT: &str = "shelf";

/// Grid covers go to the server no taller than this.
pub const COVER_HEIGHT: u32 = 600;

/// A record body as the library has it now, when it was made, and the folder it belongs to.
#[derive(Debug, Clone)]
pub struct Derived { pub body: Body, pub ts: i64, pub root: Option<String> }

fn json<T: serde::de::DeserializeOwned>(s: Option<String>) -> Option<T> { s.and_then(|s| serde_json::from_str(&s).ok()) }

/// Every record this library implies.
pub fn current(db: &Db, s: &Settings, cache_dir: &Path) -> anyhow::Result<Vec<Derived>> {
    let roots = super::roots_of(s);
    let now = crate::shared::now_ms();
    let c = db.conn();
    let mut out = Vec::new();
    let skipped_matches = crate::skips::keys(db, "match")?;

    // games, what they were identified as, and their covers
    let mut q = c.prepare("SELECT g.id, g.system_id, g.group_key, g.title, g.parsed_json, g.meta_state, g.meta_json, json_extract(g.art_json,'$.cover'),
        (SELECT path FROM files f WHERE f.game_id=g.id ORDER BY path LIMIT 1) FROM games g")?;
    type Row = (i64, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?.collect::<Result<_, _>>()?;
    let mut ids_q = c.prepare("SELECT DISTINCT json_extract(probe_json,'$.id') FROM files WHERE game_id=?1 AND json_extract(probe_json,'$.id') IS NOT NULL ORDER BY 1 LIMIT 8")?;
    for (id, system, key, title, parsed, state, meta, cover, path) in rows {
        // a game only on the shelf belongs to the shelf, which is always there to look at
        let root = match path.as_deref() { Some(p) => match roots.rel(Path::new(p)) { Some((r, _)) => r, None => continue }, None => SHELF_ROOT.to_string() };
        let ids: Vec<String> = ids_q.query_map([id], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let game = game_ref_out(&roots, &GameRef { system: system.clone(), key: key.clone(), title: title.clone(), ids });
        let regions: Vec<String> = json::<serde_json::Value>(parsed).and_then(|p| serde_json::from_value(p["regions"].clone()).ok()).unwrap_or_default();
        let meta: Option<serde_json::Value> = json(meta);
        let cover = cover.as_deref().and_then(|p| cover_sha(db, cache_dir, Path::new(p)));
        let display = (state.as_deref() == Some("matched")).then(|| meta.as_ref()).flatten().map(|m| Display {
            title: m["title"].as_str().unwrap_or(&title).to_string(),
            year: m["release"].as_str().and_then(|r| r.get(..4)).and_then(|y| y.parse().ok()),
            developer: m["developer"].as_str().map(String::from), publisher: m["publisher"].as_str().map(String::from),
            genres: serde_json::from_value(m["genres"].clone()).unwrap_or_default(), description: m["description"].as_str().map(String::from),
            cover: cover.clone(),
        }).or_else(|| cover.clone().map(|c| Display { title: title.clone(), cover: Some(c), ..Default::default() }));
        out.push(Derived { body: Body::Game { game: game.clone(), console: system.clone(), file_title: title.clone(), regions, removed: false, display }, ts: now, root: Some(root.clone()) });
        if let Some(state) = state.filter(|s| matches!(s.as_str(), "matched" | "none" | "review")) {
            // yours: a pick, "No match", "Choose again", or a look-up you asked for
            let flag = |k: &str| meta.as_ref().is_some_and(|m| m[k].as_bool() == Some(true) || m[k].as_i64() == Some(1));
            let manual = flag("chosen") || flag("again") || flag("asked") || (state == "none" && skipped_matches.contains(&format!("{system}:{key}")));
            out.push(Derived { body: Body::Meta { game, state, meta, manual, cover }, ts: now, root: Some(root) });
        }
    }

    // fingerprints
    let mut q = c.prepare("SELECT path, size, mtime, crc32, sha1, md5, headerless_crc32, headerless_sha1 FROM files WHERE hash_key IS NOT NULL AND is_dir=0")?;
    let rows: Vec<(String, i64, i64, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>)> =
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?.collect::<Result<_, _>>()?;
    for (path, size, mtime, crc32, sha1, md5, headerless_crc32, headerless_sha1) in rows {
        let Some((root, rel)) = roots.rel(Path::new(&path)) else { continue };
        out.push(Derived { body: Body::Fingerprint { root: root.clone(), path: rel, size, mtime_s: mtime_secs(mtime), crc32, sha1, md5, headerless_crc32, headerless_sha1 }, ts: now, root: Some(root) });
    }

    // suggestions said no to
    for sk in ["match", "junk", "dupe", "layout", "rename"] {
        for x in crate::skips::list(db, sk)? {
            out.push(Derived { body: Body::Skip { skip_kind: x.kind, key: roots.to_portable(&x.key), label: roots.to_portable(&x.label), system: x.system_id, removed: false }, ts: x.at * 1000, root: None });
        }
    }

    // play status and collections
    let mut q = c.prepare("SELECT ref_json, status, score, notes, started, finished, hours, favorite, updated_at FROM play")?;
    let rows: Vec<(String, PlayInfo, i64)> = q.query_map([], |r| Ok((r.get(0)?, PlayInfo {
        status: r.get(1)?, score: r.get::<_, Option<i64>>(2)?.map(|v| v.clamp(0, 10) as u8), notes: r.get(3)?, started: r.get(4)?, finished: r.get(5)?, hours: r.get(6)?, favorite: r.get::<_, i64>(7)? != 0,
    }, r.get(8)?)))?.collect::<Result<_, _>>()?;
    for (rj, info, ts) in rows {
        let Ok(g) = serde_json::from_str::<GameRef>(&rj) else { continue };
        out.push(Derived { body: Body::Play { game: game_ref_out(&roots, &g), info }, ts, root: None });
    }
    let mut q = c.prepare("SELECT id, name, notes, deleted, created_at, updated_at, items_updated_at FROM collections")?;
    let rows: Vec<(String, String, Option<String>, bool, i64, i64, i64)> = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?, r.get(5)?, r.get(6)?)))?.collect::<Result<_, _>>()?;
    let mut iq = c.prepare("SELECT ref_json, note FROM collection_items WHERE collection_id=?1 ORDER BY pos")?;
    for (id, name, notes, deleted, created, updated, items_at) in rows {
        // a placeholder for items that arrived before their collection: not ours to describe
        if updated > 0 { out.push(Derived { body: Body::Collection { id: id.clone(), name, notes, deleted, created }, ts: updated, root: None }); }
        if items_at > 0 {
            let items: Vec<ItemRef> = iq.query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?.collect::<Result<Vec<_>, _>>()?
                .into_iter().map(|(j, note)| ItemRef { game: game_ref_out(&roots, &serde_json::from_str(&j).unwrap_or_default()), note }).collect();
            out.push(Derived { body: Body::CollectionItems { id, items }, ts: items_at, root: None });
        }
    }

    // History
    let mut q = c.prepare("SELECT batch_id, kind, src, dst, label, created_at FROM ops WHERE undone=0")?;
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, i64)> = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
    for (batch_id, op, src, dst, label, at) in rows {
        out.push(Derived { body: Body::Op { batch_id, op, src: src.map(|p| roots.to_portable(&p)), dst: dst.map(|p| roots.to_portable(&p)), label, at }, ts: at * 1000, root: None });
    }

    // physical copies, removed ones too (so other devices remove them)
    for (mut copy, ts) in crate::shelf::all(db)? {
        copy.game = game_ref_out(&roots, &copy.game);
        out.push(Derived { body: Body::ShelfCopy { copy }, ts, root: None });
    }

    out.push(Derived { body: Body::Settings { shared: s.shared_json() }, ts: now, root: None });
    Ok(out)
}

/// Local mtimes are nanoseconds; records compare whole seconds (mounts differ in precision).
pub fn mtime_secs(m: i64) -> i64 { if m > 100_000_000_000_000 { m / 1_000_000_000 } else if m > 100_000_000_000 { m / 1000 } else { m } }

/// The records the server hasn't had: changed ones, games gone from folders that could be
/// listed (`listed`: root ids), and suggestions brought back.
pub fn pending(db: &Db, machine: &str, now: Vec<Derived>, listed: &HashSet<String>) -> anyhow::Result<Vec<(Record, Option<String>)>> {
    let mut sent: HashMap<String, (String, Option<String>, Option<String>)> = HashMap::new();
    {
        let mut q = db.conn().prepare("SELECT key, hash, root, body FROM sync_sent")?;
        for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?)))? {
            let (k, h, root, body) = r?;
            sent.insert(k, (h, root, body));
        }
    }
    let mut waiting: HashMap<String, (String, i64)> = HashMap::new();
    {
        let mut q = db.conn().prepare("SELECT key, hash, ts FROM sync_pending")?;
        for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? { let (k, h, t) = r?; waiting.insert(k, (h, t)); }
    }
    // a change sent before without an answer goes again as the same record (same time, same id)
    let stable = |key: &str, hash: &str, ts: i64| -> anyhow::Result<i64> {
        if let Some((h, t)) = waiting.get(key) { if h == hash { return Ok(*t); } }
        db.conn().execute("INSERT INTO sync_pending(key, hash, ts) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET hash=excluded.hash, ts=excluded.ts", (key, hash, ts))?;
        Ok(ts)
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for d in now {
        let key = d.body.key();
        let hash = d.body.content_hash();
        let unchanged = sent.get(&key).is_some_and(|(h, _, _)| *h == hash);
        if !unchanged { let ts = stable(&key, &hash, d.ts)?; out.push((Record::new(machine, ts, d.body), d.root)); }
        seen.insert(key);
    }
    let ts = crate::shared::now_ms();
    for (key, (hash, root, body)) in &sent {
        if seen.contains(key) { continue; }
        let Some(body) = body.as_deref().and_then(|b| serde_json::from_str::<Body>(b).ok()) else { continue };
        let gone = match body {
            Body::Game { game, console, file_title, regions, display, .. } if root.as_ref().is_some_and(|r| listed.contains(r)) =>
                Body::Game { game, console, file_title, regions, display, removed: true },
            Body::Skip { skip_kind, key, label, system, .. } => Body::Skip { skip_kind, key, label, system, removed: true },
            _ => continue,
        };
        let h = gone.content_hash();
        if h != *hash { let ts = stable(key, &h, ts)?; out.push((Record::new(machine, ts, gone), root.clone())); }
    }
    Ok(out)
}

/// Remember records as the server's (sent, or received from it).
pub fn mark_sent(db: &Db, recs: &[(Record, Option<String>)]) -> anyhow::Result<()> {
    // inside a caller's transaction, or in one of its own
    let tx = if db.conn().is_autocommit() { Some(db.conn().unchecked_transaction()?) } else { None };
    {
        let mut q = db.conn().prepare("INSERT INTO sync_sent(key, hash, ts, root, body) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(key) DO UPDATE SET hash=excluded.hash, ts=excluded.ts, root=COALESCE(excluded.root, sync_sent.root), body=excluded.body")?;
        for (r, root) in recs {
            let keep = matches!(r.body, Body::Game { .. } | Body::Skip { .. });
            q.execute((r.body.key(), r.body.content_hash(), r.ts, root, keep.then(|| serde_json::to_string(&r.body).unwrap_or_default())))?;
            db.conn().execute("DELETE FROM sync_pending WHERE key=?1", [r.body.key()])?;
        }
    }
    if let Some(tx) = tx { tx.commit()?; }
    Ok(())
}

/// A cover at grid size, and its sha256 (the name it's stored under on the server).
pub fn cover_blob(path: &Path) -> anyhow::Result<(String, Vec<u8>)> {
    let bytes = std::fs::read(path)?;
    let (data, _) = crate::metadata::boxart::encode(&bytes, COVER_HEIGHT)?;
    Ok((hex(&Sha256::digest(&data)), data))
}

/// The sha256 a cover is (or will be) uploaded under; remembered per file and mtime. A cover
/// that came from the server is named by it already.
pub fn cover_sha(db: &Db, cache_dir: &Path, path: &Path) -> Option<String> {
    if path.starts_with(super::cover_dir(cache_dir)) { return path.file_stem().map(|s| s.to_string_lossy().into_owned()); }
    let mtime = std::fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let p = crate::util::p2s(path);
    let c = db.conn();
    if let Some(sha) = c.query_row("SELECT sha FROM sync_covers WHERE path=?1 AND mtime=?2", (&p, mtime), |r| r.get::<_, String>(0)).optional().ok().flatten() { return Some(sha); }
    let (sha, _) = cover_blob(path).ok()?;
    let _ = c.execute("INSERT INTO sync_covers(path, mtime, sha) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime, sha=excluded.sha", (&p, mtime, &sha));
    Some(sha)
}
