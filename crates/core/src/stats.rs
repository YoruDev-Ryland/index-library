//! Library statistics for the dashboard.
use crate::db::Db;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub id: String,
    pub name: String,
    pub games: u64,
    pub files: u64,
    pub bytes: u64,
    pub hashed: u64,
    pub verified: u64,
    /// "key=value" → number of games
    pub status_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats { pub systems: Vec<SystemStats>, pub archive_count: u64, pub archive_bytes: u64, pub dupe_bytes: u64 }

pub fn stats(db: &Db, settings: &crate::config::Settings) -> anyhow::Result<Stats> {
    let c = db.conn();
    let mut q = c.prepare("SELECT system_id, COUNT(DISTINCT game_id), COUNT(*), COALESCE(SUM(size),0), SUM(sha1 IS NOT NULL), SUM(dat_status='verified') FROM files GROUP BY system_id ORDER BY system_id")?;
    let mut systems: Vec<SystemStats> = q.query_map([], |r| Ok(SystemStats {
        id: r.get(0)?, name: String::new(), games: r.get::<_, i64>(1)? as u64, files: r.get::<_, i64>(2)? as u64, bytes: r.get::<_, i64>(3)? as u64,
        hashed: r.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64, verified: r.get::<_, Option<i64>>(5)?.unwrap_or(0) as u64, status_counts: BTreeMap::new(),
    }))?.collect::<Result<_, _>>()?;
    let mut sq = c.prepare("SELECT s.key || '=' || s.value, COUNT(*) FROM status s JOIN games g ON g.id=s.game_id WHERE g.system_id=?1 GROUP BY 1")?;
    for s in systems.iter_mut() {
        s.name = crate::systems::by_id(&s.id).map(|d| d.name.to_string()).unwrap_or_else(|| s.id.clone());
        s.status_counts = sq.query_map([&s.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?.collect::<Result<_, _>>()?;
    }
    let (archive_count, archive_bytes): (i64, i64) = c.query_row("SELECT COUNT(*), COALESCE(SUM(size),0) FROM archive", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let dupe_bytes = crate::dupes::find_dupes_with(db, None, &settings.region_order, settings.one_game_one_rom)?.iter().map(|g| g.reclaimable).sum();
    Ok(Stats { systems, archive_count: archive_count as u64, archive_bytes: archive_bytes as u64, dupe_bytes })
}
