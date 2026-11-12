//! <system>/_archive/manifest.json — survives a lost local DB and is visible to every PC.
use crate::fsx::Transport;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// file name inside the _archive folder
    pub file: String,
    /// root-relative original location ('/' separators)
    pub original_path: String,
    pub archived_at: i64,
    pub delete_after: i64,
    pub reason: String,
    pub action: Option<String>,
    pub size: u64,
    pub machine: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest { pub entries: Vec<ManifestEntry> }

pub const MANIFEST: &str = "manifest.json";

impl Manifest {
    pub fn load(fs: &dyn Transport, archive_dir: &Path) -> anyhow::Result<Manifest> {
        let p = archive_dir.join(MANIFEST);
        match fs.stat(&p) {
            Ok(m) => Ok(serde_json::from_slice(&fs.read_range(&p, 0, m.size as usize)?)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, fs: &dyn Transport, archive_dir: &Path) -> anyhow::Result<()> {
        fs.write_atomic(&archive_dir.join(MANIFEST), &serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
