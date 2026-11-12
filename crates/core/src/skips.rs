//! Suggestions you've said no to, so they aren't made again (see db/skipped.sql).
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skipped { pub kind: String, pub key: String, pub label: String, pub system_id: Option<String>, pub at: i64 }

pub fn skip(db: &Db, kind: &str, key: &str, label: &str, system_id: Option<&str>) -> anyhow::Result<()> {
    db.conn().execute("INSERT INTO skipped(kind, key, label, system_id, at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(kind,key) DO UPDATE SET label=excluded.label, at=excluded.at",
        (kind, key, label, system_id, crate::db::now()))?;
    Ok(())
}
pub fn unskip(db: &Db, kind: &str, key: &str) -> anyhow::Result<()> {
    db.conn().execute("DELETE FROM skipped WHERE kind=?1 AND key=?2", (kind, key))?;
    Ok(())
}
pub fn keys(db: &Db, kind: &str) -> anyhow::Result<HashSet<String>> {
    let mut q = db.conn().prepare("SELECT key FROM skipped WHERE kind=?1")?;
    let v = q.query_map([kind], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}
pub fn list(db: &Db, kind: &str) -> anyhow::Result<Vec<Skipped>> {
    let mut q = db.conn().prepare("SELECT kind, key, label, system_id, at FROM skipped WHERE kind=?1 ORDER BY at DESC")?;
    let v = q.query_map([kind], |r| Ok(Skipped { kind: r.get(0)?, key: r.get(1)?, label: r.get(2)?, system_id: r.get(3)?, at: r.get(4)? }))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// A game's match is skipped by its system and group key, which stay the same across rescans.
pub fn game_key(db: &Db, game_id: i64) -> anyhow::Result<Option<(String, String, String)>> {
    use rusqlite::OptionalExtension;
    Ok(db.conn().query_row("SELECT system_id || ':' || group_key, title, system_id FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?)
}
