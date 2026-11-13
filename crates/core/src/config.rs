use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "system")]
pub enum FolderMapping {
    System(String),
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ApiKeys {
    pub steamgriddb: String,
    pub igdb_client_id: String,
    pub igdb_client_secret: String,
    pub screenscraper_user: String,
    pub screenscraper_password: String,
    pub screenscraper_devid: String,
    pub screenscraper_devpassword: String,
    /// rawg.io: a free key from rawg.io/apidocs
    pub rawg: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library_roots: Vec<PathBuf>,
    pub folder_overrides: BTreeMap<String, FolderMapping>,
    pub ignored: Vec<String>,
    pub archive_hold_days: u32,
    pub temp_dir: Option<PathBuf>,
    pub python: String,
    pub tools: BTreeMap<String, PathBuf>,
    pub keys: ApiKeys,
    pub scan_concurrency: usize,
    pub region_order: Vec<String>,
    pub hash_level: String,
    /// System ids pinned to the Favorites group in the sidebar, in order.
    pub favorite_systems: Vec<String>,
    /// Per-system folder layout chosen by the user ("loose", "per-game", "grouped", "leave");
    /// systems not listed follow the layout most of their games already use.
    pub folder_layouts: BTreeMap<String, String>,
    /// 1G1R: keep one release per game (the best region); other-region releases count as duplicates.
    pub one_game_one_rom: bool,
    /// "system" (follow the OS), "light" or "dark"
    pub theme: String,
    /// the sync server this PC connects to (this machine's; never synced)
    pub sync: SyncSettings,
    /// what prices on the shelf are in (ISO code: USD, EUR, GBP…)
    pub currency: String,
    /// look for new versions of Index on GitHub (this PC's choice; never synced)
    pub auto_update: bool,
}

/// A library folder as sync knows it: its id on every device, where it is on this one, and
/// where it was on the device that first synced it (shown when mapping it on a new device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RootMap { pub id: String, pub path: PathBuf, pub origin: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SyncSettings {
    /// empty: not connected
    pub url: String,
    pub token: String,
    pub device_id: String,
    pub roots: Vec<RootMap>,
}

impl SyncSettings {
    pub fn enabled(&self) -> bool { !self.url.is_empty() && !self.token.is_empty() }
}

pub const DEFAULT_IGNORED: &[&str] = &[
    "Emulator Software", "hearto", "3DS-Decrypt", "Decryptor (Old)", "PSPDecrypter",
    "_archive", ".index-library",
];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library_roots: vec![],
            folder_overrides: BTreeMap::new(),
            ignored: DEFAULT_IGNORED.iter().map(|s| s.to_string()).collect(),
            archive_hold_days: 7,
            temp_dir: None,
            python: if cfg!(windows) { "python".into() } else { "python3".into() },
            tools: BTreeMap::new(),
            keys: ApiKeys::default(),
            scan_concurrency: 16,
            region_order: ["USA", "World", "Europe", "Japan"].iter().map(|s| s.to_string()).collect(),
            hash_level: "background".into(),
            favorite_systems: vec![],
            folder_layouts: BTreeMap::new(),
            one_game_one_rom: true,
            theme: "system".into(),
            sync: SyncSettings::default(),
            currency: "USD".into(),
            auto_update: true,
        }
    }
}

pub fn project_dirs() -> directories::ProjectDirs {
    directories::ProjectDirs::from("", "", "index-library").expect("no home directory")
}

impl Settings {
    pub fn default_path() -> PathBuf {
        project_dirs().config_dir().join("settings.toml")
    }
    pub fn load(path: &Path) -> anyhow::Result<Settings> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(toml::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(d) = path.parent() { std::fs::create_dir_all(d)?; }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
    /// The settings every device shares (sent to the sync server, API keys included). Library
    /// folders go as their sync ids; where they're mounted stays on each machine.
    pub fn shared_json(&self) -> serde_json::Value {
        serde_json::json!({
            "roots": self.sync.roots.iter().map(|r| serde_json::json!({"id": r.id, "origin": r.origin})).collect::<Vec<_>>(),
            "folder_overrides": self.folder_overrides, "ignored": self.ignored, "archive_hold_days": self.archive_hold_days,
            "region_order": self.region_order, "favorite_systems": self.favorite_systems, "folder_layouts": self.folder_layouts,
            "one_game_one_rom": self.one_game_one_rom, "theme": self.theme, "keys": self.keys, "currency": self.currency,
        })
    }
    /// Take the shared settings from another device (fields it doesn't have are kept).
    pub fn apply_shared(&mut self, v: &serde_json::Value) {
        fn take<T: serde::de::DeserializeOwned>(v: &serde_json::Value, k: &str, into: &mut T) {
            if let Some(x) = v.get(k).and_then(|x| serde_json::from_value(x.clone()).ok()) { *into = x; }
        }
        take(v, "folder_overrides", &mut self.folder_overrides);
        take(v, "ignored", &mut self.ignored);
        take(v, "archive_hold_days", &mut self.archive_hold_days);
        take(v, "region_order", &mut self.region_order);
        take(v, "favorite_systems", &mut self.favorite_systems);
        take(v, "folder_layouts", &mut self.folder_layouts);
        take(v, "one_game_one_rom", &mut self.one_game_one_rom);
        take(v, "theme", &mut self.theme);
        take(v, "keys", &mut self.keys);
        take(v, "currency", &mut self.currency);
    }
    pub fn temp_dir(&self) -> PathBuf {
        self.temp_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("index-library"))
    }
}

/// Stable per-machine id persisted in the local data dir.
pub fn machine_id(data_dir: &Path) -> anyhow::Result<String> {
    let p = data_dir.join("machine-id");
    if let Ok(s) = std::fs::read_to_string(&p) {
        let s = s.trim().to_string();
        if !s.is_empty() { return Ok(s); }
    }
    std::fs::create_dir_all(data_dir)?;
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::write(&p, &id)?;
    Ok(id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        let s = Settings::default();
        s.save(&p).unwrap();
        let l = Settings::load(&p).unwrap();
        assert_eq!(s, l);
        assert_eq!(l.archive_hold_days, 7);
        assert!(l.ignored.iter().any(|x| x == "Emulator Software"));
        assert!(l.ignored.iter().any(|x| x == "_archive"));
        assert!(l.favorite_systems.is_empty());
    }
    #[test]
    fn favorites_persist_and_old_files_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        std::fs::write(&p, "archive_hold_days = 3\n").unwrap();
        let mut s = Settings::load(&p).unwrap();
        assert_eq!(s.archive_hold_days, 3);
        s.favorite_systems = vec!["n3ds".into(), "switch".into()];
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap().favorite_systems, vec!["n3ds", "switch"]);
    }
    #[test]
    fn machine_id_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let a = machine_id(dir.path()).unwrap();
        assert_eq!(a, machine_id(dir.path()).unwrap());
    }
    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let l = Settings::load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(l, Settings::default());
    }
}
