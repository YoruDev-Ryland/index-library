//! Optional sync with an Index sync server: everything a library knows, as records the server
//! stores and hands to other devices (phones, other PCs, a PC being rebuilt).
pub mod portable;
pub mod record;
pub mod export;

/// The ids sync knows this machine's library folders by.
pub fn roots_of(s: &crate::config::Settings) -> portable::Roots {
    portable::Roots { ids: s.sync.roots.iter().filter(|r| !r.path.as_os_str().is_empty()).map(|r| (r.id.clone(), r.path.clone())).collect() }
}

/// Where synced covers are kept: named by their sha256.
pub fn cover_dir(cache_dir: &std::path::Path) -> std::path::PathBuf { cache_dir.join("art").join("sync") }
pub mod client;
pub mod apply;
pub mod run;
