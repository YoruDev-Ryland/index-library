//! Duplicate detection with a keep suggestion.
use crate::db::Db;
use crate::parse::{norm_title, parse};
use crate::systems::{by_id, GroupKind};
use crate::util::{archive_set_key, full_stem};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DupeReason { SameHash, EncDec, SameTitleDiffFormat, SameTitle, SameTitleDiffRegion, OlderUpdate }

/// How sure we are that an extra holds nothing the kept copies don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Proof {
    /// one of the files being kept
    Kept,
    /// byte-for-byte the same as a kept file (same fingerprint)
    Identical,
    /// the same content pieces as the kept files, packed differently (Switch NCAs are content hashes)
    SameContent,
    /// an older version of the same update or DLC: the kept newer version replaces it
    Superseded,
    /// a release from another region, dropped by one-copy-per-game (1G1R)
    OtherRegion,
    /// checked, and it holds data the kept files don't
    Different,
    /// couldn't be compared yet (not fingerprinted, or the archive is compressed)
    Unchecked,
}

impl Proof {
    /// nothing in it is missing from the kept files
    pub fn copy(self) -> bool { matches!(self, Proof::Identical | Proof::SameContent) }
    /// fine to archive without a second look: a copy, or an old version a kept one replaces
    pub fn safe(self) -> bool { self.copy() || self == Proof::Superseded }
}

#[derive(Debug, Clone, Serialize)]
pub struct DupeGroup {
    /// the group's exact files, for remembering that you're keeping them all
    pub skip_key: String,
    pub reason: DupeReason,
    pub system_id: String,
    pub title: String,
    pub files: Vec<i64>,
    /// full paths, parallel to `files`
    pub names: Vec<String>,
    /// sizes in bytes, parallel to `files`
    pub sizes: Vec<u64>,
    pub keep: i64,
    /// other files that belong with `keep` (the remaining parts of a multi-part archive)
    pub keep_also: Vec<i64>,
    /// bytes freed by archiving every file except `keep`
    pub reclaimable: u64,
    /// per file, parallel to `files`: kept, or how its content compares with what's kept
    pub proofs: Vec<Proof>,
    /// bytes of extras proven to be copies (safe to archive)
    pub proven_bytes: u64,
    /// for archives, what's inside (largest files first), parallel to `files`; "" otherwise
    pub inside: Vec<String>,
    /// archives fully unpacked and fingerprinted, parallel to `files`
    pub unpacked: Vec<bool>,
    /// files "Unpack and compare" could still open, parallel to `files`
    pub can_unpack: Vec<bool>,
    /// what each file is, parallel to `files` (Switch: "Base game", "Update 11", "DLC 5 · version 1")
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
struct F {
    id: i64, path: String, game: i64, dir: i64, system: String, ext: String, size: i64, sha1: Option<String>, crc: Option<String>, encrypted: Option<bool>, verified: bool, title: String, regions: Vec<String>, disc: Option<u32>, suffix: bool,
    /// Switch: content id and version (header first, then name), own title and leftover tags
    tid: Option<String>, version: Option<u64>, own_title: String, tags: Vec<String>,
    /// revision ("A", "2"); later is better
    rev: Option<String>,
    /// stray spaces in the name ("Croc .gbc")
    untidy: bool,
    /// Switch: content pieces (NCA ids) this file holds, when they could be read
    pieces: Vec<String>,
    /// archives: (size, crc) of each member, and the content ids inside
    members: Vec<(u64, Option<String>)>,
    ids: Vec<String>,
    archive: bool,
    /// fingerprints (SHA-1) of everything the file holds, when fully known: the file itself, or
    /// every file inside an archive that was unpacked and compared
    contents: Vec<String>,
    inside: String,
    unpacked: bool,
    /// its contents can be read by "Unpack and compare"
    opener: bool,
}

fn human(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (b as f64, 0);
    while v >= 1000.0 && i < 4 { v /= 1000.0; i += 1; }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

/// "EBOOT.PBP (370 MB), front.jpg (195 KB) and 2 more"
fn describe(members: &[serde_json::Value]) -> String {
    let mut m: Vec<(String, u64)> = members.iter().map(|x| (x["name"].as_str().unwrap_or("").rsplit(['/', '\\']).next().unwrap_or("").to_string(), x["size"].as_u64().unwrap_or(0))).filter(|(n, _)| !n.is_empty()).collect();
    if m.is_empty() { return String::new(); }
    m.sort_by(|a, b| b.1.cmp(&a.1));
    let shown: Vec<String> = m.iter().take(3).map(|(n, s)| format!("{n} ({})", human(*s))).collect();
    let more = m.len().saturating_sub(3);
    format!("{}{}", shown.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() })
}

fn load(db: &Db, system: Option<&str>) -> anyhow::Result<Vec<F>> {
    let mut q = db.conn().prepare(
        "SELECT f.id, f.game_id, f.dir_id, f.system_id, f.name, f.path, f.ext, f.size, f.sha1, json_extract(f.probe_json,'$.encrypted'), f.dat_status, g.title,
                json_extract(f.probe_json,'$.id'), json_extract(f.probe_json,'$.extra.version'), json_extract(f.probe_json,'$.extra.container'), f.crc32, f.probe_json
         FROM files f JOIN games g ON g.id=f.game_id WHERE (?1 IS NULL OR f.system_id=?1)")?;
    let rows = q.query_map([system], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, i64>(7)?, r.get::<_, Option<String>>(8)?, r.get::<_, Option<bool>>(9)?, r.get::<_, Option<String>>(10)?, r.get::<_, String>(11)?, r.get::<_, Option<String>>(12)?, r.get::<_, Option<String>>(13)?, r.get::<_, Option<String>>(14)?, r.get::<_, Option<String>>(15)?, r.get::<_, Option<String>>(16)?)))?;
    let mut out = Vec::new();
    for r in rows {
        let (id, game, dir, sys, name, path, mut ext, size, sha1, enc, dat, title, probe_id, probe_ver, container, crc, probe) = r?;
        let pj: serde_json::Value = probe.as_deref().and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default();
        let strs = |v: &serde_json::Value| -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default() };
        let ms = pj["extra"]["members"].as_array().cloned().unwrap_or_default();
        let mut pieces = strs(&pj["extra"]["ncas"]);
        // an archive's pieces are known only when every member's could be read
        if !ms.is_empty() && ms.iter().all(|m| m["ncas"].is_array()) { for m in &ms { pieces.extend(strs(&m["ncas"])); } }
        let members: Vec<(u64, Option<String>)> = ms.iter().map(|m| (m["size"].as_u64().unwrap_or(0), m["crc"].as_str().map(|s| s.to_string()))).collect();
        let ids = strs(&pj["extra"]["ids"]);
        let archive = matches!(ext.as_str(), "rar" | "zip" | "7z");
        let unpacked = pj["extra"]["unpacked"]["members"].as_array().cloned().unwrap_or_default();
        for m in &unpacked { pieces.extend(strs(&m["ncas"])); }
        let inside = if !unpacked.is_empty() { describe(&unpacked) } else { describe(&ms) };
        let is_unpacked = !unpacked.is_empty();
        // what it holds, when fully known; release notes aside. A PKG or archive that hasn't been
        // opened yet is unknown, not "just its own fingerprint".
        let opener = crate::deep::can_unpack(&sys, &ext);
        let contents: Vec<String> = if !unpacked.is_empty() {
            unpacked.iter().filter(|m| !crate::deep::is_note(m["name"].as_str().unwrap_or(""))).filter_map(|m| m["sha1"].as_str().map(|s| s.to_string())).collect()
        } else if opener { vec![] } else { sha1.iter().cloned().collect() };
        // the header knows the real format (a cart image named .nsp)
        if ext == "nsp" && container.as_deref() == Some("xci") { ext = "xci".into(); }
        let parsers = by_id(&sys).map(|s| s.parsers).unwrap_or(&[]);
        let p = parse(parsers, &full_stem(&name));
        let header = probe_id.filter(|t| t.len() == 16);
        let version = match &header {
            Some(h) => probe_ver.or_else(|| (p.title_id.as_deref() == Some(h.as_str())).then(|| p.version.clone()).flatten()),
            None => p.version.clone(),
        }.and_then(|v| v.parse().ok());
        let tid = header.or(p.title_id.clone());
        let untidy = name != name.trim() || full_stem(&name) != full_stem(&name).trim() || name.contains("  ");
        out.push(F { id, path, game, dir, system: sys, ext, size, sha1, crc, untidy, encrypted: enc, verified: dat.as_deref() == Some("verified"), title, regions: p.regions.clone(), disc: p.disc, suffix: p.flags.iter().any(|x| x == "decrypted-suffix"),
                     tid, version, own_title: p.title.clone(), tags: p.tags.clone(), rev: p.rev.clone(), pieces, members, ids, archive, contents, inside, unpacked: is_unpacked, opener });
    }
    Ok(out)
}

/// Lower is better.
/// Not a final retail release: prototypes, betas, demos, pirate and unlicensed copies, bad dumps.
fn unofficial(f: &F) -> bool {
    f.tags.iter().any(|t| {
        let t = t.to_lowercase();
        ["proto", "alpha", "beta", "demo", "sample", "preview", "pirate", "unl", "kiosk", "debug"].iter().any(|w| t.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w))
            || t == "b" || t.starts_with("b ") || t.starts_with("b1") || t.starts_with("b2") || t.contains("bad dump")
    })
}

/// A variant kept on purpose rather than a regional copy: BIOS files, hacks, translations.
fn variant(f: &F) -> bool {
    f.tags.iter().any(|t| {
        let t = t.to_lowercase();
        t == "bios" || t.starts_with("un-") || t.contains("license") || t.contains("english") || t.contains("undub") || t.contains("hack") || t.contains("patch") || t.contains("translat") || t.starts_with("t+") || t.starts_with("t-") || t == "h" || t.starts_with("h ")
    })
}

/// How ready a file is to play as it sits, lower is better: formats the emulator opens directly
/// (compressed ones first: they run and take the least space), then ones that need installing,
/// then archives that must be unpacked, and multi-part archives last.
fn readiness(f: &F) -> u8 {
    if archive_set_key(&f.path) != f.path { return 9; } // "x.part2.rar"
    let sys = f.system.as_str();
    // systems whose emulators can't open a zip/7z directly
    let disc = matches!(sys, "ps1" | "psp" | "psvita" | "switch" | "n3ds" | "gamecube" | "pc");
    match (sys, f.ext.as_str()) {
        (_, "rar") => 8,
        (_, "zip" | "7z") if disc => 8,
        ("ps1", "chd") => 0, ("ps1", "pbp") => 1, ("ps1", _) => 2,
        ("psp", "cso" | "chd") => 0, ("psp", "iso") => 1, ("psp", "pbp") => 2, ("psp", "pkg") => 5,
        ("gamecube", "rvz") => 0, ("gamecube", "ciso" | "gcz" | "wbfs") => 1, ("gamecube", _) => 2,
        ("n3ds", "cia") => 1, ("n3ds", _) => 0,
        ("switch", "nsz" | "xcz") => 1, ("switch", _) => 0,
        (_, "zip") => 0, (_, "7z") => 1,
        _ => 2,
    }
}

/// Picking between releases (1G1R): the release first, then the best copy of it.
fn release_rank(f: &F, region_order: &[String]) -> (bool, usize, std::cmp::Reverse<(usize, String)>, (u8, u8, u8, u8, bool, usize, std::cmp::Reverse<(usize, String)>, usize, bool, i64)) {
    let r = rank(f, region_order);
    (r.4, r.5, r.6.clone(), r)
}

fn rank(f: &F, region_order: &[String]) -> (u8, u8, u8, u8, bool, usize, std::cmp::Reverse<(usize, String)>, usize, bool, i64) {
    let exts = by_id(&f.system).map(|s| s.exts).unwrap_or(&[]);
    let region = f.regions.iter().filter_map(|r| region_order.iter().position(|o| o == r)).min().unwrap_or(region_order.len());
    (
        match f.encrypted { Some(false) => 0, None if f.suffix => 0, None => 1, Some(true) => 2 },
        // runs as it is, without unpacking or installing
        readiness(f),
        // equally decrypted: the plain name over the "-decrypted" one
        f.suffix as u8,
        if f.verified { 0 } else { 1 },
        // a final release over a prototype, beta or pirate copy
        unofficial(f),
        region,
        // the latest revision
        std::cmp::Reverse(f.rev.as_ref().map(|r| (r.len(), r.to_uppercase())).unwrap_or((0, String::new()))),
        exts.iter().position(|e| *e == f.ext).unwrap_or(exts.len()),
        // a tidy name over "Croc .gbc"
        f.untidy,
        -f.size,
    )
}

fn group(reason: DupeReason, files: &[&F], region_order: &[String]) -> DupeGroup {
    let keep = files.iter().min_by_key(|f| rank(f, region_order)).unwrap();
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    DupeGroup {
        skip_key: format!("{reason:?}\n{}", paths.join("\n")),
        reason, system_id: keep.system.clone(), title: keep.title.clone(), files: files.iter().map(|f| f.id).collect(), names: files.iter().map(|f| f.path.clone()).collect(), sizes: files.iter().map(|f| f.size.max(0) as u64).collect(), keep: keep.id, keep_also: Vec::new(),
        reclaimable: files.iter().filter(|f| f.id != keep.id).map(|f| f.size as u64).sum(),
        proofs: vec![], proven_bytes: 0, inside: files.iter().map(|f| f.inside.clone()).collect(), unpacked: files.iter().map(|f| f.unpacked).collect(), can_unpack: files.iter().map(|f| f.opener && !f.unpacked).collect(), notes: files.iter().map(|f| note_any(f)).collect(),
    }
}

/// How an extra compares with the files kept beside it. `pieces` gives each file's content pieces
/// (shared by all parts of a multi-part archive).
fn prove(f: &F, kept: &[&F], pieces: &dyn Fn(&F) -> BTreeSet<String>, contents: &dyn Fn(&F) -> BTreeSet<String>) -> Proof {
    let same_fp = |k: &&F| (k.sha1.is_some() && k.sha1 == f.sha1) || (k.crc.is_some() && k.crc == f.crc && k.size == f.size)
        || (f.members.len() == 1 && f.members[0].1.is_some() && k.crc == f.members[0].1 && k.size as u64 == f.members[0].0);
    if kept.iter().any(same_fp) { return Proof::Identical; }
    // every file it holds is, byte for byte, a kept file or a file inside a kept archive
    let mine_c = contents(f);
    let theirs_c: BTreeSet<String> = kept.iter().flat_map(|k| contents(k)).collect();
    if !mine_c.is_empty() && mine_c.is_subset(&theirs_c) { return Proof::Identical; }
    let mine = pieces(f);
    let theirs: BTreeSet<String> = kept.iter().flat_map(|k| pieces(k)).collect();
    if !mine.is_empty() && mine.is_subset(&theirs) { return Proof::SameContent; }
    if f.tid.is_some() && f.version.is_some() && kept.iter().any(|k| k.tid == f.tid && k.version.zip(f.version).map(|(a, b)| a > b).unwrap_or(false)) { return Proof::Superseded; }
    if !mine.is_empty() && !theirs.is_empty() { return Proof::Different; }
    // both sides fully fingerprinted (archives unpacked) and they don't match
    if !mine_c.is_empty() && kept.iter().all(|k| !contents(k).is_empty()) { return Proof::Different; }
    let fp = |x: &F| x.sha1.is_some() || x.crc.is_some();
    if fp(f) && !kept.is_empty() && kept.iter().all(|k| fp(k)) && !f.opener && kept.iter().all(|k| !k.opener) { return Proof::Different; }
    Proof::Unchecked
}

/// What a Switch file holds, from its content id and version.
fn note(f: &F) -> String {
    let Some(t) = f.tid.as_deref().and_then(|t| u64::from_str_radix(t, 16).ok()) else { return String::new() };
    let v = f.version.map(|v| format!(" · v{v}")).unwrap_or_default();
    match t & 0x1FFF {
        0 => format!("Base game{v}"),
        0x800 => format!("Update{v}"),
        n if n & 0x1000 != 0 => format!("DLC {}{v}", n & 0xFFF),
        _ => String::new(),
    }
}

/// The note for a file, or for an archive holding several pieces of content.
fn note_any(f: &F) -> String {
    if f.tid.is_none() && !f.ids.is_empty() {
        let inner: Vec<String> = f.ids.iter().map(|i| note(&F { tid: Some(i.clone()), version: None, ..f.clone() })).collect();
        return format!("Archive with {}", inner.join(", "));
    }
    note(f)
}

/// `one_per_title` (1G1R): of a title released in several regions, only the best release stays;
/// the others become extras. Off: every region is kept and no region groups are made.
pub fn find_dupes_with(db: &Db, system: Option<&str>, region_order: &[String], one_per_title: bool) -> anyhow::Result<Vec<DupeGroup>> {
    let files = load(db, system)?;
    let mut out = Vec::new();
    // identical content — only for one-file-per-game systems (cue/bin audio tracks are often
    // byte-identical across regions) and never for empty files
    let mut by_hash: HashMap<&str, Vec<&F>> = HashMap::new();
    for f in &files {
        let single = by_id(&f.system).map(|s| s.group == GroupKind::Single).unwrap_or(false);
        if let (Some(h), true, true) = (&f.sha1, single, f.size > 0) { by_hash.entry(h.as_str()).or_default().push(f); }
    }
    let mut hash_groups: Vec<&Vec<&F>> = by_hash.values().filter(|v| v.len() > 1).collect();
    hash_groups.sort_by_key(|v| v[0].id);
    for v in hash_groups { out.push(group(DupeReason::SameHash, v, region_order)); }
    // several files of one game in the same folder (one-file-per-game systems), split by disc.
    // Files in different folders (Games/ vs Updates/ vs DLC/) are different content; real
    // cross-folder copies are caught by SameHash once hashed.
    let mut by_game: BTreeMap<(i64, i64, Option<u32>), Vec<&F>> = BTreeMap::new();
    for f in &files {
        if by_id(&f.system).map(|s| s.group == GroupKind::Single).unwrap_or(false) { by_game.entry((f.game, f.dir, f.disc)).or_default().push(f); }
    }
    for v in by_game.values().filter(|v| v.len() > 1) {
        // the header decides; a "-decrypted" suffix only counts where the header couldn't be read
        let enc_mix = v.iter().any(|f| f.encrypted == Some(true)) && v.iter().any(|f| f.encrypted == Some(false) || (f.suffix && f.encrypted.is_none()))
            || v.iter().any(|f| f.suffix && f.encrypted.is_none()) && v.iter().any(|f| !f.suffix && f.encrypted.is_none());
        let reason = if enc_mix { DupeReason::EncDec } else if v.iter().any(|f| f.ext != v[0].ext) { DupeReason::SameTitleDiffFormat } else { DupeReason::SameTitle };
        out.push(group(reason, v, region_order));
    }
    switch_dupes(&files, region_order, &mut out);
    // same title in different regions (separate games): the best release keeps all of its files
    // (every disc, its updates and DLC); every file of the other releases is an extra
    if one_per_title {
        let extras_so_far: std::collections::HashSet<i64> = out.iter()
            .flat_map(|g| g.files.iter().copied().filter(move |f| *f != g.keep && !g.keep_also.contains(f))).collect();
        let mut by_title: BTreeMap<(String, String), Vec<&F>> = BTreeMap::new();
        for f in files.iter().filter(|f| !variant(f)) { by_title.entry((f.system.clone(), f.title.to_lowercase())).or_default().push(f); }
        let mut losing: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut region_groups = Vec::new();
        for v in by_title.values() {
            let mut games: Vec<i64> = v.iter().map(|f| f.game).collect();
            games.sort(); games.dedup();
            let mut regions: Vec<String> = v.iter().map(|f| f.regions.join(",")).collect();
            regions.sort(); regions.dedup();
            if games.len() < 2 || regions.len() < 2 { continue; }
            let candidates: Vec<&F> = v.iter().copied().filter(|f| !extras_so_far.contains(&f.id)).collect();
            let pool = if candidates.is_empty() { v.clone() } else { candidates };
            let best = pool.iter().min_by_key(|f| release_rank(f, region_order)).unwrap();
            let mut g = group(DupeReason::SameTitleDiffRegion, v, region_order);
            g.keep = best.id;
            g.keep_also = v.iter().filter(|f| f.game == best.game && f.id != best.id && !extras_so_far.contains(&f.id)).map(|f| f.id).collect();
            losing.extend(games.iter().copied().filter(|x| *x != best.game));
            region_groups.push(g);
        }
        // a release that goes to the archive whole needs no tidying of its own copies
        let game_of: HashMap<i64, i64> = files.iter().map(|f| (f.id, f.game)).collect();
        out.retain(|g| !g.files.iter().all(|f| game_of.get(f).map(|x| losing.contains(x)).unwrap_or(false)));
        out.extend(region_groups);
    }
    // A file kept in one group must never be an "extra" in another.
    let mut keepers: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for g in out.iter_mut() {
        if let Some(k) = g.files.iter().find(|f| keepers.contains(f)) { g.keep = *k; }
        keepers.insert(g.keep);
        keepers.extend(g.keep_also.iter().copied());
    }
    let by_id_: HashMap<i64, &F> = files.iter().map(|f| (f.id, f)).collect();
    let mut unit_pieces: HashMap<String, BTreeSet<String>> = HashMap::new();
    for f in &files { unit_pieces.entry(archive_set_key(&f.path)).or_default().extend(f.pieces.iter().cloned()); }
    let pieces = |f: &F| unit_pieces.get(&archive_set_key(&f.path)).cloned().unwrap_or_default();
    let mut unit_contents: HashMap<String, BTreeSet<String>> = HashMap::new();
    for f in &files { unit_contents.entry(archive_set_key(&f.path)).or_default().extend(f.contents.iter().cloned()); }
    let contents = |f: &F| unit_contents.get(&archive_set_key(&f.path)).cloned().unwrap_or_default();
    for g in out.iter_mut() {
        let sizes: HashMap<i64, i64> = files.iter().filter(|f| g.files.contains(&f.id)).map(|f| (f.id, f.size)).collect();
        g.reclaimable = g.files.iter().filter(|f| **f != g.keep && !keepers.contains(f)).map(|f| sizes.get(f).copied().unwrap_or(0) as u64).sum();
        let kept: Vec<&F> = g.files.iter().filter(|f| **f == g.keep || g.keep_also.contains(f) || keepers.contains(f)).filter_map(|f| by_id_.get(f).copied()).collect();
        g.proofs = g.files.iter().map(|id| {
            let f = by_id_[id];
            if kept.iter().any(|k| k.id == f.id) { return Proof::Kept; }
            match prove(f, &kept, &pieces, &contents) {
                p if p.copy() => p,
                _ if g.reason == DupeReason::SameTitleDiffRegion => Proof::OtherRegion,
                p => p,
            }
        }).collect();
        let first_safe: HashMap<String, Proof> = g.files.iter().zip(&g.proofs).filter(|(_, p)| p.safe()).map(|(f, p)| (archive_set_key(&by_id_[f].path), *p)).collect();
        // parts 2..n of a set have no header of their own: they take their set's proof
        for (i, id) in g.files.clone().iter().enumerate() {
            if g.proofs[i] == Proof::Unchecked { if let Some(p) = first_safe.get(&archive_set_key(&by_id_[id].path)) { g.proofs[i] = *p; } }
        }
        g.proven_bytes = g.files.iter().zip(&g.proofs).filter(|(f, p)| p.safe() && !keepers.contains(*f)).map(|(f, _)| sizes.get(f).copied().unwrap_or(0) as u64).sum();
    }
    Ok(out)
}

/// Switch: within a game, each piece of content (base, each update, each DLC) should exist once,
/// wherever it sits in the library. A file with no id counts as the base when it is named as the
/// game and nothing else. The parts of a multi-part archive are one copy.
fn switch_dupes(files: &[F], region_order: &[String], out: &mut Vec<DupeGroup>) {
    let sw: Vec<&F> = files.iter().filter(|f| by_id(&f.system).map(|s| s.group == GroupKind::SwitchTitle).unwrap_or(false)).collect();
    let mut base_of_game: HashMap<i64, String> = HashMap::new();
    for f in &sw {
        if let Some(t) = &f.tid { if u64::from_str_radix(t, 16).map(|v| v & 0x1FFF == 0).unwrap_or(false) { base_of_game.entry(f.game).or_insert_with(|| t.clone()); } }
    }
    let mut slots: BTreeMap<(i64, String), Vec<&F>> = BTreeMap::new();
    for f in &sw {
        let fname = f.path.rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
        let named_as_game = f.tags.is_empty() && f.version.is_none() && norm_title(&f.own_title) == norm_title(&f.title)
            && !fname.contains("update") && !fname.contains("dlc");
        let slot = match &f.tid {
            Some(t) => t.clone(),
            None if named_as_game => base_of_game.get(&f.game).cloned().unwrap_or_else(|| "base".into()),
            None => continue,
        };
        slots.entry((f.game, slot)).or_default().push(f);
    }
    for v in slots.values() {
        let mut units: BTreeMap<String, Vec<&F>> = BTreeMap::new();
        for f in v { units.entry(archive_set_key(&f.path)).or_default().push(f); }
        if units.len() < 2 { continue; }
        // newest version, then an unpacked file over an archive, then one already filed in a
        // "Title [id]" folder, then the system's format order (region tags on Switch names are
        // release-group labels, not content)
        fn sw_rank(f: &F) -> (u8, bool, usize, i64) {
            let exts = by_id(&f.system).map(|s| s.exts).unwrap_or(&[]);
            let parent = std::path::Path::new(&f.path).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let id_folder = crate::parse::parse(&[crate::systems::ParserKind::Switch], &parent).title_id.is_some();
            (readiness(f), !id_folder, exts.iter().position(|e| *e == f.ext).unwrap_or(exts.len()), -f.size)
        }
        fn best_of<'a>(u: &[&'a F]) -> &'a F { u.iter().min_by_key(|f| sw_rank(f)).unwrap() }
        let keep_unit = units.values().min_by_key(|u| (std::cmp::Reverse(u.iter().filter_map(|f| f.version).max()), sw_rank(best_of(u)))).unwrap();
        let keep = best_of(keep_unit);
        let mut versions: Vec<u64> = v.iter().filter_map(|f| f.version).collect();
        versions.sort(); versions.dedup();
        let reason = if versions.len() > 1 { DupeReason::OlderUpdate } else if v.iter().any(|f| f.ext != v[0].ext) { DupeReason::SameTitleDiffFormat } else { DupeReason::SameTitle };
        let mut g = group(reason, v, region_order);
        g.keep = keep.id;
        g.keep_also = keep_unit.iter().filter(|f| f.id != keep.id).map(|f| f.id).collect();
        out.push(g);
    }
    // an archive holding several pieces ("[2DLC].rar"): grouped with the files that hold the same
    // pieces, so it can be compared with them; it is a copy only if every piece is there
    for a in sw.iter().filter(|f| f.tid.is_none() && f.ids.len() > 1) {
        let mut cover: Vec<&F> = sw.iter().copied().filter(|f| f.id != a.id && f.game == a.game && f.ids.is_empty() && f.tid.as_ref().map(|t| a.ids.contains(t)).unwrap_or(false)).collect();
        if cover.is_empty() { continue; }
        cover.sort_by_key(|f| (f.archive, f.tid.clone(), std::cmp::Reverse(f.version)));
        // one file per piece: the newest unpacked one
        let mut seen = std::collections::HashSet::new();
        cover.retain(|f| seen.insert(f.tid.clone()));
        let mut all: Vec<&F> = vec![a];
        all.extend(cover.iter().copied());
        let mut g = group(DupeReason::SameTitle, &all, region_order);
        g.keep = cover[0].id;
        g.keep_also = cover[1..].iter().map(|f| f.id).collect();
        out.push(g);
    }
}

pub fn find_dupes(db: &Db, system: Option<&str>) -> anyhow::Result<Vec<DupeGroup>> {
    let s = crate::config::Settings::default();
    find_dupes_with(db, system, &s.region_order, s.one_game_one_rom)
}
