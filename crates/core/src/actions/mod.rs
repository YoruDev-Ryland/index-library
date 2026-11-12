//! Actions: per-system processing steps (decrypt, convert, compress) with status tracking.
//!
//! Adding an action = one new file implementing [`Action`] + one line in [`registry`].
//! Use [`ActionCtx`] helpers so the action stays short and every change is journaled,
//! archived and cleaned up correctly.
pub mod compress;
pub mod n3ds;
pub mod psp;

use crate::archive::archive_file;
use crate::config::Settings;
use crate::db::{now, Db};
use crate::fsx::Transport;
use crate::jobs::JobCtx;
use crate::ops::{apply, Op};
use crate::probe::Probe;
use crate::shared::journal::{game_key, EntryKind, Journal};
use crate::shared::lease::Lease;
use crate::shared::rel_to_root;
use crate::util::{ext_of, p2s};
use once_cell::sync::Lazy;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub id: i64,
    pub game_id: Option<i64>,
    pub path: PathBuf,
    pub name: String,
    pub ext: String,
    pub system_id: String,
    pub size: i64,
    pub mtime: i64,
    pub probe: Option<Probe>,
    pub status: BTreeMap<String, String>,
}

impl FileRow {
    pub fn status_is(&self, key: &str, value: &str) -> bool { self.status.get(key).map(|v| v == value).unwrap_or(false) }
}

#[derive(Debug, Clone, Serialize)]
pub struct Requirement { pub name: String, pub ok: bool, pub hint: String }

#[derive(Debug, Clone, Serialize)]
pub enum ActionOutcome { Done { output: PathBuf }, Skipped(String) }

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExecReport { pub ok: u64, pub skipped: Vec<(PathBuf, String)>, pub failed: Vec<(PathBuf, String)> }

#[derive(Debug, Clone, Serialize)]
pub struct ActionInfo { pub id: &'static str, pub name: &'static str, pub description: &'static str, pub systems: &'static [&'static str], pub sets: &'static [&'static str] }

pub trait Action: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str { "" }
    fn systems(&self) -> &'static [&'static str];
    /// Is this file a candidate (right type, not already processed)?
    fn applies(&self, f: &FileRow) -> bool;
    /// Status keys set on success.
    fn sets(&self) -> &'static [&'static str];
    fn requires(&self, s: &Settings) -> Vec<Requirement>;
    fn run(&self, ctx: &mut ActionCtx, file: &FileRow) -> anyhow::Result<ActionOutcome>;
    fn info(&self) -> ActionInfo { ActionInfo { id: self.id(), name: self.name(), description: self.description(), systems: self.systems(), sets: self.sets() } }
}

static REGISTRY: Lazy<Vec<Box<dyn Action>>> = Lazy::new(|| vec![
    Box::new(n3ds::DecryptCia),
    Box::new(n3ds::Decrypt3ds),
    Box::new(psp::PkgToCso),
    Box::new(compress::Ps1Chd),
    Box::new(compress::GcRvz),
]);

pub fn registry() -> &'static [Box<dyn Action>] { &REGISTRY }
pub fn by_id(id: &str) -> Option<&'static dyn Action> { REGISTRY.iter().find(|a| a.id() == id).map(|b| b.as_ref()) }

pub fn file_rows(db: &Db, ids: &[i64]) -> anyhow::Result<Vec<FileRow>> {
    let mut q = db.conn().prepare("SELECT id, game_id, path, name, ext, system_id, size, mtime, probe_json FROM files WHERE id=?1")?;
    let mut sq = db.conn().prepare("SELECT key, value FROM status WHERE game_id=?1")?;
    let mut out = Vec::new();
    for id in ids {
        let row = q.query_row([id], |r| Ok(FileRow {
            id: r.get(0)?, game_id: r.get(1)?, path: PathBuf::from(r.get::<_, String>(2)?), name: r.get(3)?, ext: r.get(4)?,
            system_id: r.get(5)?, size: r.get(6)?, mtime: r.get(7)?,
            probe: r.get::<_, Option<String>>(8)?.and_then(|s| serde_json::from_str(&s).ok()), status: BTreeMap::new(),
        })).optional()?;
        let Some(mut row) = row else { continue };
        if let Some(g) = row.game_id { row.status = sq.query_map([g], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?; }
        out.push(row);
    }
    Ok(out)
}

/// For each action, the files in `ids` it applies to.
pub fn applicable(db: &Db, ids: &[i64]) -> anyhow::Result<Vec<(&'static dyn Action, Vec<i64>)>> {
    let rows = file_rows(db, ids)?;
    Ok(registry().iter().filter_map(|a| {
        let hits: Vec<i64> = rows.iter().filter(|r| a.systems().contains(&r.system_id.as_str()) && a.applies(r)).map(|r| r.id).collect();
        (!hits.is_empty()).then_some((a.as_ref(), hits))
    }).collect())
}

/// Execution context handed to an action. Temp files and leases are released on drop.
pub struct ActionCtx<'a> {
    pub db: &'a Db,
    pub fs: &'a dyn Transport,
    pub settings: &'a Settings,
    pub journal: &'a Journal,
    pub action_id: &'static str,
    job: Option<&'a JobCtx>,
    cancel: &'a AtomicBool,
    temps: Vec<PathBuf>,
    leases: Vec<Lease>,
}

impl<'a> ActionCtx<'a> {
    pub fn log(&self, line: &str) { if let Some(j) = self.job { j.log(line); } }
    pub fn cancelled(&self) -> bool { self.cancel.load(Ordering::Relaxed) || self.job.map(|j| j.cancelled()).unwrap_or(false) }

    /// Fresh private temp dir for this run (removed on drop).
    pub fn temp_dir(&mut self) -> anyhow::Result<PathBuf> {
        let d = self.settings.temp_dir().join(format!("job-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d)?;
        self.temps.push(d.clone());
        Ok(d)
    }

    /// Copy `src` (on the NAS) to local temp, checking free space first (needs ~2.2× the size).
    pub fn temp_copy(&mut self, src: &Path) -> anyhow::Result<PathBuf> {
        let dir = self.temp_dir()?;
        let size = self.fs.stat(src)?.size;
        let free = fs4::available_space(&dir).unwrap_or(u64::MAX);
        let need = size.saturating_mul(22) / 10;
        if free < need { anyhow::bail!("not enough space in {} (need {:.1} GB, have {:.1} GB)", p2s(&dir), need as f64 / 1e9, free as f64 / 1e9); }
        let dst = dir.join(src.file_name().unwrap_or_default());
        self.log(&format!("copying {} to local temp ({:.1} MB)", src.file_name().unwrap_or_default().to_string_lossy(), size as f64 / 1e6));
        self.fs.copy(src, &dst, &|_| {})?;
        Ok(dst)
    }

    /// Run an external program, streaming its output into the job log. Kills it on cancel.
    pub fn run_cmd(&mut self, program: &str, args: &[&std::ffi::OsStr], cwd: Option<&Path>) -> anyhow::Result<()> {
        let mut cmd = Command::new(program);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
        if let Some(c) = cwd { cmd.current_dir(c); }
        self.log(&format!("$ {program} {}", args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")));
        let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("could not start {program}: {e}"))?;
        let tail = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let mut readers = Vec::new();
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let tx = tx.clone();
            readers.push(std::thread::spawn(move || {
                for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
                    let _ = tx.send(String::from_utf8_lossy(&line).trim_end_matches('\r').to_string());
                }
            }));
        }
        drop(tx);
        let status = loop {
            while let Ok(l) = rx.try_recv() { self.log(&l); let mut t = tail.lock().unwrap(); t.push(l); if t.len() > 5 { t.remove(0); } }
            if self.cancelled() { let _ = child.kill(); let _ = child.wait(); anyhow::bail!("cancelled"); }
            if let Some(s) = child.try_wait()? { break s; }
            std::thread::sleep(Duration::from_millis(50));
        };
        for r in readers { let _ = r.join(); }
        while let Ok(l) = rx.try_recv() { self.log(&l); let mut t = tail.lock().unwrap(); t.push(l); if t.len() > 5 { t.remove(0); } }
        if !status.success() {
            anyhow::bail!("{program} failed ({status}): {}", tail.lock().unwrap().join(" | "));
        }
        Ok(())
    }

    /// Upload a finished local file to `dst` on the NAS (via `dst.lo-part`, then rename) and index it.
    pub fn place_output(&mut self, local: &Path, dst: &Path, like: &FileRow) -> anyhow::Result<()> {
        if self.fs.exists(dst) { anyhow::bail!("target already exists: {}", p2s(dst)); }
        let mut part = dst.as_os_str().to_owned();
        part.push(".lo-part");
        let part = PathBuf::from(part);
        self.log(&format!("uploading {}", dst.file_name().unwrap_or_default().to_string_lossy()));
        if let Err(e) = self.fs.copy(local, &part, &|_| {}) { let _ = self.fs.remove_file(&part); return Err(e.into()); }
        let r = apply(self.db, self.fs, self.journal, vec![Op::mv(part.clone(), dst.to_path_buf())], &format!("{}: output", self.action_id))?;
        if let Some((_, e)) = r.failed.first() { let _ = self.fs.remove_file(&part); anyhow::bail!("could not place output: {e}"); }
        self.index_file(dst, like)
    }

    fn index_file(&self, p: &Path, like: &FileRow) -> anyhow::Result<()> {
        let m = self.fs.stat(p)?;
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let dir_id: i64 = match self.db.conn().query_row("SELECT id FROM dirs WHERE path=?1", [p2s(p.parent().unwrap())], |r| r.get(0)).optional()? {
            Some(d) => d,
            None => self.db.conn().query_row("SELECT dir_id FROM files WHERE id=?1", [like.id], |r| r.get(0)).optional()?.unwrap_or(0),
        };
        self.db.conn().execute("INSERT INTO files(path, dir_id, system_id, name, ext, size, mtime, game_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                                ON CONFLICT(path) DO UPDATE SET size=excluded.size, mtime=excluded.mtime, game_id=excluded.game_id, probe_json=NULL, probe_key=NULL, sha1=NULL, crc32=NULL, md5=NULL, hash_key=NULL",
            (p2s(p), dir_id, &like.system_id, &name, ext_of(&name), m.size as i64, m.mtime, like.game_id))?;
        Ok(())
    }

    /// Move the original into the system's _archive with the configured hold.
    pub fn archive_original(&mut self, path: &Path, reason: &str) -> anyhow::Result<crate::archive::ArchiveEntry> {
        self.log(&format!("archiving original {}", path.file_name().unwrap_or_default().to_string_lossy()));
        archive_file(self.db, self.fs, self.journal, path, reason, Some(self.action_id), self.settings.archive_hold_days)
    }

    pub fn restore_archived(&mut self, e: &crate::archive::ArchiveEntry) -> anyhow::Result<()> {
        crate::archive::restore(self.db, self.fs, self.journal, e.id).map(|_| ())
    }

    pub fn set_status(&self, game_id: Option<i64>, key: &str, value: &str) -> anyhow::Result<()> {
        let Some(g) = game_id else { return Ok(()) };
        self.db.conn().execute("INSERT INTO status(game_id, key, value, updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(game_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
            (g, key, value, now()))?;
        if let Ok(k) = game_key(self.db, &self.journal.roots, g) {
            let _ = self.journal.append(&EntryKind::Status { game_key: k, key: key.into(), value: value.into() });
        }
        Ok(())
    }

    /// Take a cross-PC lease on `path` for the rest of this run.
    pub fn lease(&mut self, path: &Path) -> anyhow::Result<()> {
        let Some((i, _)) = rel_to_root(&self.journal.roots, path) else { return Ok(()) };
        let l = Lease::acquire(self.journal.fs.clone(), &self.journal.roots[i], path, &self.journal.machine, Duration::from_secs(6 * 3600))?;
        self.leases.push(l);
        Ok(())
    }

    /// Probe a local output file.
    pub fn probe_local(&self, kind: crate::probe::ProbeKind, p: &Path) -> anyhow::Result<Probe> {
        Ok(crate::probe::probe_file(kind, &crate::fsx::LocalFs, p)?)
    }
}

impl Drop for ActionCtx<'_> {
    fn drop(&mut self) {
        for t in &self.temps { let _ = std::fs::remove_dir_all(t); }
    }
}

/// Resolve a tool path from settings, else a default under the library roots.
pub fn tool_path(s: &Settings, key: &str, default_rel: &[&str]) -> Option<PathBuf> {
    if let Some(p) = s.tools.get(key) { return Some(p.clone()); }
    s.library_roots.iter().map(|r| default_rel.iter().fold(r.clone(), |p, c| p.join(c))).find(|p| p.exists())
}

/// The program to invoke for a tool: the configured path, else the bare name (found via PATH).
pub fn tool_program(s: &Settings, key: &str, default_name: &str) -> String {
    s.tools.get(key).map(|p| p2s(p)).unwrap_or_else(|| default_name.to_string())
}

/// Is `program` an existing file, or found on PATH?
pub fn program_exists(program: &str) -> bool {
    let p = Path::new(program);
    if p.components().count() > 1 { return p.is_file(); }
    let exts: Vec<String> = if cfg!(windows) { vec!["".into(), ".exe".into(), ".cmd".into(), ".bat".into()] } else { vec!["".into()] };
    std::env::var_os("PATH").map(|path| std::env::split_paths(&path).any(|d| exts.iter().any(|e| d.join(format!("{program}{e}")).is_file()))).unwrap_or(false)
}

pub fn tool_requirement(s: &Settings, key: &str, default_name: &str, hint: &str) -> Requirement {
    let prog = tool_program(s, key, default_name);
    Requirement { name: default_name.to_string(), ok: program_exists(&prog), hint: format!("{hint} Or set Settings → Tools → {key} to the {default_name} executable.") }
}

/// All files under `dir` (recursive).
pub fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p) } else { out.push(p) }
        }
    }
    out
}

/// The system folder (<root>/<first component>) containing `p`.
pub fn system_folder(roots: &[PathBuf], p: &Path) -> Option<PathBuf> {
    crate::archive::archive_dir_for(roots, p).ok().and_then(|a| a.parent().map(|x| x.to_path_buf()))
}

pub fn program_runs(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// Run `action` over `file_ids` one at a time. Failures are collected; the batch keeps going.
#[allow(clippy::too_many_arguments)]
pub fn execute(db: &Db, fs: &dyn Transport, settings: &Settings, journal: &Journal, action: &'static dyn Action, file_ids: &[i64], job: Option<&JobCtx>, cancel: &AtomicBool) -> anyhow::Result<ExecReport> {
    let rows = file_rows(db, file_ids)?;
    let total = rows.len() as u64;
    let mut rep = ExecReport::default();
    for (i, row) in rows.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) || job.map(|j| j.cancelled()).unwrap_or(false) { anyhow::bail!("cancelled"); }
        if let Some(j) = job { j.progress(i as u64, total, &row.name); }
        if !action.systems().contains(&row.system_id.as_str()) || !action.applies(row) {
            rep.skipped.push((row.path.clone(), "not applicable".into()));
            continue;
        }
        let mut ctx = ActionCtx { db, fs, settings, journal, action_id: action.id(), job, cancel, temps: vec![], leases: vec![] };
        let r = ctx.lease(&row.path).and_then(|_| action.run(&mut ctx, row));
        drop(ctx);
        match r {
            Ok(ActionOutcome::Done { output }) => { rep.ok += 1; if let Some(j) = job { j.log(&format!("✓ {} → {}", row.name, output.file_name().unwrap_or_default().to_string_lossy())); } }
            Ok(ActionOutcome::Skipped(why)) => rep.skipped.push((row.path.clone(), why)),
            Err(e) => { if let Some(j) = job { j.log(&format!("✗ {}: {e:#}", row.name)); } rep.failed.push((row.path.clone(), format!("{e:#}"))); }
        }
    }
    if let Some(j) = job { j.progress(total, total, "finished"); }
    Ok(rep)
}
