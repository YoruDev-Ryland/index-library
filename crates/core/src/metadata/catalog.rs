//! Every game IGDB lists for a console, kept locally and matched here with `similarity`: the
//! right game is found even when IGDB's word search wouldn't return it for the name searched.
use super::{normalize, score_for, MetaCandidate};
use crate::db::Db;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A catalog is read again after this long.
pub const REFRESH_SECS: i64 = 30 * 86_400;
/// Consoles listing more games than this (PC) are left to title search.
pub const MAX_ENTRIES: usize = 50_000;

struct Entry { id: String, name: String, aliases: Vec<String> }
struct Index { generation: String, entries: Vec<Entry>, by_word: HashMap<String, Vec<usize>> }

static INDEXES: Mutex<Option<HashMap<String, Arc<Index>>>> = Mutex::new(None);

/// Words too common to tell games apart.
const COMMON: &[&str] = &["the", "of", "and", "a", "an", "in", "no", "to", "de", "on", "for", "game", "games"];

fn words(s: &str) -> Vec<String> {
    normalize(s).split(' ').filter(|w| !w.is_empty() && !COMMON.contains(w)).map(String::from).collect()
}

/// Store a console's catalog, replacing the one before.
pub fn store(db: &Db, system_id: &str, source: &str, entries: &[super::igdb::CatalogEntry]) -> anyhow::Result<()> {
    let tx = db.conn().unchecked_transaction()?;
    tx.execute("DELETE FROM catalog WHERE system_id=?1 AND source=?2", (system_id, source))?;
    {
        let mut q = tx.prepare("INSERT OR REPLACE INTO catalog(system_id, source, id, name, aliases_json, year) VALUES(?1,?2,?3,?4,?5,?6)")?;
        for e in entries {
            q.execute((system_id, source, e.id.to_string(), &e.name, (!e.aliases.is_empty()).then(|| serde_json::to_string(&e.aliases).unwrap_or_default()), e.year))?;
        }
    }
    tx.execute("INSERT INTO catalogs(system_id, source, fetched_at, entries, generation) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(system_id, source) DO UPDATE SET fetched_at=excluded.fetched_at, entries=excluded.entries, generation=excluded.generation",
        (system_id, source, crate::db::now(), entries.len() as i64, uuid::Uuid::new_v4().to_string()))?;
    tx.commit()?;
    Ok(())
}

/// When a console's catalog was last read (None: never).
pub fn fetched_at(db: &Db, system_id: &str, source: &str) -> Option<i64> {
    db.conn().query_row("SELECT fetched_at FROM catalogs WHERE system_id=?1 AND source=?2", (system_id, source), |r| r.get(0)).ok()
}

fn index(db: &Db, system_id: &str) -> anyhow::Result<Option<Arc<Index>>> {
    let Ok(generation) = db.conn().query_row("SELECT COALESCE(generation, fetched_at) FROM catalogs WHERE system_id=?1 AND source='igdb'", [system_id], |r| r.get::<_, rusqlite::types::Value>(0)) else { return Ok(None) };
    let generation = match generation { rusqlite::types::Value::Text(t) => t, v => format!("{v:?}") };
    let mut g = INDEXES.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(i) = map.get(system_id) { if i.generation == generation { return Ok(Some(i.clone())); } }
    let mut q = db.conn().prepare("SELECT id, name, aliases_json FROM catalog WHERE system_id=?1 AND source='igdb'")?;
    let entries: Vec<Entry> = q.query_map([system_id], |r| Ok(Entry { id: r.get(0)?, name: r.get(1)?,
        aliases: r.get::<_, Option<String>>(2)?.and_then(|a| serde_json::from_str(&a).ok()).unwrap_or_default() }))?.collect::<Result<_, _>>()?;
    let mut by_word: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        let mut ws: Vec<String> = std::iter::once(&e.name).chain(&e.aliases).flat_map(|n| words(n)).collect();
        ws.sort(); ws.dedup();
        for w in ws { by_word.entry(w).or_default().push(i); }
    }
    let idx = Arc::new(Index { generation, entries, by_word });
    map.insert(system_id.to_string(), idx.clone());
    Ok(Some(idx))
}

/// The console's games most like these names (best first, at most `n`, none below `min`): the
/// ones sharing a word with a name, scored like any candidate (other names included).
pub fn best(db: &Db, system_id: &str, names: &[&str], n: usize, min: f32) -> anyhow::Result<Vec<(String, f32)>> {
    let Some(idx) = index(db, system_id)? else { return Ok(vec![]) };
    let mut seen: Vec<usize> = names.iter().flat_map(|n| words(n)).filter_map(|w| idx.by_word.get(&w)).flatten().copied().collect();
    seen.sort_unstable(); seen.dedup();
    let mut scored: Vec<(String, f32)> = seen.into_iter().map(|i| {
        let e = &idx.entries[i];
        let c = MetaCandidate { source: "igdb".into(), title: e.name.clone(), aliases: e.aliases.clone(), ..Default::default() };
        (e.id.clone(), score_for(names, &c))
    }).filter(|(_, s)| *s >= min).collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(n);
    Ok(scored)
}
