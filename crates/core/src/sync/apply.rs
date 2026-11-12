//! Records from the server into this library. The same rules as the server's: a manual
//! decision is replaced only by another, else the newer record wins.
use super::portable::{game_ref_in, Roots};
use super::record::{Body, Record};
use crate::config::Settings;
use crate::db::Db;
use crate::userdata::{GameRef, ItemRef};
use rusqlite::OptionalExtension;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// taken into the library
    Done,
    /// left as it is here (older, or not for this PC)
    Skipped,
    /// for a game or file this library doesn't have yet: kept, tried again after a scan
    Waiting,
}

/// The game with exactly this key here. (Not by title: a record for a key that no longer
/// exists, such as a renamed folder's, must not land on whichever game shares its name.)
pub fn local_game(db: &Db, roots: &Roots, g: &GameRef) -> anyhow::Result<Option<i64>> {
    let Some(g) = game_ref_in(roots, g) else { return Ok(None) };
    Ok(db.conn().query_row("SELECT id FROM games WHERE system_id=?1 AND group_key=?2", (&g.system, &g.key), |r| r.get(0)).optional()?)
}

/// When this library last had the record for a key (sent or received), ms.
fn known_ts(db: &Db, key: &str) -> anyhow::Result<Option<i64>> {
    Ok(db.conn().query_row("SELECT ts FROM sync_sent WHERE key=?1", [key], |r| r.get(0)).optional()?)
}

pub fn apply(db: &Db, roots: &Roots, settings: &mut Settings, cache_dir: &Path, r: &Record) -> anyhow::Result<Applied> {
    let c = db.conn();
    let older = |key: &str| -> anyhow::Result<bool> { Ok(known_ts(db, key)?.is_some_and(|t| t > r.ts)) };
    Ok(match &r.body {
        // what the library has is this PC's to say
        Body::Game { game, .. } => if local_game(db, roots, game)?.is_some() { Applied::Done } else { Applied::Waiting },
        Body::Fingerprint { root, path, size, mtime_s, crc32, sha1, md5, headerless_crc32, headerless_sha1 } => {
            let Some(abs) = roots.abs(root, path) else { return Ok(Applied::Skipped) };
            let row: Option<(i64, i64, i64)> = c.query_row("SELECT id, size, mtime FROM files WHERE path=?1", [crate::util::p2s(&abs)], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?))).optional()?;
            match row {
                None => Applied::Waiting,
                // the file changed since: it's fingerprinted afresh
                Some((_, sz, mt)) if sz != *size || super::export::mtime_secs(mt) != *mtime_s => Applied::Skipped,
                Some((id, _, _)) => {
                    c.execute("UPDATE files SET crc32=?1, sha1=?2, md5=?3, headerless_crc32=?4, headerless_sha1=?5, hash_key=size || ':' || mtime WHERE id=?6",
                        (crc32, sha1, md5, headerless_crc32, headerless_sha1, id))?;
                    Applied::Done
                }
            }
        }
        Body::Meta { game, state, meta, manual, cover } => {
            let Some(gid) = local_game(db, roots, game)? else { return Ok(Applied::Waiting) };
            let (sys, key, cur): (String, String, Option<String>) = c.query_row("SELECT system_id, group_key, meta_json FROM games WHERE id=?1", [gid], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)))?;
            let mine = cur.and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok()).is_some_and(|m| m["chosen"].as_bool() == Some(true))
                || crate::skips::keys(db, "match")?.contains(&format!("{sys}:{key}"));
            // manual beats automatic whatever the times; among equals the newer wins
            if (mine && !manual) || (older(&r.body.key())? && !(*manual && !mine)) { return Ok(Applied::Skipped); }
            c.execute("UPDATE games SET meta_state=?1, meta_json=?2 WHERE id=?3", (state, meta.as_ref().map(|m| m.to_string()), gid))?;
            if let Some(p) = cover.as_deref().and_then(|s| cover_file(cache_dir, s)) { crate::metadata::boxart::set_cover(db, gid, &p)?; }
            Applied::Done
        }
        Body::Skip { skip_kind, key, label, system, removed } => {
            let Some(k) = roots.to_local(key) else { return Ok(Applied::Skipped) };
            if older(&r.body.key())? { return Ok(Applied::Skipped); }
            if *removed { crate::skips::unskip(db, skip_kind, &k)?; }
            else { crate::skips::skip(db, skip_kind, &k, &roots.to_local(label).unwrap_or_else(|| label.clone()), system.as_deref())?; }
            Applied::Done
        }
        Body::Play { game, info } => {
            crate::userdata::apply_play(db, &game_ref_in(roots, game).unwrap_or_else(|| game.clone()), info, r.ts)?;
            Applied::Done
        }
        Body::Collection { id, name, notes, deleted, created } => { crate::userdata::apply_collection(db, id, name, notes, *deleted, *created, r.ts)?; Applied::Done }
        Body::CollectionItems { id, items } => {
            let items: Vec<ItemRef> = items.iter().map(|i| ItemRef { game: game_ref_in(roots, &i.game).unwrap_or_else(|| i.game.clone()), note: i.note.clone() }).collect();
            crate::userdata::apply_items(db, id, &items, r.ts)?;
            Applied::Done
        }
        Body::Op { batch_id, op, src, dst, label, at } => {
            let (src, dst) = (src.as_deref().and_then(|s| roots.to_local(s)), dst.as_deref().and_then(|s| roots.to_local(s)));
            c.execute("INSERT INTO ops(batch_id, machine_id, kind, src, dst, created_at, label) SELECT ?1,?2,?3,?4,?5,?6,?7
                WHERE NOT EXISTS (SELECT 1 FROM ops WHERE batch_id=?1 AND kind=?3 AND src IS ?4 AND dst IS ?5)", (batch_id, &r.machine, op, &src, &dst, at, label))?;
            Applied::Done
        }
        // kinds this version doesn't know (from newer devices): kept on the server, skipped here
        Body::Unknown => Applied::Skipped,
        Body::ShelfCopy { copy } => {
            // a game in a folder not mapped here: it goes on the game for its release instead
            let game = game_ref_in(roots, &copy.game).unwrap_or_else(|| GameRef { key: String::new(), ..copy.game.clone() });
            let copy = crate::shelf::Copy { game, ..copy.clone() };
            crate::shelf::apply(db, &copy, r.ts)?;
            if copy.deleted { crate::shelf::drop_if_empty(db, &copy.id)?; }
            Applied::Done
        }
        Body::Settings { shared } => {
            if older("settings")? { return Ok(Applied::Skipped); }
            settings.apply_shared(shared);
            Applied::Done
        }
    })
}

/// A synced cover on disk, by its sha256.
pub fn cover_file(cache_dir: &Path, sha: &str) -> Option<std::path::PathBuf> {
    let d = super::cover_dir(cache_dir);
    ["jpg", "png"].iter().map(|e| d.join(format!("{sha}.{e}"))).find(|p| p.exists())
}

/// Keep a cover from the server under its sha256.
pub fn store_cover(cache_dir: &Path, sha: &str, bytes: &[u8]) -> anyhow::Result<std::path::PathBuf> {
    let ext = if bytes.starts_with(b"\x89PNG") { "png" } else { "jpg" };
    let d = super::cover_dir(cache_dir);
    std::fs::create_dir_all(&d)?;
    let p = d.join(format!("{sha}.{ext}"));
    std::fs::write(p.with_extension(format!("{ext}.part")), bytes)?;
    std::fs::rename(p.with_extension(format!("{ext}.part")), &p)?;
    Ok(p)
}
