//! Folder layout consistency. Each system has one way its games are laid out, the one most of its
//! games already use (or the one chosen in settings):
//!   - loose:    every game's files sit directly in the system folder
//!   - per-game: every game has its own folder
//!   - grouped:  shared folders that each hold many games (Games/, DLC/, Updates/)
//! Games that don't follow it are listed with the moves that would bring them in line. Duplicate
//! A file only goes to the archive when it is proven to hold nothing the kept files don't
//! (same fingerprint, or the same content pieces); everything else is unique and is moved, so
//! all of a game's content ends up in one place and nothing unique is lost.
use crate::config::Settings;
use crate::db::Db;
use crate::fsx::Transport;
use crate::ops::Op;
use crate::systems::{by_id, is_ignored, map_folder, GroupKind, Mapping};
use crate::util::p2s;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout { Loose, PerGame, Grouped, Leave }

impl Layout {
    pub fn parse(s: &str) -> Option<Layout> {
        match s { "loose" => Some(Layout::Loose), "per-game" => Some(Layout::PerGame), "grouped" => Some(Layout::Grouped), "leave" => Some(Layout::Leave), _ => None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Problem {
    /// one game's files are in several places
    Split,
    /// a loose game in a system whose games have their own folders
    NeedsFolder,
    /// a game in its own folder in a system whose games sit loose
    InOwnFolder,
    /// a loose game in a system whose games are filed in shared folders (no automatic fix)
    NotFiled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Move { pub file_id: i64, pub src: PathBuf, pub dst: PathBuf, pub size: u64 }

/// A file proven to be a copy of one that stays.
#[derive(Debug, Clone, Serialize)]
pub struct Spare { pub file_id: i64, pub src: PathBuf, pub size: u64, pub same_as: PathBuf, pub proof: crate::dupes::Proof, pub note: String }

#[derive(Debug, Clone, Serialize)]
pub struct LayoutItem {
    pub game_id: i64,
    pub title: String,
    pub problem: Problem,
    /// where the game's files end up (None when there is no automatic fix)
    pub target: Option<PathBuf>,
    /// the target folder doesn't exist yet
    pub new_folder: bool,
    /// the folders the game's files are in now, relative to the system folder ("" = the system folder)
    pub places: Vec<String>,
    pub moves: Vec<Move>,
    /// proven copies that go to the archive with the fix
    pub duplicates: Vec<Spare>,
    /// files left where they are, with why
    pub left: Vec<(PathBuf, String)>,
    /// the game, the problem and where its files are: for leaving this one as it is
    pub skip_key: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemLayout {
    pub system_id: String,
    pub detected: Layout,
    /// what the check follows: the setting if there is one, else `detected`
    pub layout: Layout,
    pub chosen: bool,
    /// games following each layout, and games split across places
    pub loose: usize,
    pub per_game: usize,
    pub grouped: usize,
    pub split: usize,
    pub items: Vec<LayoutItem>,
}

struct Row { id: i64, game: i64, title: String, path: PathBuf, size: u64, tid: Option<String> }

/// (system folder, the path below it)
fn locate(settings: &Settings, path: &Path) -> Option<(PathBuf, Vec<String>)> {
    for root in &settings.library_roots {
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if comps.len() < 2 { return None; }
        let sys_dir = root.join(&comps[0]);
        let below = comps[1..].to_vec();
        // tool folders, the archive and shared state are not part of the layout
        if below[..below.len() - 1].iter().any(|c| is_ignored(settings, c) || c == crate::archive::ARCHIVE_DIR || c == crate::shared::SHARED_DIR) { return None; }
        if !matches!(map_folder(settings, &comps[0]), Mapping::System(_)) { return None; }
        return Some((sys_dir, below));
    }
    None
}

fn base_id(tid: &str) -> Option<u64> { u64::from_str_radix(tid, 16).ok().map(|v| v & !0x1FFF) }
fn has_bracket_id(name: &str) -> bool {
    let b = name.as_bytes();
    (0..b.len().saturating_sub(17)).any(|i| b[i] == b'[' && b[i + 17] == b']' && b[i + 1..i + 17].iter().all(|c| c.is_ascii_hexdigit()))
}

pub fn check(db: &Db, settings: &Settings, system: Option<&str>) -> anyhow::Result<Vec<SystemLayout>> {
    // only proven copies go to the archive; any other extra is unique content and is moved
    let dupes = crate::dupes::find_dupes_with(db, system, &settings.region_order, settings.one_game_one_rom)?;
    let mut extras: std::collections::HashMap<i64, (PathBuf, crate::dupes::Proof, String)> = std::collections::HashMap::new();
    let mut keepers: HashSet<i64> = HashSet::new();
    for g in &dupes {
        // other-region releases are a Duplicates decision, never a side effect of tidying folders
        if matches!(g.reason, crate::dupes::DupeReason::SameTitleDiffRegion) { continue; }
        keepers.insert(g.keep); keepers.extend(g.keep_also.iter().copied());
        let keep_path = g.files.iter().position(|f| *f == g.keep).map(|i| PathBuf::from(&g.names[i])).unwrap_or_default();
        for (i, f) in g.files.iter().enumerate() {
            if g.proofs.get(i).map(|p| p.copy()).unwrap_or(false) { extras.insert(*f, (keep_path.clone(), g.proofs[i], g.notes.get(i).cloned().unwrap_or_default())); }
        }
    }
    extras.retain(|f, _| !keepers.contains(f));

    let mut q = db.conn().prepare(
        "SELECT f.id, f.game_id, f.system_id, g.title, f.path, f.size, COALESCE(json_extract(f.probe_json,'$.id'), '')
         FROM files f JOIN games g ON g.id=f.game_id WHERE f.is_dir=0 AND (?1 IS NULL OR f.system_id=?1) ORDER BY f.system_id, f.path")?;
    let rows = q.query_map([system], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?)))?;
    // system -> (system folder, rows with their path below it)
    let mut by_sys: BTreeMap<String, (PathBuf, Vec<(Row, Vec<String>)>)> = BTreeMap::new();
    for r in rows {
        let (id, game, sys, title, path, size, probe) = r?;
        let path = PathBuf::from(path);
        let Some((sys_dir, below)) = locate(settings, &path) else { continue };
        let name = below.last().cloned().unwrap_or_default();
        let named = by_id(&sys).filter(|s| s.group == GroupKind::SwitchTitle)
            .and_then(|s| crate::parse::parse(s.parsers, &crate::util::full_stem(&name)).title_id);
        let tid = Some(probe).filter(|p| p.len() == 16).or(named);
        let e = by_sys.entry(sys).or_insert_with(|| (sys_dir.clone(), Vec::new()));
        if e.0 != sys_dir { continue; } // a second library root's copy of the system: checked on its own below
        e.1.push((Row { id, game, title, path, size: size.max(0) as u64, tid }, below));
    }

    let mut out = Vec::new();
    for (sys, (sys_dir, rows)) in by_sys {
        out.push(check_system(settings, &sys, &sys_dir, &rows, &extras));
    }
    Ok(out)
}

fn check_system(settings: &Settings, sys: &str, sys_dir: &Path, rows: &[(Row, Vec<String>)], extras: &std::collections::HashMap<i64, (PathBuf, crate::dupes::Proof, String)>) -> SystemLayout {
    // games in each top-level folder ("" = loose in the system folder)
    let mut games_in: BTreeMap<String, BTreeSet<i64>> = BTreeMap::new();
    let mut places_of: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for (r, below) in rows {
        if extras.contains_key(&r.id) { continue; }
        let place = if below.len() == 1 { String::new() } else { below[0].clone() };
        games_in.entry(place.clone()).or_default().insert(r.game);
        places_of.entry(r.game).or_default().insert(place);
    }
    let own_folder = |p: &str| !p.is_empty() && games_in.get(p).map(|g| g.len() == 1).unwrap_or(false);
    let (mut loose, mut per_game, mut grouped, mut split) = (0, 0, 0, 0);
    for places in places_of.values() {
        if places.len() > 1 { split += 1; continue; }
        let p = places.iter().next().unwrap();
        if p.is_empty() { loose += 1 } else if own_folder(p) { per_game += 1 } else { grouped += 1 }
    }
    // the layout most games use; ties go to the one that moves less (loose, then grouped)
    let detected = [(loose, Layout::Loose), (grouped, Layout::Grouped), (per_game, Layout::PerGame)].into_iter()
        .max_by_key(|(n, l)| (*n, std::cmp::Reverse(*l))).map(|(_, l)| l).unwrap_or(Layout::Loose);
    let chosen_layout = settings.folder_layouts.get(sys).and_then(|s| Layout::parse(s));
    let layout = chosen_layout.unwrap_or(detected);

    // how this system names game folders and whether it keeps subfolders inside them
    let own: Vec<&String> = games_in.keys().filter(|p| own_folder(p)).collect();
    let id_names = !own.is_empty() && own.iter().filter(|p| has_bracket_id(p)).count() * 2 >= own.len();
    let in_own: Vec<&Vec<String>> = rows.iter().filter(|(_, b)| b.len() > 1 && own_folder(&b[0])).map(|(_, b)| b).collect();
    let nested = !in_own.is_empty() && in_own.iter().filter(|b| b.len() > 2).count() * 2 > in_own.len();

    let mut taken: HashSet<PathBuf> = rows.iter().map(|(r, _)| r.path.clone()).collect();
    let mut items = Vec::new();
    let mut by_game: BTreeMap<i64, Vec<&(Row, Vec<String>)>> = BTreeMap::new();
    for row in rows { by_game.entry(row.0.game).or_default().push(row); }
    for (game, files) in by_game {
        let Some(places) = places_of.get(&game) else { continue };
        let is_split = places.len() > 1;
        let place = places.iter().next().unwrap();
        let problem = match layout {
            Layout::Leave => None,
            Layout::PerGame if is_split => Some(Problem::Split),
            Layout::PerGame if place.is_empty() || !own_folder(place) => Some(Problem::NeedsFolder),
            // a game split over a group folder and the system folder is normal for some layouts
            // (Games/ + Updates/); only loose files stand out there
            Layout::Grouped if places.contains("") => Some(Problem::NotFiled),
            Layout::Loose if is_split => Some(Problem::Split),
            Layout::Loose if own_folder(place) => Some(Problem::InOwnFolder),
            _ => None,
        };
        let Some(problem) = problem else { continue };
        let title = files[0].0.title.clone();
        let mut item = LayoutItem { game_id: game, title: title.clone(), problem, target: None, new_folder: false, places: vec![], moves: vec![], duplicates: vec![], left: vec![], skip_key: String::new() };
        let mut shown: BTreeSet<String> = BTreeSet::new();
        for (_, b) in &files { shown.insert(if b.len() == 1 { String::new() } else { b[0].clone() }); }
        item.places = shown.into_iter().collect();
        let target: Option<(PathBuf, bool)> = match (layout, problem) {
            (_, Problem::NotFiled) => None,
            (Layout::Loose, _) => Some((sys_dir.to_path_buf(), false)),
            _ => {
                // the game's own folder: one named with its id, else the one holding the most of it
                let base = files.iter().filter_map(|(r, _)| r.tid.as_deref().and_then(base_id)).min();
                let base_hex = base.map(|b| format!("{b:016X}"));
                let mut owned: Vec<(&String, u64, bool)> = places.iter().filter(|p| own_folder(p)).map(|p| {
                    let bytes = files.iter().filter(|(_, b)| b.len() > 1 && &b[0] == p).map(|(r, _)| r.size).sum();
                    (p, bytes, base_hex.as_ref().map(|h| p.to_uppercase().contains(h.as_str())).unwrap_or(false))
                }).collect();
                owned.sort_by_key(|(p, bytes, id)| (!id, std::cmp::Reverse(*bytes), (*p).clone()));
                match owned.first() {
                    Some((p, _, _)) => Some((sys_dir.join(p), false)),
                    None => {
                        let mut name = crate::rename::sanitize_filename(&title);
                        if id_names { if let Some(h) = &base_hex { name = format!("{name} [{h}]"); } }
                        if name.is_empty() { None } else { Some((sys_dir.join(name), true)) }
                    }
                }
            }
        };
        if let Some((dir, new)) = target {
            item.new_folder = new && !games_in.contains_key(&p2s(dir.strip_prefix(sys_dir).unwrap_or(&dir)));
            for (r, below) in &files {
                if let Some((same_as, proof, note)) = extras.get(&r.id) {
                    item.duplicates.push(Spare { file_id: r.id, src: r.path.clone(), size: r.size, same_as: same_as.clone(), proof: *proof, note: note.clone() });
                    continue;
                }
                if r.path.starts_with(&dir) && dir != sys_dir { continue; }
                if dir == sys_dir && below.len() == 1 { continue; }
                // keep the path inside the old folder when this system keeps subfolders in game folders
                let inner: PathBuf = if nested && below.len() > 2 { below[1..].iter().collect() } else { PathBuf::from(below.last().unwrap()) };
                let mut dst = dir.join(&inner);
                // a different file already has this name: keep both, numbering the newcomer
                let mut n = 2;
                while taken.contains(&dst) {
                    let name = inner.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                    let (stem, ext) = (crate::util::full_stem(&name), crate::util::full_ext(&name));
                    dst = dir.join(inner.parent().unwrap_or(Path::new(""))).join(if ext.is_empty() { format!("{stem} ({n})") } else { format!("{stem} ({n}).{ext}") });
                    n += 1;
                }
                taken.insert(dst.clone());
                item.moves.push(Move { file_id: r.id, src: r.path.clone(), dst, size: r.size });
            }
            item.target = Some(dir);
            if item.moves.is_empty() && item.duplicates.is_empty() { continue; }
        } else {
            item.left = files.iter().map(|(r, _)| (r.path.clone(), "no automatic fix — move it into one of the folders yourself".into())).collect();
        }
        items.push(item);
    }
    SystemLayout { system_id: sys.to_string(), detected, layout, chosen: chosen_layout.is_some(), loose, per_game, grouped, split, items }
}

/// Moves for the chosen games, then removal of the folders they leave empty (deepest first).
/// A folder that still holds something (a duplicate, a readme) is kept.
pub fn to_ops(fs: &dyn Transport, items: &[&LayoutItem], sys_dir_of: impl Fn(&Path) -> Option<PathBuf>) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    // spare copies are archived before these ops run; they leave their folders too
    let leaving: Vec<&PathBuf> = items.iter().flat_map(|i| i.moves.iter().map(|m| &m.src).chain(i.duplicates.iter().map(|d| &d.src))).collect();
    let moving: HashSet<&Path> = leaving.iter().map(|p| p.as_path()).collect();
    for i in items {
        for m in &i.moves { ops.push(Op::mv(m.src.clone(), m.dst.clone())); }
        for src in i.moves.iter().map(|m| &m.src).chain(i.duplicates.iter().map(|d| &d.src)) {
            let stop = sys_dir_of(src);
            let mut d = src.parent();
            while let Some(p) = d {
                if Some(p) == stop.as_deref() || i.target.as_deref().map(|t| t.starts_with(p)).unwrap_or(false) { break; }
                dirs.insert(p.to_path_buf());
                d = p.parent();
            }
        }
    }
    // only folders that will be empty once the moves are done
    let mut deepest: Vec<PathBuf> = dirs.into_iter().collect();
    deepest.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut emptied: HashSet<PathBuf> = HashSet::new();
    for d in deepest {
        let Ok(entries) = fs.list(&d) else { continue };
        if entries.iter().all(|e| { let p = d.join(&e.name); if e.is_dir { emptied.contains(&p) } else { moving.contains(p.as_path()) } }) {
            ops.push(Op::remove_empty_dir(d.clone()));
            emptied.insert(d);
        }
    }
    ops
}
