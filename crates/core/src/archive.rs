//! Archive: replaced originals wait in <system>/_archive with a hold timer.
use crate::db::{now, Db};
use crate::fsx::Transport;
use crate::ops::{apply, Op};
use crate::shared::journal::{EntryKind, Journal};
use crate::shared::manifest::{Manifest, ManifestEntry};
use crate::shared::{abs_from, rel_to_root};
use crate::util::{ext_of, p2s, stem_of};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const ARCHIVE_DIR: &str = "_archive";
const DAY: i64 = 86_400;
/// delete_after value for originals that must never be offered for deletion.
pub const PINNED: i64 = i64::MAX;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArchiveEntry {
    pub id: i64,
    pub system_id: Option<String>,
    pub path: PathBuf,
    pub original_path: PathBuf,
    pub archived_at: i64,
    pub delete_after: i64,
    pub reason: Option<String>,
    pub action: Option<String>,
    pub size: i64,
    #[serde(skip)]
    pub batch_id: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PurgeReport { pub deleted: u64, pub bytes: u64, pub failed: Vec<(PathBuf, String)> }

/// <root>/<system folder>/_archive for a file inside the library.
pub fn archive_dir_for(roots: &[PathBuf], file: &Path) -> anyhow::Result<PathBuf> {
    let (i, rel) = rel_to_root(roots, file).ok_or_else(|| anyhow::anyhow!("{} is not inside a library root", p2s(file)))?;
    let first = rel.split('/').next().filter(|s| !s.is_empty()).ok_or_else(|| anyhow::anyhow!("file is at the library root"))?;
    Ok(roots[i].join(first).join(ARCHIVE_DIR))
}

fn free_slot(fs: &dyn Transport, dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !fs.exists(&first) { return first; }
    let (stem, ext) = (stem_of(name), ext_of(name));
    let orig_ext = if ext.is_empty() { String::new() } else { format!(".{}", &name[name.len() - ext.len()..]) };
    (2..).map(|n| dir.join(format!("{stem} ({n}){orig_ext}"))).find(|p| !fs.exists(p)).unwrap()
}

fn update_manifest(fs: &dyn Transport, dir: &Path, f: impl FnOnce(&mut Manifest)) -> anyhow::Result<()> {
    let mut m = Manifest::load(fs, dir)?;
    f(&mut m);
    m.save(fs, dir)
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<ArchiveEntry> {
    Ok(ArchiveEntry {
        id: r.get(0)?, system_id: r.get(1)?, path: PathBuf::from(r.get::<_, String>(2)?), original_path: PathBuf::from(r.get::<_, String>(3)?),
        archived_at: r.get(4)?, delete_after: r.get(5)?, reason: r.get(6)?, action: r.get(7)?, size: r.get(8)?, batch_id: String::new(),
    })
}
const COLS: &str = "id, system_id, path, original_path, archived_at, delete_after, reason, action, size";

pub fn archive_file(db: &Db, fs: &dyn Transport, journal: &Journal, file: &Path, reason: &str, action: Option<&str>, hold_days: u32) -> anyhow::Result<ArchiveEntry> {
    let dir = archive_dir_for(&journal.roots, file)?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dst = free_slot(fs, &dir, &name);
    let size = fs.stat(file)?.size as i64;
    let system_id: Option<String> = db.conn().query_row("SELECT system_id FROM files WHERE path=?1", [p2s(file)], |r| r.get(0)).optional()?;
    let res = apply(db, fs, journal, vec![Op::archive(file.to_path_buf(), dst.clone())], &format!("archive: {reason}"))?;
    if let Some((_, e)) = res.failed.first() { anyhow::bail!("could not archive {}: {e}", p2s(file)); }
    let at = now();
    let delete_after = at + hold_days as i64 * DAY;
    update_manifest(fs, &dir, |m| m.entries.push(ManifestEntry {
        file: dst.file_name().unwrap().to_string_lossy().into_owned(), original_path: journal.rel(file), archived_at: at,
        delete_after, reason: reason.into(), action: action.map(String::from), size: size as u64, machine: journal.machine.clone(),
    }))?;
    db.conn().execute("INSERT INTO archive(system_id, path, original_path, archived_at, delete_after, reason, action, size) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                       ON CONFLICT(path) DO UPDATE SET original_path=excluded.original_path, archived_at=excluded.archived_at, delete_after=excluded.delete_after, reason=excluded.reason, action=excluded.action, size=excluded.size",
        (&system_id, p2s(&dst), p2s(file), at, delete_after, reason, action, size))?;
    let mut e = get(db, &dst)?.expect("just inserted");
    e.batch_id = res.batch_id;
    Ok(e)
}

fn get(db: &Db, path: &Path) -> anyhow::Result<Option<ArchiveEntry>> {
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM archive WHERE path=?1"), [p2s(path)], row).optional()?)
}
fn by_id(db: &Db, id: i64) -> anyhow::Result<ArchiveEntry> {
    Ok(db.conn().query_row(&format!("SELECT {COLS} FROM archive WHERE id=?1"), [id], row)?)
}

pub fn list(db: &Db) -> anyhow::Result<Vec<ArchiveEntry>> {
    let mut q = db.conn().prepare(&format!("SELECT {COLS} FROM archive ORDER BY delete_after"))?;
    let v = q.query_map([], row)?.collect::<Result<_, _>>()?;
    Ok(v)
}

pub fn expired(db: &Db, now: i64) -> anyhow::Result<Vec<ArchiveEntry>> {
    Ok(list(db)?.into_iter().filter(|e| e.delete_after <= now).collect())
}

/// Drop an entry from the table and its manifest (file already moved/deleted).
pub fn forget(db: &Db, fs: &dyn Transport, archived_path: &Path) -> anyhow::Result<()> {
    db.conn().execute("DELETE FROM archive WHERE path=?1", [p2s(archived_path)])?;
    if let (Some(dir), Some(name)) = (archived_path.parent(), archived_path.file_name()) {
        let name = name.to_string_lossy().into_owned();
        if fs.exists(&dir.join(crate::shared::manifest::MANIFEST)) {
            update_manifest(fs, dir, |m| m.entries.retain(|e| e.file != name))?;
        }
    }
    Ok(())
}

pub fn purge(db: &Db, fs: &dyn Transport, journal: &Journal, ids: &[i64]) -> anyhow::Result<PurgeReport> {
    let mut rep = PurgeReport::default();
    for id in ids {
        let Ok(e) = by_id(db, *id) else { continue }; // gone after a manifest rebuild
        if !fs.exists(&e.path) {
            forget(db, fs, &e.path)?; // already gone (another PC purged it)
            continue;
        }
        let r = apply(db, fs, journal, vec![Op::delete(e.path.clone())], "archive: delete")?;
        match r.failed.first() {
            Some((_, err)) => rep.failed.push((e.path.clone(), err.clone())),
            None => { forget(db, fs, &e.path)?; rep.deleted += 1; rep.bytes += e.size as u64; }
        }
    }
    Ok(rep)
}

pub fn restore(db: &Db, fs: &dyn Transport, journal: &Journal, id: i64) -> anyhow::Result<PathBuf> {
    let e = by_id(db, id)?;
    if !fs.exists(&e.path) { forget(db, fs, &e.path)?; anyhow::bail!("{} is no longer in the archive (restored or deleted on another PC)", p2s(&e.path)); }
    let r = apply(db, fs, journal, vec![Op::restore(e.path.clone(), e.original_path.clone())], "archive: restore")?;
    if let Some((_, err)) = r.failed.first() { anyhow::bail!("could not restore: {err}"); }
    forget(db, fs, &e.path)?;
    Ok(e.original_path)
}

pub fn extend(db: &Db, fs: &dyn Transport, journal: &Journal, id: i64, days: u32) -> anyhow::Result<()> {
    let e = by_id(db, id)?;
    if e.delete_after == PINNED { return Ok(()); }
    let new = e.delete_after.max(now()) + days as i64 * DAY;
    db.conn().execute("UPDATE archive SET delete_after=?1 WHERE id=?2", (new, id))?;
    if let (Some(dir), Some(name)) = (e.path.parent(), e.path.file_name()) {
        let name = name.to_string_lossy().into_owned();
        update_manifest(fs, dir, |m| for me in m.entries.iter_mut().filter(|me| me.file == name) { me.delete_after = new; })?;
    }
    let _ = journal.append_at(&EntryKind::ArchiveHold { path: journal.rel(&e.path), delete_after: new }, Some(&e.path));
    Ok(())
}

/// Rebuild the archive table from every <root>/<system>/_archive/manifest.json. Returns entries known.
pub fn rebuild_from_manifests(db: &Db, fs: &dyn Transport, roots: &[PathBuf]) -> anyhow::Result<usize> {
    let mut n = 0;
    for root in roots {
        let Ok(top) = fs.list(root) else { continue };
        for d in top.into_iter().filter(|d| d.is_dir) {
            let dir = root.join(&d.name).join(ARCHIVE_DIR);
            if !fs.exists(&dir.join(crate::shared::manifest::MANIFEST)) { continue; }
            let m = Manifest::load(fs, &dir)?;
            let system = match crate::systems::map_folder(&crate::config::Settings::default(), &d.name) { crate::systems::Mapping::System(s) => Some(s.id), _ => None };
            let keep: Vec<String> = m.entries.iter().map(|e| p2s(&dir.join(&e.file))).collect();
            for e in &m.entries {
                db.conn().execute("INSERT INTO archive(system_id, path, original_path, archived_at, delete_after, reason, action, size) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                                   ON CONFLICT(path) DO UPDATE SET delete_after=excluded.delete_after, original_path=excluded.original_path",
                    (system, p2s(&dir.join(&e.file)), p2s(&abs_from(root, &e.original_path)), e.archived_at, e.delete_after, &e.reason, &e.action, e.size as i64))?;
                n += 1;
            }
            // entries no longer in the manifest were purged/restored elsewhere
            let pre = crate::util::subtree_prefix(&dir);
            let mut q = db.conn().prepare("SELECT path FROM archive WHERE substr(path,1,?1)=?2")?;
            let existing: Vec<String> = q.query_map((pre.chars().count() as i64, &pre), |r| r.get(0))?.collect::<Result<_, _>>()?;
            for p in existing.into_iter().filter(|p| !keep.contains(p)) { db.conn().execute("DELETE FROM archive WHERE path=?1", [p])?; }
        }
    }
    Ok(n)
}

/// Keep an archived original indefinitely (e.g. its replacement never arrived). Excluded from
/// expiry and from "Empty archive", on every PC (stored in the manifest).
pub fn pin(db: &Db, fs: &dyn Transport, id: i64, reason: &str) -> anyhow::Result<()> {
    let e = by_id(db, id)?;
    db.conn().execute("UPDATE archive SET delete_after=?1, reason=?2 WHERE id=?3", (PINNED, reason, id))?;
    if let (Some(dir), Some(name)) = (e.path.parent(), e.path.file_name()) {
        let name = name.to_string_lossy().into_owned();
        update_manifest(fs, dir, |m| for me in m.entries.iter_mut().filter(|me| me.file == name) { me.delete_after = PINNED; me.reason = reason.into(); })?;
    }
    Ok(())
}

/// Delete only entries that are *still* past their hold after re-reading the shared manifests
/// (another PC may have extended or restored them).
pub fn purge_expired(db: &Db, fs: &dyn Transport, journal: &Journal, ids: &[i64]) -> anyhow::Result<PurgeReport> {
    let _ = crate::shared::journal::sync(db, fs, &journal.roots, &journal.machine);
    rebuild_from_manifests(db, fs, &journal.roots)?;
    let now = now();
    let still: Vec<i64> = ids.iter().copied().filter(|id| by_id(db, *id).map(|e| e.delete_after <= now).unwrap_or(false)).collect();
    purge(db, fs, journal, &still)
}

/// Delete everything in the archive except pinned originals, after re-reading the manifests.
pub fn purge_all(db: &Db, fs: &dyn Transport, journal: &Journal) -> anyhow::Result<PurgeReport> {
    let _ = crate::shared::journal::sync(db, fs, &journal.roots, &journal.machine);
    rebuild_from_manifests(db, fs, &journal.roots)?;
    let ids: Vec<i64> = list(db)?.into_iter().filter(|e| e.delete_after != PINNED).map(|e| e.id).collect();
    purge(db, fs, journal, &ids)
}
