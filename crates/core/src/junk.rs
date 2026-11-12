//! Things that don't belong in system folders. Listing only (no content reads).
use crate::config::Settings;
use crate::db::Db;
use crate::fsx::Transport;
use crate::systems::{is_ignored, map_folder, registry, Mapping, SystemDef};
use crate::util::ext_of;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum JunkKind {
    UrlShortcut, EmptyDir, RomInToolFolder, Executable, TempLeftover, ForeignSystem,
    /// a folder holding a program (and its own files): belongs in the system's _Tools folder
    ToolFolder,
    /// scripts loose in a system folder, with the lists and notes they work on
    ToolFiles,
    /// a skipped tool folder sitting in the system folder rather than in _Tools
    LooseToolFolder,
}

#[derive(Debug, Clone, Serialize)]
pub struct JunkItem {
    pub path: PathBuf, pub kind: JunkKind, pub system_id: String, pub suggestion: String, pub size: u64,
    /// ToolFiles: every file that moves together (the scripts and the files beside them)
    #[serde(default)]
    pub members: Vec<PathBuf>,
}

const EXEC_EXTS: &[&str] = &["exe", "bat", "cmd", "dll", "py", "sh", "msi", "apk", "jar"];
/// Files that travel with scripts: the lists, notes and settings they read.
const COMPANION_EXTS: &[&str] = &["txt", "tsv", "csv", "json", "ini", "cfg", "md", "db", "xml"];
const SHORTCUT_EXTS: &[&str] = &["url", "lnk", "desktop", "webloc"];
const ROMLIKE: &[&str] = &["zip", "7z", "rar", "iso", "bin", "cue", "chd", "cso", "pbp", "img", "nsp", "xci", "cia", "3ds", "nds", "gba", "gb", "gbc", "nes", "sfc", "smc", "md", "gcm", "rvz", "pkg", "vpk"];

/// Is this folder a program's own folder: a program somewhere inside, and at most a few small
/// game-type files (a tool's data, like PSX2PSP's BASE.PBP)? A folder of games that happens to
/// hold a program is not: moving it would hide the games.
fn is_program_folder(fs: &dyn Transport, sys: &SystemDef, dir: &Path) -> bool {
    let (mut program, mut games, mut big) = (false, 0, false);
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0;
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs.list(&d) else { continue };
        for c in entries {
            if c.is_dir { stack.push(d.join(&c.name)); continue; }
            let ext = ext_of(&c.name);
            if EXEC_EXTS.contains(&ext.as_str()) { program = true; }
            else if sys.accepts_ext(&ext) { games += 1; big |= c.size > 64 << 20; }
        }
        seen += 1;
        if seen > 200 || games > 3 || big { return false; }
    }
    program
}

fn is_temp(name: &str) -> bool {
    let l = name.to_lowercase();
    [".lo-part", ".lo-tmp", ".part", ".tmp", ".crdownload", ".partial"].iter().any(|s| l.ends_with(s))
}

fn classify(sys: &SystemDef, dir: &Path, name: &str, size: u64, in_tool: bool, tool_has_program: bool, out: &mut Vec<JunkItem>) {
    let ext = ext_of(name);
    let path = dir.join(name);
    let push = |kind, suggestion: String, out: &mut Vec<JunkItem>| out.push(JunkItem { path: path.clone(), kind, system_id: sys.id.into(), suggestion, size, members: vec![] });
    if is_temp(name) { push(JunkKind::TempLeftover, "Leftover from an interrupted copy — safe to delete.".into(), out); return; }
    if in_tool {
        // a program's own small data files (PSX2PSP's BASE.PBP) aren't stray games; anything
        // game-sized is, even beside a program (a game left in a decrypter's folder)
        if sys.accepts_ext(&ext) && !EXEC_EXTS.contains(&ext.as_str()) && !(tool_has_program && size < 16 << 20) {
            push(JunkKind::RomInToolFolder, format!("Game file inside a tool folder — move it into the {} folder.", sys.name), out);
        }
        return;
    }
    if SHORTCUT_EXTS.contains(&ext.as_str()) { push(JunkKind::UrlShortcut, "Web shortcut — archive or delete.".into(), out); return; }
    if sys.accepts_ext(&ext) { return; }
    if EXEC_EXTS.contains(&ext.as_str()) && sys.id != "pc" { push(JunkKind::Executable, "Program inside a game folder — move it into the system's _Tools folder.".into(), out); return; }
    if ROMLIKE.contains(&ext.as_str()) {
        let other: Vec<&str> = registry().iter().filter(|s| s.id != sys.id && s.accepts_ext(&ext)).map(|s| s.name).take(3).collect();
        push(JunkKind::ForeignSystem, format!("Not a {} file type (.{ext}) — review; may belong to {}.", sys.name, if other.is_empty() { "another system".into() } else { other.join(" / ") }), out);
    }
}

pub fn find_junk(_db: &Db, fs: &dyn Transport, settings: &Settings) -> anyhow::Result<Vec<JunkItem>> {
    let mut out = Vec::new();
    for root in &settings.library_roots {
        let Ok(top) = fs.list(root) else { continue };
        for e in top.into_iter().filter(|e| e.is_dir) {
            let Mapping::System(sys) = map_folder(settings, &e.name) else { continue };
            let sys_dir = root.join(&e.name);
            let tools = sys_dir.join(crate::systems::TOOLS_DIR);
            // (dir, inside a tool folder?, a program sits in this tool folder or above)
            let mut stack = vec![(sys_dir.clone(), false, false)];
            while let Some((dir, in_tool, has_prog)) = stack.pop() {
                let Ok(entries) = fs.list(&dir) else { continue };
                let is_sys_root = dir == sys_dir;
                if entries.is_empty() && !is_sys_root {
                    out.push(JunkItem { path: dir.clone(), kind: JunkKind::EmptyDir, system_id: sys.id.into(), suggestion: "Empty folder — safe to delete.".into(), size: 0, members: vec![] });
                    continue;
                }
                let prog_here = entries.iter().any(|c| !c.is_dir && EXEC_EXTS.contains(&ext_of(&c.name).as_str()));
                if is_sys_root {
                    // scripts loose in the system folder move to _Tools with the lists beside them
                    let scripts: Vec<&crate::fsx::Entry> = entries.iter().filter(|c| !c.is_dir && EXEC_EXTS.contains(&ext_of(&c.name).as_str())).collect();
                    if !scripts.is_empty() && sys.id != "pc" {
                        let mut members: Vec<PathBuf> = entries.iter().filter(|c| !c.is_dir && (EXEC_EXTS.contains(&ext_of(&c.name).as_str()) || COMPANION_EXTS.contains(&ext_of(&c.name).as_str()))).map(|c| dir.join(&c.name)).collect();
                        members.sort();
                        let size = entries.iter().filter(|c| members.contains(&dir.join(&c.name))).map(|c| c.size).sum();
                        let names: Vec<String> = scripts.iter().map(|c| c.name.clone()).collect();
                        out.push(JunkItem { path: dir.join(&scripts[0].name), kind: JunkKind::ToolFiles, system_id: sys.id.into(),
                            suggestion: format!("{} with the lists and notes beside {} — move them together into {}/{}.", names.join(", "), if names.len() == 1 { "it" } else { "them" }, e.name, crate::systems::TOOLS_DIR), size, members });
                    }
                }
                for c in entries {
                    if c.is_dir {
                        if c.name == crate::archive::ARCHIVE_DIR || c.name == crate::shared::SHARED_DIR { continue; }
                        let child = dir.join(&c.name);
                        let ignored = is_ignored(settings, &c.name);
                        if is_sys_root && !ignored && sys.id != "pc" && is_program_folder(fs, sys, &child) {
                            // a program folder: it moves as a whole, its contents aren't listed one by one
                            out.push(JunkItem { path: child.clone(), kind: JunkKind::ToolFolder, system_id: sys.id.into(),
                                suggestion: format!("A program's folder — move the whole folder into {}/{} so its files stay together.", e.name, crate::systems::TOOLS_DIR), size: 0, members: vec![] });
                            continue;
                        }
                        if is_sys_root && ignored && child != tools {
                            out.push(JunkItem { path: child.clone(), kind: JunkKind::LooseToolFolder, system_id: sys.id.into(),
                                suggestion: format!("A tools folder outside {0}/{1} — move it there to keep every tool in one place (move any games out of it first).", e.name, crate::systems::TOOLS_DIR), size: 0, members: vec![] });
                        }
                        stack.push((child, in_tool || ignored, (in_tool || ignored) && (has_prog || prog_here)));
                    } else if is_sys_root && EXEC_EXTS.contains(&ext_of(&c.name).as_str()) {
                        // listed above with the files that travel with it
                        continue;
                    } else {
                        classify(sys, &dir, &c.name, c.size, in_tool, has_prog || prog_here, &mut out);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}
