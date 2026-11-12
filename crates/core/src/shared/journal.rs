//! Append-only per-machine operation journal at <root>/.index-library/journal/<machine>.jsonl.
use super::{abs_from, now_ms, rel_to_root, shared_dir};
use crate::db::Db;
use crate::fsx::Transport;
use crate::util::p2s;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EntryKind {
    /// A file-system op; paths are root-relative.
    Op { batch_id: String, op: String, src: Option<String>, dst: Option<String>, #[serde(default)] label: Option<String> },
    Status { game_key: String, key: String, value: String },
    ArchiveHold { path: String, delete_after: i64 },
    /// your play status for a game (the whole record; newest wins)
    Play { game: crate::userdata::GameRef, info: crate::userdata::PlayInfo },
    /// a collection's name and notes (the whole record; newest wins)
    Collection { id: String, name: String, notes: Option<String>, deleted: bool, created: i64 },
    /// a collection's entries in play order (the whole list; newest wins)
    CollectionItems { id: String, items: Vec<crate::userdata::ItemRef> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub ts: i64,
    pub machine: String,
    #[serde(flatten)]
    pub kind: EntryKind,
}

#[derive(Clone)]
pub struct Journal {
    pub fs: Arc<dyn Transport>,
    pub roots: Vec<PathBuf>,
    pub machine: String,
}

fn journal_dir(root: &Path) -> PathBuf { shared_dir(root).join("journal") }

impl Journal {
    pub fn new(fs: Arc<dyn Transport>, roots: Vec<PathBuf>, machine: String) -> Journal { Journal { fs, roots, machine } }

    /// Root-relative form of an absolute path (falls back to the absolute string).
    pub fn rel(&self, p: &Path) -> String { rel_to_root(&self.roots, p).map(|(_, r)| r).unwrap_or_else(|| p2s(p)) }

    fn root_for(&self, kind: &EntryKind) -> Option<&PathBuf> {
        // Entries carry relative paths; pick the root by the original absolute path when given.
        let _ = kind;
        self.roots.first()
    }

    pub fn append(&self, kind: &EntryKind) -> anyhow::Result<()> {
        self.append_at(kind, None)
    }

    /// Append to the journal of the root containing `near` (or the first root).
    pub fn append_at(&self, kind: &EntryKind, near: Option<&Path>) -> anyhow::Result<()> {
        let root = near.and_then(|p| rel_to_root(&self.roots, p)).map(|(i, _)| &self.roots[i]).or_else(|| self.root_for(kind));
        let Some(root) = root else { return Ok(()) };
        let e = JournalEntry { ts: now_ms(), machine: self.machine.clone(), kind: kind.clone() };
        let mut line = serde_json::to_vec(&e)?;
        line.push(b'\n');
        self.fs.append(&journal_dir(root).join(format!("{}.jsonl", self.machine)), &line)?;
        Ok(())
    }
}

/// Stable cross-machine identity of a game: probe id, else sha1, else root-relative path.
pub fn game_key(db: &Db, roots: &[PathBuf], game_id: i64) -> anyhow::Result<String> {
    let c = db.conn();
    let row: Option<(String, Option<String>, Option<String>, String)> = c.query_row(
        "SELECT f.system_id, json_extract(f.probe_json,'$.id'), f.sha1, f.path FROM files f WHERE f.game_id=?1 ORDER BY f.path LIMIT 1",
        [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?;
    let Some((sys, probe, sha1, path)) = row else { anyhow::bail!("game {game_id} has no files") };
    Ok(match (probe, sha1) {
        (Some(p), _) => format!("probe:{sys}:{p}"),
        (None, Some(s)) => format!("sha1:{s}"),
        _ => format!("path:{}", rel_to_root(roots, Path::new(&path)).map(|(_, r)| r).unwrap_or(path)),
    })
}

fn resolve_game(db: &Db, roots: &[PathBuf], key: &str) -> anyhow::Result<Option<i64>> {
    let c = db.conn();
    let r = if let Some(rest) = key.strip_prefix("probe:") {
        let (sys, id) = rest.split_once(':').unwrap_or(("", rest));
        c.query_row("SELECT game_id FROM files WHERE system_id=?1 AND json_extract(probe_json,'$.id')=?2 LIMIT 1", (sys, id), |r| r.get(0)).optional()?
    } else if let Some(s) = key.strip_prefix("sha1:") {
        c.query_row("SELECT game_id FROM files WHERE sha1=?1 LIMIT 1", [s], |r| r.get(0)).optional()?
    } else if let Some(rel) = key.strip_prefix("path:") {
        let mut found = None;
        for root in roots {
            found = c.query_row("SELECT game_id FROM files WHERE path=?1", [p2s(&abs_from(root, rel))], |r| r.get(0)).optional()?;
            if found.is_some() { break; }
        }
        found
    } else { None };
    Ok(r.flatten())
}

/// Merge other machines' new journal entries into the local DB. Returns entries applied.
pub fn sync(db: &Db, fs: &dyn Transport, roots: &[PathBuf], me: &str) -> anyhow::Result<usize> {
    let mut applied = 0;
    let mut archive_touched = false;
    for root in roots {
        let dir = journal_dir(root);
        let Ok(entries) = fs.list(&dir) else { continue };
        for e in entries {
            let Some(machine) = e.name.strip_suffix(".jsonl") else { continue };
            if machine == me || e.is_dir { continue; }
            let cursor_key = format!("{machine}|{}", p2s(root));
            let offset: i64 = db.conn().query_row("SELECT offset FROM journal_cursor WHERE machine_id=?1", [&cursor_key], |r| r.get(0)).optional()?.unwrap_or(0);
            if e.size as i64 <= offset { continue; }
            let data = fs.read_range(&dir.join(&e.name), offset as u64, (e.size as i64 - offset) as usize)?;
            let Some(last_nl) = data.iter().rposition(|b| *b == b'\n') else { continue };
            let complete = &data[..=last_nl];
            for line in complete.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
                let Ok(entry) = serde_json::from_slice::<JournalEntry>(line) else { continue };
                if matches!(&entry.kind, EntryKind::Op { op, .. } if matches!(op.as_str(), "archive" | "restore" | "delete")) { archive_touched = true; }
                apply(db, root, roots, &entry)?;
                applied += 1;
            }
            db.conn().execute("INSERT INTO journal_cursor(machine_id, offset) VALUES(?1,?2) ON CONFLICT(machine_id) DO UPDATE SET offset=excluded.offset",
                (&cursor_key, offset + complete.len() as i64))?;
        }
    }
    if archive_touched { crate::archive::rebuild_from_manifests(db, fs, roots)?; }
    Ok(applied)
}

fn apply(db: &Db, root: &Path, roots: &[PathBuf], e: &JournalEntry) -> anyhow::Result<()> {
    let c = db.conn();
    match &e.kind {
        EntryKind::Status { game_key, key, value } => {
            if let Some(gid) = resolve_game(db, roots, game_key)? {
                let ts = e.ts / 1000;
                c.execute("INSERT INTO status(game_id, key, value, updated_at) VALUES(?1,?2,?3,?4)
                           ON CONFLICT(game_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at WHERE excluded.updated_at >= status.updated_at",
                    (gid, key, value, ts))?;
            }
        }
        EntryKind::Op { batch_id, op, src, dst, label } => {
            let src = src.as_ref().map(|s| p2s(&abs_from(root, s)));
            let dst = dst.as_ref().map(|s| p2s(&abs_from(root, s)));
            match (op.as_str(), &src, &dst) {
                ("delete", Some(s), _) => { c.execute("DELETE FROM files WHERE path=?1", [s])?; c.execute("DELETE FROM archive WHERE path=?1", [s])?; }
                // archived files leave the library; the archive table is rebuilt from manifests after sync
                ("archive", Some(s), _) => { c.execute("DELETE FROM files WHERE path=?1", [s])?; }
                ("restore" | "writefile" | "rmdir", _, _) => {}
                (_, Some(s), Some(d)) => crate::ops::update_index_path(db, Path::new(s), Path::new(d))?,
                _ => {}
            }
            // Record it so History/Undo on this PC can see and reverse it.
            if let Some(orig) = batch_id.strip_prefix("undo:") {
                c.execute("UPDATE ops SET undone=1 WHERE batch_id=?1", [orig])?;
            } else {
                c.execute("INSERT INTO ops(batch_id, machine_id, kind, src, dst, created_at, label) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    (batch_id, &e.machine, op, &src, &dst, e.ts / 1000, label))?;
            }
        }
        EntryKind::ArchiveHold { path, delete_after } => {
            c.execute("UPDATE archive SET delete_after=?1 WHERE path=?2", (delete_after, p2s(&abs_from(root, path))))?;
        }
        EntryKind::Play { game, info } => crate::userdata::apply_play(db, game, info, e.ts)?,
        EntryKind::Collection { id, name, notes, deleted, created } => crate::userdata::apply_collection(db, id, name, notes, *deleted, *created, e.ts)?,
        EntryKind::CollectionItems { id, items } => crate::userdata::apply_items(db, id, items, e.ts)?,
    }
    Ok(())
}
