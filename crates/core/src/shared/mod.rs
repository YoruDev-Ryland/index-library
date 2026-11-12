//! State shared between PCs via small files on the NAS (never SQLite over SMB).
pub mod journal;
pub mod lease;
pub mod manifest;

use std::path::{Component, Path, PathBuf};

pub const SHARED_DIR: &str = ".index-library";

pub fn shared_dir(root: &Path) -> PathBuf { root.join(SHARED_DIR) }

/// Which root contains `p`, and the root-relative path with '/' separators.
pub fn rel_to_root(roots: &[PathBuf], p: &Path) -> Option<(usize, String)> {
    for (i, r) in roots.iter().enumerate() {
        if let Ok(rest) = p.strip_prefix(r) {
            let parts: Vec<String> = rest.components().filter_map(|c| match c { Component::Normal(s) => Some(s.to_string_lossy().into_owned()), _ => None }).collect();
            return Some((i, parts.join("/")));
        }
    }
    None
}

pub fn abs_from(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) { p.push(part); }
    p
}

pub fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }
