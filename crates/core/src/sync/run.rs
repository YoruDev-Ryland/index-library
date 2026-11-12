//! One sync with the server: send what changed here, then take what changed elsewhere.
use super::apply::{apply, cover_file, store_cover, Applied};
use super::client::Client;
use super::export::{self, cover_blob};
use super::record::{Body, Record};
use crate::config::{RootMap, Settings};
use crate::db::Db;
use rusqlite::OptionalExtension;
use std::collections::HashSet;
use std::path::Path;

/// Records sent per request.
pub const BATCH: usize = 500;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SyncReport {
    pub pushed: usize,
    pub pulled: usize,
    pub applied: usize,
    pub waiting: usize,
    pub covers_up: usize,
    pub covers_down: usize,
    /// how far this PC's clock is from the server's, ms (positive: ahead)
    pub clock_skew_ms: i64,
    /// settings from another device were taken
    pub settings_changed: bool,
    /// what was sent and received, by kind of record ("Play", "Meta", …)
    pub sent: std::collections::BTreeMap<String, usize>,
    pub received: std::collections::BTreeMap<String, usize>,
}

/// One entry in the sync activity list.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    /// when it ended, ms
    pub at: i64,
    pub sent: std::collections::BTreeMap<String, usize>,
    pub received: std::collections::BTreeMap<String, usize>,
    pub covers_up: usize,
    pub covers_down: usize,
    pub error: Option<String>,
}

/// Entries kept in the activity list.
pub const ACTIVITY: usize = 30;

/// Remember how a sync ended (kept across restarts): the last result, and an activity entry
/// when something moved or it failed.
pub fn record(db: &Db, r: &Result<SyncReport, String>) -> anyhow::Result<()> {
    let at = crate::shared::now_ms();
    state_set(db, "last", Some(&serde_json::to_string(&serde_json::json!({"at": at, "report": r.as_ref().ok(), "error": r.as_ref().err()}))?))?;
    let a = match r {
        Ok(rep) if rep.sent.is_empty() && rep.received.is_empty() && rep.covers_up == 0 && rep.covers_down == 0 => return Ok(()),
        Ok(rep) => Activity { at, sent: rep.sent.clone(), received: rep.received.clone(), covers_up: rep.covers_up, covers_down: rep.covers_down, error: None },
        Err(e) => Activity { at, error: Some(e.clone()), ..Default::default() },
    };
    let mut list: Vec<Activity> = state_get(db, "activity")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    // the same failure again and again is one entry, kept current
    if a.error.is_some() && list.first().is_some_and(|x| x.error == a.error) { list[0].at = at; } else { list.insert(0, a); }
    list.truncate(ACTIVITY);
    state_set(db, "activity", Some(&serde_json::to_string(&list)?))
}

/// What the last sync left: (when, report or error).
pub fn last(db: &Db) -> Option<(i64, Result<SyncReport, String>)> {
    let v: serde_json::Value = serde_json::from_str(&state_get(db, "last").ok()??).ok()?;
    let at = v["at"].as_i64()?;
    Some((at, match v["error"].as_str() { Some(e) => Err(e.to_string()), None => Ok(serde_json::from_value(v["report"].clone()).ok()?) }))
}

pub fn activity(db: &Db) -> Vec<Activity> { state_get(db, "activity").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default() }

/// Is there anything to do: changes here not yet sent, or records on the server not yet taken?
pub fn has_work(db: &Db, client: &Client, machine: &str, s: &Settings, cache_dir: &Path) -> anyhow::Result<bool> {
    let n = export::pending(db, machine, export::current(db, s, cache_dir)?, &listed_roots(s))?.iter()
        .filter(|(r, _)| state_get(db, "settings_ok").ok().flatten().is_some() || !matches!(r.body, Body::Settings { .. })).count();
    state_set(db, "pending", Some(&n.to_string()))?;
    if n > 0 { return Ok(true); }
    let after: i64 = state_get(db, "cursor")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok(client.pull(after, 1)?.records.iter().any(|x| x.record.machine != machine))
}

pub fn state_get(db: &Db, name: &str) -> anyhow::Result<Option<String>> {
    Ok(db.conn().query_row("SELECT value FROM sync_state WHERE name=?1", [name], |r| r.get(0)).optional()?)
}
pub fn state_set(db: &Db, name: &str, value: Option<&str>) -> anyhow::Result<()> {
    match value {
        Some(v) => db.conn().execute("INSERT INTO sync_state(name, value) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET value=excluded.value", (name, v))?,
        None => db.conn().execute("DELETE FROM sync_state WHERE name=?1", [name])?,
    };
    Ok(())
}

/// Give every library folder a sync id (new folders get one the first time they're seen).
pub fn ensure_root_ids(s: &mut Settings) -> bool {
    let mut changed = false;
    for r in s.library_roots.clone() {
        if !s.sync.roots.iter().any(|m| m.path == r) {
            s.sync.roots.push(RootMap { id: uuid::Uuid::new_v4().to_string(), origin: crate::util::p2s(&r), path: r });
            changed = true;
        }
    }
    changed
}

/// The folders whose games can be said to be gone: mapped here and there to look at.
pub fn listed_roots(s: &Settings) -> HashSet<String> {
    s.sync.roots.iter().filter(|r| s.library_roots.contains(&r.path) && r.path.is_dir()).map(|r| r.id.clone())
        .chain(std::iter::once(export::SHELF_ROOT.to_string())).collect()
}

/// Upload the covers these records name that the server doesn't have.
fn upload_covers(db: &Db, client: &Client, cache_dir: &Path, recs: &[(Record, Option<String>)]) -> anyhow::Result<usize> {
    let mut shas: Vec<&str> = recs.iter().filter_map(|(r, _)| match &r.body { Body::Meta { cover, .. } => cover.as_deref(), Body::Game { display, .. } => display.as_ref().and_then(|d| d.cover.as_deref()), _ => None }).collect();
    shas.sort(); shas.dedup();
    let mut n = 0;
    for sha in shas {
        if client.has_blob(sha)? { continue; }
        let data = if let Some(p) = cover_file(cache_dir, sha) { std::fs::read(p)? } else {
            let path: Option<String> = db.conn().query_row("SELECT path FROM sync_covers WHERE sha=?1 LIMIT 1", [sha], |r| r.get(0)).optional()?;
            let Some(path) = path else { continue };
            let (s2, data) = cover_blob(Path::new(&path))?;
            if s2 != sha { continue; } // changed since: the next sync names it afresh
            data
        };
        client.put_blob(sha, data)?;
        n += 1;
    }
    Ok(n)
}

/// Apply one record from elsewhere; covers it names are fetched first.
fn take(db: &Db, client: Option<&Client>, s: &mut Settings, cache_dir: &Path, r: &Record, rep: &mut SyncReport) -> anyhow::Result<Applied> {
    if let (Some(cl), Body::Meta { cover: Some(sha), .. }) = (client, &r.body) {
        if cover_file(cache_dir, sha).is_none() {
            if let Some(b) = cl.get_blob(sha)? { store_cover(cache_dir, sha, &b)?; rep.covers_down += 1; }
        }
    }
    let roots = super::roots_of(s);
    let before = s.shared_json();
    let a = apply(db, &roots, s, cache_dir, r)?;
    if s.shared_json() != before { rep.settings_changed = true; }
    let root = match &r.body { Body::Fingerprint { root, .. } => Some(root.clone()), _ => None };
    match a {
        Applied::Done => {
            rep.applied += 1;
            *rep.received.entry(r.body.kind().to_string()).or_default() += 1;
            // a game this PC has: it now knows it in this form
            let root = root.or_else(|| match &r.body {
                Body::Game { game, .. } => super::apply::local_game(db, &roots, game).ok().flatten()
                    .and_then(|gid| db.conn().query_row("SELECT path FROM files WHERE game_id=?1 ORDER BY path LIMIT 1", [gid], |x| x.get::<_, String>(0)).ok())
                    .and_then(|p| roots.rel(Path::new(&p))).map(|(id, _)| id),
                _ => None,
            });
            // a copy is remembered as this PC will describe it (its game in this PC's terms), so
            // it isn't sent back as a change
            let mine = match &r.body {
                Body::ShelfCopy { copy } => crate::shelf::get(db, &copy.id)?.map(|mut c| {
                    c.game = super::portable::game_ref_out(&roots, &c.game);
                    Record { body: Body::ShelfCopy { copy: c }, ..r.clone() }
                }),
                _ => None,
            };
            export::mark_sent(db, &[(mine.unwrap_or_else(|| r.clone()), root)])?;
            db.conn().execute("DELETE FROM sync_inbox WHERE key=?1", [r.body.key()])?;
        }
        Applied::Waiting => {
            rep.waiting += 1;
            db.conn().execute("INSERT INTO sync_inbox(key, json) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET json=excluded.json", (r.body.key(), serde_json::to_string(r)?))?;
        }
        Applied::Skipped => { db.conn().execute("DELETE FROM sync_inbox WHERE key=?1", [r.body.key()])?; }
    }
    Ok(a)
}

/// Try again the records waiting for games or files this library didn't have (after a scan).
pub fn apply_inbox(db: &Db, client: Option<&Client>, s: &mut Settings, cache_dir: &Path, rep: &mut SyncReport) -> anyhow::Result<()> {
    let mut q = db.conn().prepare("SELECT json FROM sync_inbox")?;
    let recs: Vec<String> = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    drop(q);
    rep.waiting = 0;
    // fingerprints first: matches and the rest are by game, and games can depend on them
    let mut recs: Vec<Record> = recs.into_iter().filter_map(|j| serde_json::from_str(&j).ok()).collect();
    recs.sort_by_key(|r| !matches!(r.body, Body::Fingerprint { .. }));
    for r in &recs { take(db, client, s, cache_dir, r, rep)?; }
    Ok(())
}

/// Send what changed here (in batches, each remembered once the server has it), then take
/// what's new from other devices.
pub fn sync_once(db: &Db, client: &Client, machine: &str, s: &mut Settings, cache_dir: &Path, progress: &dyn Fn(u64, u64, &str)) -> anyhow::Result<SyncReport> {
    let mut rep = SyncReport::default();
    let h = client.health()?;
    rep.clock_skew_ms = crate::shared::now_ms() - h.time_ms;

    progress(0, 0, "finding changes (covers are sized for the server the first time)");
    let mut pending = export::pending(db, machine, export::current(db, s, cache_dir)?, &listed_roots(s))?;
    // this PC's settings never go up before it has taken the server's
    if state_get(db, "settings_ok")?.is_none() { pending.retain(|(r, _)| !matches!(r.body, Body::Settings { .. })); }
    let total = pending.len() as u64;
    for batch in pending.chunks(BATCH) {
        progress(rep.pushed as u64, total, &format!("sending {} of {} changes", rep.pushed, total));
        rep.covers_up += upload_covers(db, client, cache_dir, batch)?;
        let recs: Vec<Record> = batch.iter().map(|(r, _)| r.clone()).collect();
        client.push(&recs)?;
        export::mark_sent(db, batch)?;
        rep.pushed += batch.len();
        for (r, _) in batch { *rep.sent.entry(r.body.kind().to_string()).or_default() += 1; }
        state_set(db, "pending", Some(&(total as usize - rep.pushed).to_string()))?;
    }

    let mut after: i64 = state_get(db, "cursor")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    loop {
        let page = client.pull(after, 2000)?;
        if page.records.is_empty() { break; }
        for x in &page.records {
            rep.pulled += 1;
            if x.record.machine == machine { continue; }
            take(db, Some(client), s, cache_dir, &x.record, &mut rep)?;
        }
        after = page.next;
        state_set(db, "cursor", Some(&after.to_string()))?;
    }
    if rep.applied > 0 { apply_inbox(db, Some(client), s, cache_dir, &mut rep)?; }
    rep.waiting = db.conn().query_row("SELECT COUNT(*) FROM sync_inbox", [], |r| r.get::<_, i64>(0))? as usize;
    Ok(rep)
}

/// A new PC taking everything from the server: settings now, the rest as the library is
/// scanned (it waits in the inbox until the games and files are found).
pub fn restore(db: &Db, client: &Client, s: &mut Settings, cache_dir: &Path, progress: &dyn Fn(usize)) -> anyhow::Result<SyncReport> {
    let mut rep = SyncReport::default();
    let mut after_key = String::new();
    let mut max_seq = 0;
    loop {
        let page = client.state(&[], &after_key, 2000)?;
        for x in &page.records {
            rep.pulled += 1;
            max_seq = max_seq.max(x.seq);
            take(db, Some(client), s, cache_dir, &x.record, &mut rep)?;
        }
        progress(rep.pulled);
        match page.next_key { Some(k) => after_key = k, None => break }
    }
    state_set(db, "cursor", Some(&max_seq.to_string()))?;
    Ok(rep)
}

/// The library folders a server knows, from its shared settings (for mapping on a new PC).
pub fn server_roots(client: &Client) -> anyhow::Result<Option<Vec<RootMap>>> {
    let page = client.state(&["Settings"], "", 1)?;
    let Some(x) = page.records.into_iter().next() else { return Ok(None) };
    let Body::Settings { shared } = x.record.body else { return Ok(None) };
    let roots: Vec<RootMap> = shared["roots"].as_array().map(|a| a.iter().map(|r| RootMap {
        id: r["id"].as_str().unwrap_or_default().into(), origin: r["origin"].as_str().unwrap_or_default().into(), path: Default::default() }).collect()).unwrap_or_default();
    Ok(Some(roots))
}

/// Take one record from another device (as a sync would, without fetching covers).
pub fn receive(db: &Db, s: &mut Settings, cache_dir: &Path, r: &Record) -> anyhow::Result<Applied> {
    let mut rep = SyncReport::default();
    take(db, None, s, cache_dir, r, &mut rep)
}
