//! Short-lived file leases so two PCs never run an action on the same file.
use super::{now_ms, rel_to_root, shared_dir};
use crate::fsx::Transport;
use serde::{Deserialize, Serialize};
use sha1::Digest;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LeaseFile { machine: String, path: String, acquired: i64, expires: i64 }

#[derive(Debug, thiserror::Error)]
pub enum LeaseError {
    #[error("{path} is being worked on by {machine}")]
    Busy { machine: String, path: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub struct Lease { fs: Arc<dyn Transport>, file: PathBuf, rel: String, machine: String, ttl: Duration, released: bool }

fn lock_path(root: &Path, rel: &str) -> PathBuf {
    let h = sha1::Sha1::digest(rel.as_bytes());
    let name: String = h.iter().take(10).map(|b| format!("{b:02x}")).collect();
    shared_dir(root).join("locks").join(format!("{name}.json"))
}

fn read(fs: &dyn Transport, p: &Path) -> Option<LeaseFile> {
    let m = fs.stat(p).ok()?;
    serde_json::from_slice(&fs.read_range(p, 0, m.size as usize).ok()?).ok()
}

impl Lease {
    pub fn acquire(fs: Arc<dyn Transport>, root: &Path, target: &Path, machine: &str, ttl: Duration) -> Result<Lease, LeaseError> {
        let rel = rel_to_root(&[root.to_path_buf()], target).map(|(_, r)| r).unwrap_or_else(|| target.to_string_lossy().into_owned());
        let file = lock_path(root, &rel);
        if let Some(cur) = read(&*fs, &file) {
            if cur.machine != machine && cur.expires > now_ms() {
                return Err(LeaseError::Busy { machine: cur.machine, path: rel });
            }
        }
        let l = Lease { fs, file, rel, machine: machine.to_string(), ttl, released: false };
        l.write()?;
        // Read back: if another machine raced us, theirs won.
        match read(&*l.fs, &l.file) {
            Some(cur) if cur.machine == machine => Ok(l),
            Some(cur) => { let mut l = l; l.released = true; Err(LeaseError::Busy { machine: cur.machine, path: l.rel.clone() }) }
            None => Ok(l),
        }
    }
    fn write(&self) -> std::io::Result<()> {
        let now = now_ms();
        let lf = LeaseFile { machine: self.machine.clone(), path: self.rel.clone(), acquired: now, expires: now + self.ttl.as_millis() as i64 };
        self.fs.write_atomic(&self.file, &serde_json::to_vec(&lf).unwrap())
    }
    pub fn renew(&self) -> std::io::Result<()> { self.write() }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.released { return; }
        if read(&*self.fs, &self.file).map(|c| c.machine == self.machine).unwrap_or(false) {
            let _ = self.fs.remove_file(&self.file);
        }
    }
}
