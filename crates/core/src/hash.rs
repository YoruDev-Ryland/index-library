//! CRC32 + SHA1 + MD5 in one pass, with a live-adjustable throttle.
//! The level can change while a job runs: workers above the current
//! worker count idle, and a token bucket enforces the bandwidth cap.
use crate::db::Db;
use crate::fsx::Transport;
use serde::{Deserialize, Serialize};
use sha1::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashLevel { Background, Normal, Fast, Maximum }

impl HashLevel {
    pub fn workers(self) -> usize {
        match self {
            HashLevel::Background => 1,
            HashLevel::Normal => 2,
            HashLevel::Fast => 4,
            HashLevel::Maximum => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
        }
    }
    /// bytes per second; None = unlimited
    pub fn cap(self) -> Option<u64> {
        match self {
            HashLevel::Background => Some(25_000_000),
            HashLevel::Normal => Some(100_000_000),
            HashLevel::Fast => Some(250_000_000),
            HashLevel::Maximum => None,
        }
    }
    fn from_u8(v: u8) -> HashLevel { [HashLevel::Background, HashLevel::Normal, HashLevel::Fast, HashLevel::Maximum][v.min(3) as usize] }
    pub fn parse(s: &str) -> Option<HashLevel> {
        Some(match s.to_lowercase().as_str() { "background" => Self::Background, "normal" => Self::Normal, "fast" => Self::Fast, "max" | "maximum" => Self::Maximum, _ => return None })
    }
}

struct Inner {
    level: AtomicU8,
    paused: AtomicBool,
    custom_cap: AtomicU64, // 0 = none
    bucket: Mutex<(f64, Instant)>,
}

#[derive(Clone)]
pub struct Throttle(Arc<Inner>);

impl Throttle {
    pub fn new(level: HashLevel) -> Throttle {
        Throttle(Arc::new(Inner { level: AtomicU8::new(level as u8), paused: AtomicBool::new(false), custom_cap: AtomicU64::new(0), bucket: Mutex::new((0.0, Instant::now())) }))
    }
    pub fn level(&self) -> HashLevel { HashLevel::from_u8(self.0.level.load(Ordering::Relaxed)) }
    pub fn set_level(&self, l: HashLevel) { self.0.level.store(l as u8, Ordering::Relaxed); }
    pub fn pause(&self) { self.0.paused.store(true, Ordering::Relaxed); }
    pub fn resume(&self) { self.0.paused.store(false, Ordering::Relaxed); }
    pub fn is_paused(&self) -> bool { self.0.paused.load(Ordering::Relaxed) }
    /// Override the level's bandwidth cap (bytes/sec). Used by tests and advanced settings.
    pub fn set_custom_cap(&self, cap: Option<u64>) { self.0.custom_cap.store(cap.unwrap_or(0), Ordering::Relaxed); }
    fn cap(&self) -> Option<u64> {
        match self.0.custom_cap.load(Ordering::Relaxed) { 0 => self.level().cap(), c => Some(c) }
    }
    fn wait_unpaused(&self, cancel: &AtomicBool) {
        while self.is_paused() && !cancel.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_millis(100)); }
    }
    /// Block until `bytes` may be consumed.
    fn acquire(&self, bytes: u64, cancel: &AtomicBool) {
        self.wait_unpaused(cancel);
        let Some(cap) = self.cap() else { return };
        let cap = cap as f64;
        let wait = {
            let mut b = self.0.bucket.lock().unwrap();
            let now = Instant::now();
            let refill = now.duration_since(b.1).as_secs_f64() * cap;
            b.0 = (b.0 + refill).min(cap); // at most 1s of burst
            b.1 = now;
            b.0 -= bytes as f64;
            if b.0 < 0.0 { Duration::from_secs_f64(-b.0 / cap) } else { Duration::ZERO }
        };
        if !wait.is_zero() { std::thread::sleep(wait); }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct HashReport {
    pub hashed: u64,
    pub skipped: u64,
    pub bytes: u64,
    pub failed: Vec<(PathBuf, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HashProgress { pub done: u64, pub total: u64, pub bytes: u64 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashes {
    pub crc32: String, pub sha1: String, pub md5: String,
    /// (crc32, sha1) of the ROM data without its header, for ROMs that carry one: No-Intro lists
    /// NES, SNES, Atari 7800 and Lynx games by their data alone
    pub headerless: Option<(String, String)>,
}

/// Systems whose ROM files may start with a header that No-Intro leaves out of its fingerprints.
pub const HEADER_SYSTEMS: &[&str] = &["nes", "snes", "atari7800", "lynx"];

/// The length of a ROM header at the start of `first`: iNES (16), Lynx (64), Atari 7800 (128), or
/// an SNES copier header (512, when the size is 512 over a multiple of 1 KB).
pub fn header_len(system: &str, first: &[u8], size: Option<u64>) -> usize {
    match system {
        "nes" if first.starts_with(b"NES\x1a") => 16,
        "lynx" if first.starts_with(b"LYNX") => 64,
        "atari7800" if first.get(1..10) == Some(b"ATARI7800") => 128,
        "snes" if size.is_some_and(|n| n % 1024 == 512) => 512,
        _ => 0,
    }
}

fn digest(r: &mut dyn Read, system: &str, size: Option<u64>, t: &Throttle, cancel: &AtomicBool, bytes: &AtomicU64) -> std::io::Result<Hashes> {
    let mut crc = crc32fast::Hasher::new();
    let mut sha = sha1::Sha1::new();
    let mut md = md5::Md5::new();
    let (mut hcrc, mut hsha) = (crc32fast::Hasher::new(), sha1::Sha1::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut skip: Option<usize> = None; // header bytes still to leave out of the headerless hash
    let mut header = 0;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled")); }
        // the first read fills enough to see a header
        let mut n = r.read(&mut buf)?;
        if skip.is_none() { while n > 0 && n < 512 { let m = r.read(&mut buf[n..])?; if m == 0 { break; } n += m; } }
        if n == 0 { break; }
        let s = *skip.get_or_insert_with(|| { header = header_len(system, &buf[..n], size); header });
        t.acquire(n as u64, cancel);
        crc.update(&buf[..n]); sha.update(&buf[..n]); md.update(&buf[..n]);
        let from = s.min(n);
        if header > 0 { hcrc.update(&buf[from..n]); hsha.update(&buf[from..n]); }
        skip = Some(s - from);
        bytes.fetch_add(n as u64, Ordering::Relaxed);
    }
    let headerless = (header > 0).then(|| (format!("{:08x}", hcrc.finalize()), hex(&hsha.finalize())));
    Ok(Hashes { crc32: format!("{:08x}", crc.finalize()), sha1: hex(&sha.finalize()), md5: hex(&md.finalize()), headerless })
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }

/// Hash a file; for .zip the largest inner entry (the ROM) is hashed, as DATs list ROM hashes.
pub fn hash_one(fs: &dyn Transport, path: &Path, ext: &str, system: &str, size: u64, t: &Throttle, cancel: &AtomicBool, bytes: &AtomicU64) -> std::io::Result<Hashes> {
    if ext == "zip" {
        let rs = fs.open_seek(path)?;
        let mut z = zip::ZipArchive::new(rs).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut best: Option<(usize, u64)> = None;
        for i in 0..z.len() {
            let e = z.by_index_raw(i).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            if e.is_dir() { continue; }
            if best.map(|b| e.size() > b.1).unwrap_or(true) { best = Some((i, e.size())); }
        }
        if let Some((i, _)) = best {
            let mut e = z.by_index(i).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let n = e.size();
            return digest(&mut e, system, Some(n), t, cancel, bytes);
        }
    }
    let mut r = fs.open_read(path)?;
    digest(&mut r, system, Some(size), t, cancel, bytes)
}

pub fn hash_files(db: &Db, fs: &dyn Transport, file_ids: &[i64], throttle: &Throttle, progress: &dyn Fn(HashProgress), cancel: &AtomicBool) -> anyhow::Result<HashReport> {
    let mut rep = HashReport::default();
    let mut todo: Vec<(i64, PathBuf, String, String, String, u64)> = Vec::new();
    {
        let mut q = db.conn().prepare("SELECT path, ext, size, mtime, hash_key, system_id FROM files WHERE id=?1")?;
        for id in file_ids {
            let row: Option<(String, String, i64, i64, Option<String>, String)> = q.query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).ok();
            let Some((path, ext, size, mtime, hk, system)) = row else { continue };
            let key = format!("{size}:{mtime}");
            if hk.as_deref() == Some(key.as_str()) { rep.skipped += 1; continue; }
            todo.push((*id, PathBuf::from(path), ext, key, system, size.max(0) as u64));
        }
    }
    let total = todo.len() as u64;
    let next = AtomicUsize::new(0);
    let bytes = AtomicU64::new(0);
    let (tx, rx) = std::sync::mpsc::channel::<(i64, String, std::io::Result<Hashes>, PathBuf)>();
    let max_workers = HashLevel::Maximum.workers();
    std::thread::scope(|sc| {
        for w in 0..max_workers {
            let tx = tx.clone();
            let (todo, next, bytes) = (&todo, &next, &bytes);
            sc.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) { break; }
                if w >= throttle.level().workers() || throttle.is_paused() {
                    if next.load(Ordering::Relaxed) >= todo.len() { break; }
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= todo.len() { break; }
                let (id, path, ext, key, system, size) = &todo[i];
                let r = hash_one(fs, path, ext, system, *size, throttle, cancel, bytes);
                if tx.send((*id, key.clone(), r, path.clone())).is_err() { break; }
            });
        }
        drop(tx);
        // Write results on this thread (rusqlite is not Sync).
        let mut done = 0;
        for (id, key, r, path) in rx {
            done += 1;
            match r {
                Ok(h) => {
                    let (hc, hs) = h.headerless.clone().unzip();
                    let _ = db.conn().execute("UPDATE files SET crc32=?1, sha1=?2, md5=?3, hash_key=?4, dat_status=NULL, headerless_crc32=?5, headerless_sha1=?6 WHERE id=?7", (&h.crc32, &h.sha1, &h.md5, &key, hc, hs, id));
                    rep.hashed += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => rep.failed.push((path, e.to_string())),
            }
            progress(HashProgress { done, total, bytes: bytes.load(Ordering::Relaxed) });
        }
    });
    rep.bytes = bytes.load(Ordering::Relaxed);
    if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled"); }
    Ok(rep)
}
