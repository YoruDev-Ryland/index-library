//! Incremental, parallel, listing-only scanner. Never reads file contents.
use crate::config::Settings;
use crate::db::Db;
use crate::fsx::{Entry, Meta, Transport};
use crate::systems::{is_ignored, map_folder, Mapping, SystemDef};
use crate::util::{ext_of, now_nanos, p2s, subtree_prefix};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, Default)]
pub struct ScanOpts {
    /// Re-list every directory even if its mtime is unchanged.
    pub full: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ScanReport {
    pub dirs_listed: u64,
    pub dirs_skipped: u64,
    pub files_added: u64,
    pub files_removed: u64,
    pub files_changed: u64,
    pub errors: Vec<(PathBuf, String)>,
    pub unknown_folders: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    pub dirs_done: u64,
    pub dirs_queued: u64,
    pub files_seen: u64,
}

struct Work {
    path: PathBuf,
    system: &'static SystemDef,
    parent_id: Option<i64>,
    mtime: i64,
    stored: Option<(i64, Option<i64>)>,
    children: Vec<(i64, PathBuf)>,
}

enum Outcome {
    Skipped(Vec<(i64, PathBuf, std::io::Result<Meta>)>),
    Listed(Vec<Entry>),
    Failed(String),
}

pub fn scan(
    db: &Db,
    fs: &dyn Transport,
    settings: &Settings,
    opts: ScanOpts,
    progress: &dyn Fn(ScanProgress),
    cancel: &AtomicBool,
) -> anyhow::Result<ScanReport> {
    let conn = db.conn();
    let scan_id = now_nanos();
    let mut rep = ScanReport::default();
    let mut touched: BTreeSet<&'static str> = BTreeSet::new();
    let mut level: Vec<Work> = Vec::new();
    let mut files_seen = 0u64;

    for root in &settings.library_roots {
        match fs.list(root) {
            Ok(entries) => {
                rep.dirs_listed += 1;
                // An unmounted share shows up as an empty local folder. If nothing recognisable is
                // here but the index has files under this root, treat it as unreachable.
                let has_system = entries.iter().any(|e| e.is_dir && matches!(map_folder(settings, &e.name), Mapping::System(_)));
                let pre = subtree_prefix(root);
                let indexed: i64 = conn.query_row("SELECT COUNT(*) FROM files WHERE substr(path,1,?1)=?2", (pre.chars().count() as i64, &pre), |r| r.get(0))?;
                if !has_system && indexed > 0 {
                    rep.errors.push((root.clone(), "no system folders found — is the library drive or NAS share mounted? Nothing was removed from the index.".into()));
                    mark_subtree_seen(db, &root.clone(), scan_id)?;
                    continue;
                }
                for e in entries.into_iter().filter(|e| e.is_dir) {
                    match map_folder(settings, &e.name) {
                        Mapping::System(s) => level.push(Work { path: root.join(&e.name), system: s, parent_id: None, mtime: e.mtime, stored: None, children: vec![] }),
                        Mapping::Ignored => {}
                        Mapping::Unknown => if !e.name.starts_with('.') { rep.unknown_folders.push(root.join(&e.name)) },
                    }
                }
            }
            Err(err) => {
                rep.errors.push((root.clone(), err.to_string()));
                mark_subtree_seen(db, &root.clone(), scan_id)?;
            }
        }
    }

    let workers = settings.scan_concurrency.max(1);
    let mut done = 0u64;
    while !level.is_empty() {
        if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled"); }
        // Look up stored state on this thread (rusqlite is not Sync).
        for w in level.iter_mut() {
            let ps = p2s(&w.path);
            w.stored = conn.query_row("SELECT id, mtime FROM dirs WHERE path=?1", [&ps], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
            if let Some((id, Some(m))) = w.stored {
                if !opts.full && m == w.mtime {
                    let mut q = conn.prepare_cached("SELECT id, path FROM dirs WHERE parent_id=?1")?;
                    w.children = q.query_map([id], |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?))))?.collect::<Result<_, _>>()?;
                }
            }
        }
        let skip = |w: &Work| matches!(w.stored, Some((_, Some(m))) if !opts.full && m == w.mtime);
        // Parallel I/O: listing or stat-ing children.
        let next_idx = AtomicUsize::new(0);
        let mut outcomes: Vec<Option<Outcome>> = (0..level.len()).map(|_| None).collect();
        let slots: Vec<std::sync::Mutex<Option<Outcome>>> = (0..level.len()).map(|_| std::sync::Mutex::new(None)).collect();
        std::thread::scope(|sc| {
            for _ in 0..workers.min(level.len()) {
                sc.spawn(|| loop {
                    let i = next_idx.fetch_add(1, Ordering::Relaxed);
                    if i >= level.len() || cancel.load(Ordering::Relaxed) { break; }
                    let w = &level[i];
                    let o = if skip(w) {
                        Outcome::Skipped(w.children.iter().map(|(id, p)| (*id, p.clone(), fs.stat(p))).collect())
                    } else {
                        match fs.list(&w.path) { Ok(e) => Outcome::Listed(e), Err(e) => Outcome::Failed(e.to_string()) }
                    };
                    *slots[i].lock().unwrap() = Some(o);
                });
            }
        });
        for (i, s) in slots.into_iter().enumerate() { outcomes[i] = s.into_inner().unwrap(); }
        if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled"); }

        let tx = conn.unchecked_transaction()?;
        let mut next = Vec::new();
        for (w, o) in level.into_iter().zip(outcomes) {
            let ps = p2s(&w.path);
            touched.insert(w.system.id);
            done += 1;
            match o.expect("worker produced no outcome") {
                Outcome::Skipped(children) => {
                    rep.dirs_skipped += 1;
                    let id = w.stored.unwrap().0;
                    tx.execute("UPDATE dirs SET seen_scan=?1 WHERE id=?2", (scan_id, id))?;
                    files_seen += tx.execute("UPDATE files SET seen_scan=?1 WHERE dir_id=?2", (scan_id, id))? as u64;
                    for (_cid, cp, st) in children {
                        match st {
                            Ok(m) if m.is_dir => next.push(Work { path: cp, system: w.system, parent_id: Some(id), mtime: m.mtime, stored: None, children: vec![] }),
                            Ok(_) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => {
                                rep.errors.push((cp.clone(), e.to_string()));
                                tx.execute("UPDATE dirs SET seen_scan=?1, error=?2 WHERE path=?3", (scan_id, e.to_string(), p2s(&cp)))?;
                                mark_subtree_seen_tx(&tx, &cp, scan_id)?;
                            }
                        }
                    }
                }
                Outcome::Failed(err) => {
                    rep.errors.push((w.path.clone(), err.clone()));
                    tx.execute(
                        "INSERT INTO dirs(path, parent_id, system_id, mtime, scanned_at, error, seen_scan) VALUES(?1,?2,?3,NULL,?4,?5,?6)
                         ON CONFLICT(path) DO UPDATE SET error=excluded.error, seen_scan=excluded.seen_scan",
                        (&ps, w.parent_id, w.system.id, scan_id, &err, scan_id))?;
                    mark_subtree_seen_tx(&tx, &w.path, scan_id)?;
                }
                Outcome::Listed(entries) => {
                    rep.dirs_listed += 1;
                    tx.execute(
                        "INSERT INTO dirs(path, parent_id, system_id, mtime, scanned_at, error, seen_scan) VALUES(?1,?2,?3,?4,?5,NULL,?5)
                         ON CONFLICT(path) DO UPDATE SET mtime=excluded.mtime, parent_id=excluded.parent_id, system_id=excluded.system_id, scanned_at=excluded.scanned_at, error=NULL, seen_scan=excluded.seen_scan",
                        (&ps, w.parent_id, w.system.id, w.mtime, scan_id))?;
                    let dir_id: i64 = tx.query_row("SELECT id FROM dirs WHERE path=?1", [&ps], |r| r.get(0))?;
                    for e in entries {
                        if e.is_dir {
                            if !is_ignored(settings, &e.name) {
                                next.push(Work { path: w.path.join(&e.name), system: w.system, parent_id: Some(dir_id), mtime: e.mtime, stored: None, children: vec![] });
                            }
                            continue;
                        }
                        let ext = ext_of(&e.name);
                        if !w.system.accepts_ext(&ext) { continue; }
                        files_seen += 1;
                        let fp = p2s(&w.path.join(&e.name));
                        let existing: Option<(i64, i64, i64)> = tx.query_row("SELECT id, size, mtime FROM files WHERE path=?1", [&fp], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
                        match existing {
                            None => {
                                tx.execute("INSERT INTO files(path, dir_id, system_id, name, ext, size, mtime, seen_scan) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                                    (&fp, dir_id, w.system.id, &e.name, &ext, e.size as i64, e.mtime, scan_id))?;
                                rep.files_added += 1;
                            }
                            Some((id, size, mtime)) if size != e.size as i64 || mtime != e.mtime => {
                                tx.execute("UPDATE files SET size=?1, mtime=?2, dir_id=?3, seen_scan=?4, probe_json=NULL, probe_key=NULL, crc32=NULL, sha1=NULL, md5=NULL, hash_key=NULL, dat_status=NULL WHERE id=?5",
                                    (e.size as i64, e.mtime, dir_id, scan_id, id))?;
                                rep.files_changed += 1;
                            }
                            Some((id, _, _)) => { tx.execute("UPDATE files SET seen_scan=?1, dir_id=?2 WHERE id=?3", (scan_id, dir_id, id))?; }
                        }
                    }
                }
            }
        }
        tx.commit()?;
        progress(ScanProgress { dirs_done: done, dirs_queued: next.len() as u64, files_seen });
        level = next;
    }

    // Sweep: anything not seen in this scan is gone.
    let tx = conn.unchecked_transaction()?;
    rep.files_removed = tx.execute("DELETE FROM files WHERE seen_scan IS NOT ?1", [scan_id])? as u64;
    tx.execute("DELETE FROM dirs WHERE seen_scan IS NOT ?1", [scan_id])?;
    tx.execute("DELETE FROM status WHERE game_id NOT IN (SELECT game_id FROM files WHERE game_id IS NOT NULL)", [])?;
    tx.execute("DELETE FROM games WHERE id NOT IN (SELECT game_id FROM files WHERE game_id IS NOT NULL) AND NOT EXISTS (SELECT 1 FROM shelf_copies c WHERE c.deleted=0 AND c.ref = games.system_id || ':' || games.group_key)", [])?;
    tx.commit()?;

    // Regroup every system, not just the ones whose folders changed: grouping is quick, and a
    // newer version of the app may group names differently than the stored grouping.
    let present: Vec<String> = {
        let mut q = db.conn().prepare("SELECT DISTINCT system_id FROM files")?;
        let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        v
    };
    for sid in present.iter().map(|s| s.as_str()).chain(touched.iter().copied()).collect::<BTreeSet<&str>>() {
        if let Some(s) = crate::systems::by_id(sid) { crate::grouping::assign_games(db, s)?; }
    }
    Ok(rep)
}

fn mark_subtree_seen(db: &Db, p: &std::path::Path, scan_id: i64) -> anyhow::Result<()> {
    mark_subtree_seen_tx(db.conn(), p, scan_id)
}

fn mark_subtree_seen_tx(c: &rusqlite::Connection, p: &std::path::Path, scan_id: i64) -> anyhow::Result<()> {
    let pre = subtree_prefix(p);
    let n = pre.chars().count() as i64;
    c.execute("UPDATE files SET seen_scan=?1 WHERE substr(path, 1, ?2) = ?3", (scan_id, n, &pre))?;
    c.execute("UPDATE dirs SET seen_scan=?1 WHERE substr(path, 1, ?2) = ?3 OR path = ?4", (scan_id, n, &pre, p2s(p)))?;
    Ok(())
}
