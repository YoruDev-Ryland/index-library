mod defs;
use crate::config::{FolderMapping, Settings};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ParserKind { NoIntro, GoodTools, Switch, Mame, Serial }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProbeKind { N3ds, Nds, Gba, PspPkg, Switch }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum GroupKind {
    /// one file = one game (twins by title+region merged)
    Single,
    /// .cue + .bin sets in one folder, multi-disc merged
    CueBin,
    /// every direct subfolder of the system folder is a game
    FolderPerGame,
    /// Switch: grouped by title id (base/update/dlc)
    SwitchTitle,
    /// PSN .pkg (Vita): grouped by the title id in the package's content id, so a game's
    /// updates and DLC sit under it. Files without a readable id fall back to their name.
    PkgTitle,
}

#[derive(Debug, Serialize)]
pub struct SystemDef {
    pub id: &'static str,
    pub name: &'static str,
    pub folders: &'static [&'static str],
    pub exts: &'static [&'static str],
    pub parsers: &'static [ParserKind],
    pub probe: Option<ProbeKind>,
    pub group: GroupKind,
    pub template: &'static str,
    pub igdb_platform: Option<u32>,
    pub ss_system: Option<u32>,
    pub sgdb_platform: Option<&'static str>,
}

impl SystemDef {
    pub fn accepts_ext(&self, ext: &str) -> bool {
        self.exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

pub fn registry() -> &'static [SystemDef] { defs::SYSTEMS }

pub fn by_id(id: &str) -> Option<&'static SystemDef> {
    registry().iter().find(|s| s.id == id)
}

pub enum Mapping {
    System(&'static SystemDef),
    Ignored,
    Unknown,
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Each system's folder for tools and their files; never scanned as games.
pub const TOOLS_DIR: &str = "_Tools";

pub fn is_ignored(settings: &Settings, name: &str) -> bool {
    name.eq_ignore_ascii_case(TOOLS_DIR) || settings.ignored.iter().any(|i| i.eq_ignore_ascii_case(name))
}

pub fn map_folder(settings: &Settings, folder_name: &str) -> Mapping {
    if let Some(m) = settings.folder_overrides.get(folder_name) {
        return match m {
            FolderMapping::Ignored => Mapping::Ignored,
            FolderMapping::System(id) => by_id(id).map(Mapping::System).unwrap_or(Mapping::Unknown),
        };
    }
    if is_ignored(settings, folder_name) { return Mapping::Ignored; }
    for s in registry() {
        if s.folders.iter().any(|f| *f == folder_name) { return Mapping::System(s); }
    }
    let n = norm(folder_name);
    for s in registry() {
        if s.folders.iter().any(|f| norm(f) == n) || norm(s.id) == n { return Mapping::System(s); }
    }
    Mapping::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Settings, FolderMapping};
    fn id(m: Mapping) -> String { match m { Mapping::System(s) => s.id.to_string(), Mapping::Ignored => "IGNORED".into(), Mapping::Unknown => "UNKNOWN".into() } }
    #[test]
    fn maps_folders() {
        let s = Settings::default();
        assert_eq!(id(map_folder(&s, "Nintendo - 3DS")), "n3ds");
        assert_eq!(id(map_folder(&s, "Emulator Software")), "IGNORED");
        assert_eq!(id(map_folder(&s, "Sony - PS1")), "ps1");
        assert_eq!(id(map_folder(&s, "nintendo-snes")), "snes");
        assert_eq!(id(map_folder(&s, "Random")), "UNKNOWN");
        let mut s2 = Settings::default();
        s2.folder_overrides.insert("Random".into(), FolderMapping::System("nes".into()));
        assert_eq!(id(map_folder(&s2, "Random")), "nes");
    }
    #[test]
    fn registry_ids_unique() {
        let mut ids: Vec<_> = registry().iter().map(|s| s.id).collect();
        let n = ids.len(); ids.sort(); ids.dedup();
        assert_eq!(n, ids.len());
        assert!(n >= 28);
    }
}
