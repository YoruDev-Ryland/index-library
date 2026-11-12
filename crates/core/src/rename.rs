//! Rename planning: DAT names for verified files, templates otherwise. Always previewed.
use crate::db::Db;
use crate::fsx::Transport;
use crate::ops::Op;
use crate::parse::{norm_title, parse, ParsedName};
use crate::systems::by_id;
use crate::util::{full_ext, full_stem, p2s, stem_of};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail")]
pub enum Problem { Collision, TargetExists, IllegalOnWindows(String), Unchanged }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameItem {
    pub file_id: i64,
    pub game_id: i64,
    pub src: PathBuf,
    pub dst: PathBuf,
    pub source: String, // "dat" | "official" | "matched" | "header" | "template"
    pub problem: Option<Problem>,
    /// a name the game was matched to by similarity (SteamGridDB, IGDB…), offered for you to
    /// choose; exact matches (by id or fingerprint) are used without asking
    #[serde(default)]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub suggestion_source: Option<String>,
    /// whether the suggestion was checked to be for this console: "match", or None (not checked)
    #[serde(default)]
    pub suggestion_console: Option<String>,
    /// this plan uses the suggestion
    #[serde(default)]
    pub using_suggestion: bool,
    /// for grouping the preview: the game, its console, and what this file is to it
    #[serde(default)]
    pub game_title: String,
    #[serde(default)]
    pub system_id: String,
    /// "game" | "disc" | "update" | "dlc", with a label ("Base game", "DLC 3") and sort order
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub role_label: String,
    #[serde(default)]
    pub role_order: u32,
    /// this file and the name it would get: for keeping its current name
    #[serde(default)]
    pub skip_key: String,
}

const FIELDS: &[&str] = &["title", "subtitle", "regions", "langs", "rev", "version", "disc", "tags", "title_id", "serial"];

fn field(p: &ParsedName, name: &str) -> String {
    match name {
        "title" => p.title.clone(),
        "regions" => p.regions.join(", "),
        "langs" => p.langs.join(","),
        "rev" => p.rev.clone().unwrap_or_default(),
        "version" => p.version.clone().unwrap_or_default(),
        "disc" => p.disc.map(|d| d.to_string()).unwrap_or_default(),
        "tags" => p.tags.iter().filter(|t| *t != "verified-good").cloned().collect::<Vec<_>>().join(") ("),
        "subtitle" => p.tags.iter().filter(|t| *t != "verified-good").cloned().collect::<Vec<_>>().join(" - "),
        "title_id" => p.title_id.clone().unwrap_or_default(),
        "serial" => p.serial.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// `{field}` → value. `{ ... }` containing `$field` (or a bare lowercase field word) is an
/// optional group, dropped entirely if any field it references is empty.
pub fn render_template(tpl: &str, p: &ParsedName) -> String {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('}') else { out.push_str(&rest[start..]); rest = ""; break };
        let g = &rest[start + 1..start + len];
        rest = &rest[start + len + 1..];
        if FIELDS.contains(&g) { out.push_str(&field(p, g)); continue; }
        let dollar = g.contains('$');
        let mut rendered = String::new();
        let mut empty = false;
        let mut referenced = false;
        let chars: Vec<char> = g.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let is_dollar = chars[i] == '$';
            let s = if is_dollar { i + 1 } else { i };
            let mut e = s;
            while e < chars.len() && (chars[e].is_ascii_lowercase() || chars[e] == '_') { e += 1; }
            let word: String = chars[s..e].iter().collect();
            let word_start = s == 0 || !chars[s - 1].is_alphanumeric() || is_dollar;
            if e > s && FIELDS.contains(&word.as_str()) && (is_dollar || (!dollar && word_start)) {
                let v = field(p, &word);
                referenced = true;
                if v.is_empty() { empty = true; }
                rendered.push_str(&v);
                i = e;
            } else {
                rendered.push(chars[i]);
                i += 1;
            }
        }
        if !(referenced && empty) { out.push_str(&rendered); }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Why a file name would be invalid on Windows (or SMB shares), if it would be.
pub fn windows_problem(name: &str) -> Option<String> {
    if let Some(c) = name.chars().find(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (*c as u32) < 32) {
        return Some(format!("contains '{c}'"));
    }
    if name.ends_with('.') || name.ends_with(' ') { return Some("ends with a dot or space".into()); }
    let base = name.split('.').next().unwrap_or("").trim_end().to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || ((base.starts_with("COM") || base.starts_with("LPT")) && base.len() == 4 && base.as_bytes()[3].is_ascii_digit());
    if reserved { return Some(format!("'{base}' is a reserved name")); }
    if name.len() > 255 { return Some("name longer than 255 bytes".into()); }
    None
}

/// Make an arbitrary title safe as a file name on Windows/SMB (":" → " -", other illegal chars dropped).
pub fn sanitize_filename(s: &str) -> String {
    let s = s.replace(": ", " - ").replace(':', " -");
    let s: String = s.chars().filter(|c| !matches!(c, '<' | '>' | '"' | '/' | '\\' | '|' | '?' | '*') && (*c as u32) >= 32).collect();
    s.trim().trim_end_matches('.').trim().to_string()
}

struct FileRow {
    id: i64, game_id: i64, path: String, name: String, system: String, dat_status: Option<String>, dat_game: Option<String>, dat_rom: Option<String>,
    game_title: String, probe_id: Option<String>, probe_version: Option<String>, container: Option<String>,
    meta_source: Option<String>, meta_title: Option<String>, meta_score: Option<f64>, meta_console: Option<String>,
}

/// A matched title made fit for a file name: no trademark signs, ":" as " -".
fn file_title(t: &str) -> String {
    let t: String = t.chars().filter(|c| !matches!(c, '™' | '®' | '©')).collect();
    let t = t.replace('꞉', " -");
    sanitize_filename(&RE_SP.replace_all(t.trim(), " "))
}
static RE_SP: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"\s+").unwrap());

/// (official title, suggested title + its source). Official: matched exactly — by title id in a
/// title database, or by fingerprint on ScreenScraper. Suggested: matched by name similarity.
fn matched_titles(f: &FileRow) -> (Option<String>, Option<(String, String)>) {
    let (Some(src), Some(title)) = (f.meta_source.as_deref(), f.meta_title.as_deref().filter(|t| !t.trim().is_empty())) else { return (None, None) };
    let exact = src.starts_with("titledb-") || (src == "screenscraper" && f.meta_score.unwrap_or(0.0) >= 0.999);
    if exact { return (Some(file_title(title)), None); }
    // a match for another console is never offered as a name
    if f.meta_console.as_deref() == Some("other") { return (None, None); }
    let label = match src { "steamgriddb" => "SteamGridDB", "igdb" => "IGDB", "rawg" => "RAWG", "screenscraper" => "ScreenScraper", other => other };
    (None, Some((file_title(title), label.to_string())))
}

/// Switch: the game's best-known title, plus the content's own id/version — read from the file
/// header when possible, else its name. Files with no id are only tidied when that loses nothing
/// ("i-am-setsuna" → the game's title); anything else is left as it is.
fn switch_name(sys: &crate::systems::SystemDef, f: &FileRow, title_override: Option<&str>) -> (String, String, &'static str) {
    let stem = full_stem(&f.name);
    let mut ext = full_ext(&f.name);
    // the header knows the real format: a cart image named .nsp is an .xci, and vice versa
    match (ext.to_lowercase().as_str(), f.container.as_deref()) {
        ("nsp", Some("xci")) => ext = "xci".into(),
        ("xci", Some("nsp")) => ext = "nsp".into(),
        _ => {}
    }
    let p = parse(sys.parsers, &stem);
    let header = f.probe_id.clone().filter(|t| t.len() == 16);
    let (tid, version, source) = match header {
        Some(h) => {
            let v = f.probe_version.clone().or_else(|| (p.title_id.as_deref() == Some(h.as_str())).then(|| p.version.clone()).flatten());
            (Some(h), v, "header")
        }
        None => (p.title_id.clone(), p.version.clone(), "template"),
    };
    let title = match title_override { Some(t) => t.to_string(), None if f.game_title.trim().is_empty() => p.title.clone(), None => f.game_title.clone() };
    let new_stem = if tid.is_some() {
        let q = ParsedName { title, title_id: tid, version, tags: p.tags.clone(), ..Default::default() };
        render_template(sys.template, &q)
    } else if p.tags.is_empty() && p.version.is_none() && norm_title(&p.title) == norm_title(&title) {
        title
    } else {
        stem
    };
    (new_stem, ext, source)
}

pub fn plan_renames(db: &Db, fs: &dyn Transport, game_ids: &[i64]) -> anyhow::Result<Vec<RenameItem>> {
    plan_renames_with(db, fs, game_ids, &[])
}

/// `use_matched`: games whose name-similarity match you chose to name them by.
pub fn plan_renames_with(db: &Db, fs: &dyn Transport, game_ids: &[i64], use_matched: &[i64]) -> anyhow::Result<Vec<RenameItem>> {
    let mut items = Vec::new();
    let mut q = db.conn().prepare(
        "SELECT f.id, f.game_id, f.path, f.name, f.system_id, f.dat_status, f.dat_game, f.dat_rom, g.title,
                json_extract(f.probe_json,'$.id'), json_extract(f.probe_json,'$.extra.version'), json_extract(f.probe_json,'$.extra.container'),
                CASE WHEN g.meta_state='matched' THEN json_extract(g.meta_json,'$.source') END, json_extract(g.meta_json,'$.title'), json_extract(g.meta_json,'$.score'), json_extract(g.meta_json,'$.console')
         FROM files f JOIN games g ON g.id=f.game_id WHERE f.game_id=?1 ORDER BY f.path")?;
    for gid in game_ids {
        let rows: Vec<FileRow> = q.query_map([gid], |r| Ok(FileRow {
            id: r.get(0)?, game_id: r.get(1)?, path: r.get(2)?, name: r.get(3)?, system: r.get(4)?, dat_status: r.get(5)?, dat_game: r.get(6)?, dat_rom: r.get(7)?,
            game_title: r.get(8)?, probe_id: r.get(9)?, probe_version: r.get(10)?, container: r.get(11)?,
            meta_source: r.get(12)?, meta_title: r.get(13)?, meta_score: r.get::<_, Option<f64>>(14).ok().flatten(), meta_console: r.get(15)?,
        }))?.collect::<Result<_, _>>()?;
        for f in rows {
            let Some(sys) = by_id(&f.system) else { continue };
            let is_container = matches!(crate::util::ext_of(&f.name).as_str(), "zip" | "7z" | "rar");
            let verified = f.dat_status.as_deref() == Some("verified");
            let (official, suggested) = matched_titles(&f);
            let use_suggested = suggested.is_some() && use_matched.contains(&f.game_id);
            let title_override: Option<String> = official.clone().or_else(|| use_suggested.then(|| suggested.as_ref().unwrap().0.clone()));
            let named = if official.is_some() { "official" } else if use_suggested { "matched" } else { "template" };
            let (stem, ext, source) = match (&f.dat_game, &f.dat_rom) {
                // a raw file takes its ROM's own name (tracks in a set differ); a container takes the game name
                (_, Some(r)) if verified && !is_container => (stem_of(r), full_ext(&f.name), "dat"),
                (Some(g), _) if verified => (g.clone(), full_ext(&f.name), "dat"),
                _ if sys.group == crate::systems::GroupKind::SwitchTitle => {
                    let (st, ex, src) = switch_name(sys, &f, title_override.as_deref());
                    (st, ex, if title_override.is_some() { named } else { src })
                }
                _ => {
                    let mut p = parse(sys.parsers, &full_stem(&f.name));
                    if let Some(t) = &title_override { p.title = t.clone(); p.display_title = None; }
                    (render_template(sys.template, &p), full_ext(&f.name), named)
                }
            };
            // offer the similarity match only where it would change the name and DAT names don't apply
            let current_title = parse(sys.parsers, &full_stem(&f.name)).title;
            let suggestion = suggested.filter(|(t, _)| !verified && crate::parse::norm_title(t) != crate::parse::norm_title(&current_title));
            let src = PathBuf::from(&f.path);
            let new_name = if stem.trim().is_empty() { f.name.clone() } else if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
            let dst = src.with_file_name(&new_name);
            let problem = if dst == src { Some(Problem::Unchanged) } else { windows_problem(&new_name).map(Problem::IllegalOnWindows) };
            let (role, role_label, role_order) = crate::util::file_role(&f.system, &f.name, f.probe_id.as_deref());
            items.push(RenameItem { file_id: f.id, game_id: f.game_id, src, dst, source: source.to_string(), problem,
                skip_key: format!("{}\n{}", f.path, new_name), game_title: f.game_title.clone(), system_id: f.system.clone(), role: role.into(), role_label, role_order,
                using_suggestion: use_suggested && suggestion.is_some(),
                suggestion_console: suggestion.as_ref().and(f.meta_console.clone()),
                suggestion_source: suggestion.as_ref().map(|s| s.1.clone()), suggestion: suggestion.map(|s| s.0) });
        }
    }
    // Collisions within the plan (case-insensitive: Windows and most SMB shares are).
    let mut seen: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() { seen.entry(p2s(&it.dst).to_lowercase()).or_default().push(i); }
    for idxs in seen.values().filter(|v| v.len() > 1) {
        for &i in idxs { if items[i].problem.is_none() { items[i].problem = Some(Problem::Collision); } }
    }
    for it in items.iter_mut().filter(|i| i.problem.is_none()) {
        let same_file = p2s(&it.src).to_lowercase() == p2s(&it.dst).to_lowercase();
        if !same_file && fs.exists(&it.dst) { it.problem = Some(Problem::TargetExists); }
    }
    Ok(items)
}

/// Turn approved plan items into ops: renames first, then .cue rewrites for renamed tracks.
pub fn to_ops(db: &Db, fs: &dyn Transport, items: &[RenameItem]) -> anyhow::Result<Vec<Op>> {
    let ok: Vec<&RenameItem> = items.iter().filter(|i| i.problem.is_none()).collect();
    let mut ops = Vec::new();
    let mut renamed_in_dir: BTreeMap<PathBuf, Vec<(String, String)>> = BTreeMap::new();
    let mut final_path: HashMap<PathBuf, PathBuf> = HashMap::new();
    for it in &ok {
        let (size, mtime): (i64, i64) = db.conn().query_row("SELECT size, mtime FROM files WHERE id=?1", [it.file_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        ops.push(Op::rename(it.src.clone(), it.dst.clone()).expecting(size, mtime));
        let dir = it.src.parent().unwrap_or(Path::new("")).to_path_buf();
        let n = |p: &Path| p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
        renamed_in_dir.entry(dir).or_default().push((n(&it.src), n(&it.dst)));
        final_path.insert(it.src.clone(), it.dst.clone());
    }
    for (dir, pairs) in &renamed_in_dir {
        // .cue (FILE "x.bin") and .m3u (one disc per line) files that may reference renamed files.
        // The rewrite is decided when it runs, so only renames that actually happened are applied.
        let lists: Vec<String> = {
            let mut q = db.conn().prepare("SELECT f.path FROM files f JOIN dirs d ON d.id=f.dir_id WHERE d.path=?1 AND f.ext IN ('cue','m3u')")?;
            let v = q.query_map([p2s(dir)], |r| r.get(0))?.collect::<Result<_, _>>()?;
            v
        };
        for list in lists {
            let list = PathBuf::from(list);
            let Ok(meta) = fs.stat(&list) else { continue };
            let Ok(bytes) = fs.read_range(&list, 0, meta.size as usize) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            if crate::ops::rewrite_refs_text(&text, pairs) == text { continue; }
            let target = final_path.get(&list).cloned().unwrap_or(list.clone());
            ops.push(Op::rewrite_refs(target, list, pairs.clone()));
        }
    }
    Ok(ops)
}
