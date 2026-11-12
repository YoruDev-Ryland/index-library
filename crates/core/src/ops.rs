//! Every file-system mutation goes through here: checked, journaled, undoable.
use crate::db::{now, Db};
use crate::fsx::Transport;
use crate::shared::journal::{EntryKind, Journal};
use crate::util::{ext_of, p2s};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpKind { Rename, Move, Archive, Restore, Delete, WriteFile, RemoveDir }

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self { OpKind::Rename => "rename", OpKind::Move => "move", OpKind::Archive => "archive", OpKind::Restore => "restore", OpKind::Delete => "delete", OpKind::WriteFile => "writefile", OpKind::RemoveDir => "rmdir" }
    }
    fn parse(s: &str) -> OpKind {
        match s { "move" => OpKind::Move, "archive" => OpKind::Archive, "restore" => OpKind::Restore, "delete" => OpKind::Delete, "writefile" => OpKind::WriteFile, "rmdir" => OpKind::RemoveDir, _ => OpKind::Rename }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub kind: OpKind,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    /// (size, mtime) the src must still have, from the preview.
    pub expect: Option<(i64, i64)>,
    #[serde(skip)]
    pub data: Option<Vec<u8>>,
    /// WriteFile only: (old name, new name) references to update, applied at execution time
    /// and only for renames that actually happened (new exists, old gone).
    #[serde(skip)]
    pub rewrites: Vec<(String, String)>,
}

impl Op {
    pub fn rename(src: PathBuf, dst: PathBuf) -> Op { Op { kind: OpKind::Rename, src, dst: Some(dst), expect: None, data: None, rewrites: vec![] } }
    pub fn mv(src: PathBuf, dst: PathBuf) -> Op { Op { kind: OpKind::Move, src, dst: Some(dst), expect: None, data: None, rewrites: vec![] } }
    pub fn archive(src: PathBuf, dst: PathBuf) -> Op { Op { kind: OpKind::Archive, src, dst: Some(dst), expect: None, data: None, rewrites: vec![] } }
    pub fn restore(src: PathBuf, dst: PathBuf) -> Op { Op { kind: OpKind::Restore, src, dst: Some(dst), expect: None, data: None, rewrites: vec![] } }
    pub fn remove_empty_dir(src: PathBuf) -> Op { Op { kind: OpKind::RemoveDir, src, dst: None, expect: None, data: None, rewrites: vec![] } }
    pub fn delete(src: PathBuf) -> Op { Op { kind: OpKind::Delete, src, dst: None, expect: None, data: None, rewrites: vec![] } }
    pub fn write_file(path: PathBuf, data: Vec<u8>) -> Op { Op { kind: OpKind::WriteFile, src: path, dst: None, expect: None, data: Some(data), rewrites: vec![] } }
    /// Update references inside a .cue/.m3u. `fallback` is used if `path` doesn't exist
    /// (e.g. the playlist's own rename was refused).
    pub fn rewrite_refs(path: PathBuf, fallback: PathBuf, rewrites: Vec<(String, String)>) -> Op {
        Op { kind: OpKind::WriteFile, src: path, dst: Some(fallback), expect: None, data: None, rewrites }
    }
    pub fn expecting(mut self, size: i64, mtime: i64) -> Op { self.expect = Some((size, mtime)); self }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BatchResult {
    pub batch_id: String,
    pub done: Vec<i64>,
    pub failed: Vec<(Op, String)>,
}

fn case_only(a: &Path, b: &Path) -> bool {
    a != b && p2s(a).to_lowercase() == p2s(b).to_lowercase()
}

fn move_path(fs: &dyn Transport, src: &Path, dst: &Path) -> Result<(), String> {
    if !fs.exists(src) { return Err(format!("{} no longer exists", p2s(src))); }
    if case_only(src, dst) {
        // On a case-sensitive filesystem "Foo.zip" may be a different file from "foo.zip".
        let (dir, want) = (dst.parent().unwrap_or(Path::new(".")), dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let have = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if fs.list(dir).map_err(|e| e.to_string())?.iter().any(|e| e.name == want && e.name != have) {
            return Err(format!("target already exists: {}", p2s(dst)));
        }
        let mut tmp = dst.as_os_str().to_owned();
        tmp.push(".lo-tmp");
        let tmp = PathBuf::from(tmp);
        if fs.exists(&tmp) { return Err(format!("temporary name in use: {}", p2s(&tmp))); }
        fs.rename(src, &tmp).map_err(|e| e.to_string())?;
        return fs.rename(&tmp, dst).map_err(|e| { let _ = fs.rename(&tmp, src); e.to_string() });
    }
    if fs.exists(dst) { return Err(format!("target already exists: {}", p2s(dst))); }
    if let Some(parent) = dst.parent() { fs.create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs.rename(src, dst).map_err(|e| e.to_string())
}

fn read_all(fs: &dyn Transport, p: &Path) -> std::io::Result<Vec<u8>> {
    let m = fs.stat(p)?;
    fs.read_range(p, 0, m.size as usize)
}

/// Rewrite references to renamed files: quoted names (cue FILE lines) and whole lines (m3u).
pub fn rewrite_refs_text(text: &str, pairs: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let ending = &line[body.len()..];
        let mut l = body.to_string();
        for (old, new) in pairs {
            if l.trim() == old { l = l.replace(old.as_str(), new); continue; }
            l = l.replace(&format!("\"{old}\""), &format!("\"{new}\""));
        }
        out.push_str(&l);
        out.push_str(ending);
    }
    out
}

/// Executes one op, returning data needed to undo it and the path actually touched.
fn execute(fs: &dyn Transport, op: &Op) -> Result<(Option<Vec<u8>>, PathBuf), String> {
    if let Some((size, mtime)) = op.expect {
        let m = fs.stat(&op.src).map_err(|_| format!("{} no longer exists", p2s(&op.src)))?;
        if m.size as i64 != size || m.mtime != mtime { return Err(format!("{} changed since the preview", p2s(&op.src))); }
    }
    match op.kind {
        OpKind::Rename | OpKind::Move | OpKind::Archive | OpKind::Restore => {
            move_path(fs, &op.src, op.dst.as_ref().ok_or("missing destination")?).map(|_| (None, op.src.clone()))
        }
        OpKind::Delete => {
            let m = fs.stat(&op.src).map_err(|e| e.to_string())?;
            if m.is_dir { return Err(format!("refusing to delete a folder: {}", p2s(&op.src))); }
            fs.remove_file(&op.src).map(|_| (None, op.src.clone())).map_err(|e| e.to_string())
        }
        OpKind::RemoveDir => {
            let n = fs.list(&op.src).map_err(|e| e.to_string())?.len();
            if n > 0 { return Err(format!("folder is no longer empty ({n} items): {}", p2s(&op.src))); }
            fs.remove_dir(&op.src).map(|_| (None, op.src.clone())).map_err(|e| e.to_string())
        }
        OpKind::WriteFile => {
            let path = if !fs.exists(&op.src) { op.dst.clone().filter(|d| fs.exists(d)).unwrap_or(op.src.clone()) } else { op.src.clone() };
            let old = read_all(fs, &path).map_err(|e| e.to_string())?;
            let new = if op.rewrites.is_empty() {
                op.data.clone().unwrap_or_default()
            } else {
                let dir = path.parent().unwrap_or(Path::new(""));
                let done: Vec<(String, String)> = op.rewrites.iter().filter(|(o, n)| fs.exists(&dir.join(n)) && !fs.exists(&dir.join(o))).cloned().collect();
                if done.is_empty() { return Err("none of the referenced files were renamed; left unchanged".into()); }
                rewrite_refs_text(&String::from_utf8_lossy(&old), &done).into_bytes()
            };
            if new == old { return Err("no references needed updating".into()); }
            fs.write_atomic(&path, &new).map_err(|e| e.to_string())?;
            Ok((Some(old), path))
        }
    }
}

/// Keep the local index in step with a completed move.
pub fn update_index_path(db: &Db, src: &Path, dst: &Path) -> anyhow::Result<()> {
    let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir_id: Option<i64> = match dst.parent() {
        Some(d) => db.conn().query_row("SELECT id FROM dirs WHERE path=?1", [p2s(d)], |r| r.get(0)).optional()?,
        None => None,
    };
    db.conn().execute("UPDATE OR IGNORE files SET path=?1, name=?2, ext=?3, dir_id=COALESCE(?4, dir_id) WHERE path=?5",
        (p2s(dst), &name, ext_of(&name), dir_id, p2s(src)))?;
    Ok(())
}

fn index_after(db: &Db, kind: OpKind, src: &Path, dst: Option<&Path>) -> anyhow::Result<()> {
    match (kind, dst) {
        (OpKind::Rename | OpKind::Move, Some(d)) => update_index_path(db, src, d)?,
        (OpKind::Archive | OpKind::Delete, _) => { db.conn().execute("DELETE FROM files WHERE path=?1", [p2s(src)])?; }
        _ => {}
    }
    Ok(())
}

fn journal_op(j: &Journal, batch: &str, kind: OpKind, src: &Path, dst: Option<&Path>, label: &str) {
    let _ = j.append_at(&EntryKind::Op { batch_id: batch.to_string(), op: kind.as_str().into(), src: Some(j.rel(src)), dst: dst.map(|d| j.rel(d)), label: Some(label.to_string()) }, Some(src));
}

pub fn apply(db: &Db, fs: &dyn Transport, journal: &Journal, ops: Vec<Op>, label: &str) -> anyhow::Result<BatchResult> {
    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut res = BatchResult { batch_id: batch_id.clone(), ..Default::default() };
    for op in ops {
        match execute(fs, &op) {
            Ok((undo, actual)) => {
                let dst = if op.kind == OpKind::WriteFile { None } else { op.dst.clone() };
                db.conn().execute("INSERT INTO ops(batch_id, machine_id, kind, src, dst, created_at, label, undo_data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    (&batch_id, &journal.machine, op.kind.as_str(), p2s(&actual), dst.as_deref().map(p2s), now(), label, undo))?;
                res.done.push(db.conn().last_insert_rowid());
                index_after(db, op.kind, &actual, dst.as_deref())?;
                journal_op(journal, &batch_id, op.kind, &actual, dst.as_deref(), label);
            }
            Err(e) => res.failed.push((op, e)),
        }
    }
    Ok(res)
}

/// Reverse a batch, newest op first. Ops whose files have moved on are refused.
pub fn undo_batch(db: &Db, fs: &dyn Transport, journal: &Journal, batch_id: &str) -> anyhow::Result<BatchResult> {
    let rows: Vec<(i64, String, String, Option<String>, Option<Vec<u8>>)> = {
        let mut q = db.conn().prepare("SELECT id, kind, src, dst, undo_data FROM ops WHERE batch_id=?1 AND undone=0 ORDER BY id DESC")?;
        let v = q.query_map([batch_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        v
    };
    let undo_batch_id = format!("undo:{batch_id}");
    let mut res = BatchResult { batch_id: undo_batch_id.clone(), ..Default::default() };
    for (id, kind, src, dst, undo) in rows {
        let kind = OpKind::parse(&kind);
        let src = PathBuf::from(src);
        let dst = dst.map(PathBuf::from);
        let r: Result<(), String> = match (kind, &dst) {
            (OpKind::Delete, _) => Err("deleted files can't be restored".into()),
            (OpKind::RemoveDir, _) => fs.create_dir_all(&src).map_err(|e| e.to_string()),
            (OpKind::WriteFile, _) => match &undo {
                Some(u) => fs.write_atomic(&src, u).map_err(|e| e.to_string()),
                None => Err("this file edit was made on another PC; undo it there".into()),
            },
            (_, Some(d)) => move_path(fs, d, &src),
            _ => Err("nothing to undo".into()),
        };
        match r {
            Ok(()) => {
                db.conn().execute("UPDATE ops SET undone=1 WHERE id=?1", [id])?;
                res.done.push(id);
                if let Some(d) = &dst {
                    match kind {
                        OpKind::Rename | OpKind::Move => update_index_path(db, d, &src)?,
                        OpKind::Archive => crate::archive::forget(db, fs, d)?,
                        _ => {}
                    }
                    journal_op(journal, &undo_batch_id, if kind == OpKind::Archive { OpKind::Restore } else { kind }, d, Some(&src), "undo");
                }
            }
            Err(e) => {
                let op = Op { kind, src, dst, expect: None, data: None, rewrites: vec![] };
                res.failed.push((op, e));
            }
        }
    }
    Ok(res)
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchInfo {
    pub batch_id: String, pub label: String, pub created_at: i64, pub ops: i64, pub undone: i64,
    /// what the batch did, file by file (the first 50)
    pub items: Vec<OpItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpItem { pub kind: String, pub src: String, pub dst: Option<String>, pub undone: bool }

pub fn history(db: &Db, limit: i64) -> anyhow::Result<Vec<BatchInfo>> {
    let mut q = db.conn().prepare("SELECT batch_id, COALESCE(label,''), MIN(created_at), COUNT(*), SUM(undone) FROM ops WHERE batch_id NOT LIKE 'undo:%' GROUP BY batch_id ORDER BY MIN(id) DESC LIMIT ?1")?;
    let mut v: Vec<BatchInfo> = q.query_map([limit], |r| Ok(BatchInfo { batch_id: r.get(0)?, label: r.get(1)?, created_at: r.get(2)?, ops: r.get(3)?, undone: r.get(4)?, items: vec![] }))?.collect::<Result<_, _>>()?;
    let mut qi = db.conn().prepare("SELECT kind, src, dst, undone FROM ops WHERE batch_id=?1 ORDER BY id LIMIT 50")?;
    for b in v.iter_mut() {
        b.items = qi.query_map([&b.batch_id], |r| Ok(OpItem { kind: r.get(0)?, src: r.get(1)?, dst: r.get(2)?, undone: r.get::<_, i64>(3)? != 0 }))?.collect::<Result<_, _>>()?;
    }
    Ok(v)
}
