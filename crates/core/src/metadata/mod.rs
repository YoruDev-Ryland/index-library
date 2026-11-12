//! Game info and artwork: offline title DBs → ScreenScraper (hash) → IGDB / SteamGridDB (title).
pub mod boxart;
pub mod catalog;
pub mod http;
pub mod igdb;
pub mod rawg;
pub mod consoles;
pub mod offline;
pub mod screenscraper;
pub mod sgdb;
pub mod system_art;

use crate::config::Settings;
use crate::db::Db;
use crate::parse::ParsedName;
use crate::systems::{by_id, SystemDef};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind { Cover, Hero, Logo, Icon, Screenshot, Banner }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtRef { pub kind: ArtKind, pub url: String }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetaCandidate {
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub score: f32,
    pub description: Option<String>,
    pub release: Option<String>,
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub art: Vec<ArtRef>,
    /// the consoles the source says this game was released on (empty: the source doesn't say)
    #[serde(default)]
    pub platforms: Vec<String>,
    /// "match": released on this game's console; "other": only on other consoles; None: not known
    #[serde(default)]
    pub console: Option<String>,
    /// you picked this match yourself (in Match review): automatic lookups never replace it
    #[serde(default)]
    pub chosen: bool,
    /// the other names the source knows the game by (IGDB's alternative names: regional and
    /// original titles, "Sagaia" for Darius II), compared like its name
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

impl MetaCandidate {
    /// How alike this listing's name, or one of its other names, is to a name.
    pub fn likeness(&self, name: &str) -> f32 {
        std::iter::once(&self.title).chain(&self.aliases).map(|t| similarity(name, t)).fold(0.0, f32::max)
    }
}

#[derive(Clone)]
pub struct GameQuery<'a> {
    pub system: &'a SystemDef,
    pub title: String,
    /// the exact release the file was identified as by its fingerprint, without region tags
    /// ("Clay Fighter 2 - Judgment Clay" for "C2 - Judgment Clay"), when it reads differently
    pub release: Option<String>,
    pub sha1: Option<String>,
    pub crc32: Option<String>,
    pub size: Option<i64>,
    pub file_name: Option<String>,
    pub probe_id: Option<String>,
    pub title_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "candidates")]
pub enum MatchResult { Auto(MetaCandidate), NeedsReview(Vec<MetaCandidate>), NoMatch }

pub const AUTO_SCORE: f32 = 0.9;
/// Raised when the way candidates are judged changes, so games already waiting in Match review
/// are judged again once (2: subtitles, "&"/"and", numerals, Mark III, same-title listings;
/// 3: compilations compared game by game; 4: packs searched without the pack's wording; 5: "2 Great Games!"; 6: "&" joins the games in a pack;
/// 7: a repeated year, the same words in another order; 8: names compared by section, and
/// searched without a label in front; 9: I for 1 in a code, searched without the last word;
/// 10: an acronym label in front; 13: matched against the console's whole catalog; 14: games not found before are looked up
/// again even when none wait in review; 11: searched and compared by the release a fingerprint identified, or an arcade set's title;
/// 12: compared with IGDB's other names too, and searched on until a name is confirmed for the console)
pub const MATCHER_VERSION: u32 = 14;

/// Is any online source configured?
/// Whether each source is in use, and if not, what it needs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceStatus { pub id: &'static str, pub name: &'static str, pub active: bool, pub detail: String }

pub fn source_status(db: &Db, s: &Settings) -> Vec<SourceStatus> {
    let k = &s.keys;
    let titles = |src: &str| db.conn().query_row("SELECT COUNT(*) FROM titledb WHERE source=?1", [src], |r| r.get::<_, i64>(0)).unwrap_or(0);
    let (sw, psp) = (titles("switch"), titles("psp"));
    let ss_user = !k.screenscraper_user.trim().is_empty();
    vec![
        SourceStatus { id: "titledb-switch", name: "Switch title database", active: sw > 0,
            detail: if sw > 0 { format!("{sw} titles — exact matches by title ID, with icons and banners") } else { "Not downloaded — exact matches by title ID, including new releases".into() } },
        SourceStatus { id: "titledb-psp", name: "PSP title list", active: psp > 0,
            detail: if psp > 0 { format!("{psp} titles from PSP_GAMES.tsv — exact matches by content ID") } else { "Not found — put NoPayStation's PSP_GAMES.tsv in the PSP folder or its _Tools folder".into() } },
        SourceStatus { id: "steamgriddb", name: "SteamGridDB", active: sgdb::configured(s),
            detail: if sgdb::configured(s) { "Covers and artwork, by name".into() } else { "Not set up — add an API key".into() } },
        SourceStatus { id: "screenscraper", name: "ScreenScraper", active: screenscraper::configured(s),
            detail: if screenscraper::configured(s) { "Exact matches by file fingerprint, and name search".into() }
                else if ss_user { "Not in use: your account is filled in, but ScreenScraper also requires a developer ID for each app (Cocoon and other apps ship their own; this one has none yet). Request a free one on the ScreenScraper forum and enter it here.".into() }
                else { "Not set up".into() } },
        SourceStatus { id: "rawg", name: "RAWG", active: rawg::configured(s),
            detail: if rawg::configured(s) { "Names, dates, genres, wide artwork, and the consoles each game came out on".into() } else { "Not set up — a free key from rawg.io/apidocs; checks matches against consoles".into() } },
        SourceStatus { id: "igdb", name: "IGDB", active: igdb::configured(s),
            detail: if igdb::configured(s) { "Descriptions, dates and genres, by name".into() } else { "Not set up — add a Twitch client ID and secret".into() } },
    ]
}

pub fn any_source(s: &Settings) -> bool { sgdb::configured(s) || igdb::configured(s) || screenscraper::configured(s) || rawg::configured(s) }

/// Can matches be checked against the consoles a game came out on? (IGDB or RAWG)
pub fn can_check_consoles(s: &Settings) -> bool { igdb::configured(s) || rawg::configured(s) }

/// The consoles a title was released on, from IGDB (else RAWG): ("match" | "other", consoles).
/// None when neither knows the title or neither is set up.
pub fn check_console(db: &Db, http: &http::Http, s: &Settings, system: &SystemDef, title: &str) -> Option<(String, Vec<String>)> {
    let mut found: Vec<MetaCandidate> = Vec::new();
    if igdb::configured(s) { if let Ok(v) = igdb::search_any(db, http, s, system, title) { found.extend(v); } }
    if !found.iter().any(|c| c.score >= 0.9) && rawg::configured(s) { if let Ok(v) = rawg::search_any(db, http, s, system, title) { found.extend(v); } }
    let close: Vec<&MetaCandidate> = found.iter().filter(|c| c.score >= 0.9 && !c.platforms.is_empty()).collect();
    if close.is_empty() { return None; }
    if let Some(m) = close.iter().find(|c| c.console.as_deref() == Some("match")) { return Some(("match".into(), m.platforms.clone())); }
    let mut p: Vec<String> = close.iter().flat_map(|c| c.platforms.clone()).collect();
    p.sort(); p.dedup();
    Some(("other".into(), p))
}

/// Fill in the console verdict of matches whose source doesn't list consoles (SteamGridDB).
fn check_consoles_of(db: &Db, http: &http::Http, s: &Settings, system: &SystemDef, cands: &mut [MetaCandidate], limit: usize) {
    if !can_check_consoles(s) { return; }
    for c in cands.iter_mut().filter(|c| c.console.is_none()).take(limit) {
        if let Some((v, p)) = check_console(db, http, s, system, &c.title) { c.console = Some(v); c.platforms = p; }
    }
}

/// Lowercase words of a title for comparing: accents and punctuation dropped, "&" and "+" read
/// as "and", Roman numerals II–IX as digits, and a leading or trailing "The" set aside.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' => out.push('i'),
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ő' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => out.push('u'),
            'ñ' => out.push('n'), 'ç' => out.push('c'), 'ý' | 'ÿ' => out.push('y'), 'ß' => out.push_str("ss"),
            '½' => out.push_str(" 1 2 "),
            '&' | '+' => out.push_str(" and "),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    let roman = |w: &str| match w { "ii" => "2", "iii" => "3", "iv" => "4", "v" => "5", "vi" => "6", "vii" => "7", "viii" => "8", "ix" => "9", w => w }.to_string();
    let mut w: Vec<String> = out.split_whitespace().map(roman).collect();
    // "Addams Family, The" and "The Addams Family" compare equal
    if w.last().map(String::as_str) == Some("the") { w.pop(); w.insert(0, "the".into()); }
    if w.first().map(String::as_str) == Some("the") { w.remove(0); }
    w.join(" ")
}

/// A title's letters and digits as they look: 1 as I, 0 as O.
fn lookalike(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).map(|c| match c { '1' => 'i', '0' => 'o', c => c }).collect()
}

/// The numbers in a normalized title: a sequel or a year is a different game.
fn numbers(n: &str) -> Vec<&str> {
    // any word with a digit: "2K7" and "2K8" are different years of a series
    let mut v: Vec<&str> = n.split_whitespace().filter(|w| w.chars().any(|c| c.is_ascii_digit())).collect();
    v.sort_unstable();
    v.dedup(); // "2006 FIFA World Cup - Germany 2006" has one year, said twice
    v
}

/// A title's sections, split at its subtitle marks ("2K Sports - Top Spin 2", "Pokémon: Red").
fn sections(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((i, len)) = [" - ", ":", " ~ "].iter().filter_map(|m| rest.find(m).map(|i| (i, m.len()))).min() {
        out.push(normalize(&rest[..i]));
        rest = &rest[i + len..];
    }
    out.push(normalize(rest));
    out.retain(|x| !x.is_empty());
    out
}

/// The shorter names a title also goes by: any run of its sections, without a label in front
/// ("2K Sports -") or a subtitle after ("- Learn a New Language"). Never without a section after
/// that counts something: "Tinker Bell: 2 Disney Games" is not "Tinker Bell".
fn shorter(s: &str) -> Vec<String> {
    let secs = sections(s);
    let n = secs.len();
    if !(2..=6).contains(&n) { return Vec::new(); }
    let mut out = Vec::new();
    for i in 0..n {
        for j in i..n {
            // a bare number after ("2 Disney Games") makes it another release; "Back to the 90s" doesn't
            let counted = |x: &String| x.split(' ').any(|w| w.chars().all(|c| c.is_ascii_digit()));
            if (i, j) == (0, n - 1) || secs[j + 1..].iter().any(counted) { continue; }
            let v = secs[i..=j].join(" ");
            if v.replace(' ', "").len() >= 3 && v.split(' ').any(|w| !w.chars().all(|c| c.is_ascii_digit())) { out.push(v); }
        }
    }
    out
}

/// The same name, give or take spacing or word order ("Megaman" / "Mega Man", "FIFA World Cup
/// Germany 2006" / "2006 FIFA World Cup Germany").
fn same_name(x: &str, y: &str) -> bool {
    if x == y || x.replace(' ', "") == y.replace(' ', "") { return true; }
    let (sx, sy): (std::collections::BTreeSet<&str>, std::collections::BTreeSet<&str>) = (x.split(' ').collect(), y.split(' ').collect());
    sx == sy && sx.len() >= 3
}

/// A normalized title without a one-word possessive in front ("Disney's Mulan" → "mulan").
fn unowned(n: &str) -> Option<&str> {
    let mut it = n.splitn(3, ' ');
    match (it.next(), it.next(), it.next()) { (Some(_), Some("s"), Some(rest)) => Some(rest), _ => None }
}

/// How alike two game titles are, 0 to 1.
/// 1.0 the same words · 0.97 the same but for spacing ("Mega Man" / "Megaman") ·
/// 0.95 the same but for a possessive in front ("Disney's Mulan") ·
/// 0.93 the same words in another order ·
/// 0.92 one is the other's title or subtitle on its own ("Golvellius" / "Golvellius: Valley of Doom") ·
/// otherwise by edit distance, kept below a match when the numbers differ ("Sonic" / "Sonic 3").
/// Compilations ("2 Games in 1 - Scooby-Doo + Scooby-Doo 2") compare game by game; a single game
/// is only part of one, so it never scores as a match for it, nor it for a single game.
/// Anything under 0.97 counts as a match only when the source confirms the console (see `decide`).
pub fn similarity(a: &str, b: &str) -> f32 {
    match (games_in(a), games_in(b)) {
        (Some(pa), Some(pb)) => {
            // each game in one, against its closest in the other; a missing game counts as zero
            let sum: f32 = pa.iter().map(|x| pb.iter().map(|y| alike(x, y)).fold(0.0, f32::max)).sum();
            sum / pa.len().max(pb.len()) as f32
        }
        (Some(_), None) | (None, Some(_)) => {
            // the same name once a pack's wording is set aside ("2 Games in 1 - Dragon Ball Z - The
            // Legacy of Goku I & II"), else one game is only part of a pack
            let u = alike(unpack(a), unpack(b));
            if u >= 0.97 { u } else { alike(a, b).min(0.6) }
        }
        // the same game once a pack's wording is set aside ("2 Games in 1 - Dragon Ball Z - The
        // Legacy of Goku I & II" / "Dragon Ball Z: The Legacy of Goku I & II"); only near-exact,
        // so a pack never matches one of its games by a part of its name
        (None, None) => {
            let d = alike(a, b);
            let u = alike(unpack(a), unpack(b));
            if u >= 0.97 { d.max(u) } else { d }
        }
    }
}

/// A title without a pack's wording in front ("2 Games in 1 -", "2-in-1 Fun Pack:", "3 Game Pack! -",
/// "2-Pack -", "2 Great Games! -"). Searching with that wording finds other packs instead of this one.
pub fn unpack(s: &str) -> &str {
    let Some((at, len)) = [" - ", ":"].iter().filter_map(|m| s.find(m).map(|i| (i, m.len()))).min() else { return s };
    let head = normalize(&s[..at]);
    let words: Vec<&str> = head.split(' ').collect();
    let packish = |w: &str| w.chars().all(|c| c.is_ascii_digit())
        || matches!(w, "games" | "game" | "in" | "one" | "two" | "three" | "four" | "pack" | "fun" | "double" | "combo" | "value" | "great");
    let counted = words.iter().any(|w| w.chars().all(|c| c.is_ascii_digit()) || matches!(*w, "one" | "two" | "three" | "four"));
    let rest = s[at + len..].trim_start();
    // "Pack", "in 1", or a count of games ("2 Great Games!")
    let pack = words.iter().any(|w| matches!(*w, "pack" | "in")) || (counted && words.contains(&"games"));
    if !head.is_empty() && words.iter().all(|w| packish(w)) && pack && !rest.is_empty() { rest } else { s }
}

/// The games in a compilation title, joined by " + " or " / ", without the pack's own wording in
/// front ("2 Games in 1 Double Pack:", "2-in-1 Fun Pack -", "Crash & Spyro Superpack -"). None for
/// a single game.
fn games_in(s: &str) -> Option<Vec<String>> {
    let u = unpack(s);
    let parts: Vec<&str> = u.split(" + ").flat_map(|p| p.split(" / ")).map(str::trim).filter(|p| !p.is_empty()).collect();
    let mut out: Vec<String> = parts.iter().map(|p| normalize(p)).collect();
    let words: Vec<&str> = out.first()?.split(' ').collect();
    let cut = (0..words.len().saturating_sub(1)).rev().find(|&i| matches!(words[i], "pack" | "superpack" | "pak")
        || (i > 0 && words[i - 1] == "in" && matches!(words[i], "1" | "one"))
        // "2 Great Games! Ms. Pac-Man…": a count of games, within the first few words
        || (words[i] == "games" && i <= 3 && words[..i].iter().any(|w| w.chars().all(|c| c.is_ascii_digit()) || matches!(*w, "two" | "three" | "four"))));
    if let Some(i) = cut { out[0] = words[i + 1..].join(" "); }
    // in a title that is plainly a pack, "&" / "and" joins its games too ("2 Great Games! Ms. Pac-Man
    // Maze Madness & Pac-Man World"); elsewhere it's part of a name ("Mario & Luigi")
    if out.len() == 1 && (u.len() != s.len() || cut.is_some()) {
        out = out[0].split(" and ").map(str::to_string).filter(|p| !p.is_empty()).collect();
    }
    (out.len() >= 2).then_some(out)
}

fn alike(a: &str, b: &str) -> f32 {
    let (na, nb) = (normalize(a), normalize(b));
    if na == nb { return 1.0; }
    if na.replace(' ', "") == nb.replace(' ', "") { return 0.97; }
    let (ua, ub) = (unowned(&na), unowned(&nb));
    if ua == Some(nb.as_str()) || ub == Some(na.as_str()) || (ua.is_some() && ua == ub) { return 0.95; }
    // a code with I for 1 or O for 0 ("Super Huey UH-IX" / "UH-1X"): the same name, letter for letter
    if lookalike(a) == lookalike(b) && [a, b].iter().any(|t| t.split(|c: char| !c.is_alphanumeric()).any(|w| w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_alphabetic()))) {
        return 0.95;
    }
    // an acronym label in front with no mark after it ("AWS Pro Moves Soccer" / "Pro Moves Soccer")
    let unlabeled = |t: &str| -> Option<String> {
        let (first, rest) = t.trim().split_once(' ')?;
        let rest = normalize(rest);
        ((2..=5).contains(&first.len()) && first.chars().all(|c| c.is_ascii_uppercase()) && rest.split(' ').count() >= 2).then_some(rest)
    };
    if unlabeled(a).is_some_and(|r| same_name(&r, &nb)) || unlabeled(b).is_some_and(|r| same_name(&r, &na)) { return 0.92; }
    let same_numbers = numbers(&na) == numbers(&nb);
    // the same words in another order
    if same_numbers && same_name(&na, &nb) { return 0.93; }
    // one name is a whole section (or run of sections) of the other: "2K Sports - Major League
    // Baseball 2K7" / "Major League Baseball 2K7", "Golvellius" / "Golvellius: Valley of Doom"
    let (sa, sb) = (shorter(a), shorter(b));
    let within = |names: &[String], whole: &str| names.iter().any(|v| same_name(v, whole));
    if within(&sb, &na) || within(&sa, &nb) || ub.is_some_and(|u| within(&sa, u)) || ua.is_some_and(|u| within(&sb, u)) { return 0.92; }
    let d = strsim::normalized_levenshtein(&na, &nb) as f32;
    if same_numbers { d } else { d.min(0.8) }
}

/// Score a source's candidates against the game's title again (after the comparison above
/// changed), and their console from the platforms they list.
/// How alike a candidate's name is to a game known by these names (its own name, and the release
/// its fingerprint identified): the best of them. SteamGridDB's names are a little less sure.
/// The first name is the game's own; a release name that is only part of it ("Knights" for
/// "Playmobil Interactive - Knights") says less, so it counts no more than a part of a name does.
pub fn score_for(names: &[&str], c: &MetaCandidate) -> f32 {
    let words = |s: &str| normalize(s).split(' ').filter(|w| !w.is_empty()).map(String::from).collect::<std::collections::BTreeSet<String>>();
    let own = names.first().map(|n| words(n)).unwrap_or_default();
    let best = names.iter().enumerate().map(|(i, n)| {
        let s = c.likeness(n);
        let w = words(n);
        if i > 0 && w.len() < own.len() && w.is_subset(&own) { s.min(0.92) } else { s }
    }).fold(0.0, f32::max);
    if c.source == "steamgriddb" { best * 0.98 } else { best }
}

pub fn rescore(system: &SystemDef, names: &[&str], cands: &mut [MetaCandidate]) {
    for c in cands.iter_mut() {
        if matches!(c.source.as_str(), "igdb" | "rawg" | "steamgriddb") { c.score = score_for(names, c); }
        if c.console.as_deref() != Some("match") { if let Some(v) = consoles::verdict(system, &c.platforms) { c.console = Some(v); } }
    }
}

/// Pick the match from a game's candidates, or say it needs you.
/// Sure: the best candidate isn't for another console, its name is a match, a close-but-not-exact
/// name is confirmed for this console, and no other title is as good. Another listing of the same
/// title isn't a rival, and an unchecked one never outranks one that is confirmed for the console.
pub fn decide(mut cands: Vec<MetaCandidate>) -> MatchResult {
    let other = |c: &MetaCandidate| c.console.as_deref() == Some("other");
    let confirmed = |c: &MetaCandidate| c.console.as_deref() == Some("match");
    // a game for another console is never the match; it is listed last for you to see
    cands.sort_by(|a, b| other(a).cmp(&other(b)).then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)));
    let Some(best) = cands.first() else { return MatchResult::NoMatch };
    let name = normalize(&best.title);
    // a rival: another title, not just another spelling of this one ("Yar's Revenge"), that is as
    // good a match, not merely close behind (an exact name beats a part-of-the-name match)
    let rival = cands[1..].iter().any(|c| !other(c) && !same_name(&normalize(&c.title).replace(' ', ""), &name.replace(' ', "")) && c.score > best.score - 0.01
        && !(confirmed(best) && !confirmed(c) && c.score <= best.score));
    if !other(best) && best.score >= AUTO_SCORE && (best.score >= 0.95 || confirmed(best)) && !rival {
        return MatchResult::Auto(best.clone());
    }
    MatchResult::NeedsReview(cands.into_iter().take(10).collect())
}

pub fn query_for(db: &Db, game_id: i64) -> anyhow::Result<GameQuery<'static>> {
    let (sys, title, parsed): (String, String, Option<String>) = db.conn().query_row("SELECT system_id, title, parsed_json FROM games WHERE id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let system = by_id(&sys).ok_or_else(|| anyhow::anyhow!("unknown system {sys}"))?;
    let parsed: ParsedName = parsed.and_then(|p| serde_json::from_str(&p).ok()).unwrap_or_default();
    let f: Option<(Option<String>, Option<String>, i64, String, Option<String>)> = db.conn().query_row(
        "SELECT sha1, crc32, size, name, json_extract(probe_json,'$.id') FROM files WHERE game_id=?1 ORDER BY (sha1 IS NULL), size DESC LIMIT 1",
        [game_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).optional()?;
    let (sha1, crc32, size, file_name, probe_id) = match f { Some((a, b, c, d, e)) => (a, b, Some(c), Some(d), e), None => (None, None, None, None, None) };
    // searched without a pack's wording in front: with it, sources return other packs
    let title = unpack(&title).to_string();
    let release = release_name(db, game_id)?.filter(|r| normalize(r) != normalize(&title));
    Ok(GameQuery { system, title, release, sha1, crc32, size, file_name, probe_id, title_id: parsed.title_id })
}

/// The release a game's file was identified as by its fingerprint ("Lord of the Rings, The - The
/// Two Towers (USA, Europe)"), read the way it's said ("The Lord of the Rings - The Two Towers");
/// for an arcade set, the title its set name stands for ("aof": "Art of Fighting").
pub fn release_name(db: &Db, game_id: i64) -> anyhow::Result<Option<String>> {
    let dat: Option<String> = db.conn().query_row(
        "SELECT dat_game FROM files WHERE game_id=?1 AND dat_status='verified' AND dat_game IS NOT NULL ORDER BY size DESC LIMIT 1",
        [game_id], |r| r.get(0)).optional()?;
    // a game you own physically: the release your copy is
    let dat = match dat { Some(d) => Some(d), None => crate::shelf::list_release(db, game_id)? };
    if let Some(d) = dat {
        return Ok(Some(unpack(crate::parse::parse(&[crate::systems::ParserKind::NoIntro], &d).display()).to_string()).filter(|t| !t.trim().is_empty()));
    }
    let (sys, file): (String, Option<String>) = db.conn().query_row(
        "SELECT g.system_id, (SELECT name FROM files WHERE game_id=g.id ORDER BY size DESC LIMIT 1) FROM games g WHERE g.id=?1", [game_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    if by_id(&sys).is_some_and(|s| s.parsers.contains(&crate::systems::ParserKind::Mame)) {
        if let Some(set) = file.as_deref().and_then(|f| Path::new(f).file_stem()).map(|s| s.to_string_lossy().into_owned()) {
            return offline::arcade_title(db, &set);
        }
    }
    Ok(None)
}

impl GameQuery<'_> {
    /// The names the game is known by: its own, and the release its fingerprint identified.
    pub fn names(&self) -> Vec<&str> { std::iter::once(self.title.as_str()).chain(self.release.as_deref()).collect() }
}

/// The names to search a game by, in order: its whole name; without its first section, which is
/// often a label ("2K Sports - Major League Baseball 2K7"); its first section alone; without its
/// last word; without its first word.
pub fn search_names(title: &str) -> Vec<String> {
    let mut out = vec![title.to_string()];
    if let Some((i, len)) = [" - ", ":", " ~ "].iter().filter_map(|m| title.find(m).map(|i| (i, m.len()))).min() {
        for part in [title[i + len..].trim(), title[..i].trim()] {
            if part.chars().filter(|c| c.is_alphanumeric()).count() >= 3 && !out.iter().any(|o| o == part) { out.push(part.to_string()); }
        }
    }
    // last, more broadly: without its last word, which may be spelled differently in the source
    // ("Super Huey UH-IX" finds "Super Huey UH-1X")
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() >= 3 {
        let broader = words[..words.len() - 1].join(" ");
        let broader = broader.trim_end_matches(|c: char| !c.is_alphanumeric()).to_string();
        if !out.contains(&broader) { out.push(broader); }
        // or without its first word, often a label with no mark after it ("AWS Pro Moves Soccer")
        let rest = words[1..].join(" ");
        if !out.contains(&rest) { out.push(rest); }
    }
    out
}

/// Look a game up in every configured source and pick the best candidate.
pub fn match_game(db: &Db, http: &http::Http, settings: &Settings, game_id: i64) -> anyhow::Result<MatchResult> {
    let q = query_for(db, game_id)?;
    // 1. offline exact id
    let offline_hit = match q.system.id {
        // the base game's id, from the name or the file's header (an update or DLC id maps to it)
        "switch" => q.title_id.iter().chain(q.probe_id.iter()).find_map(|t| crate::util::switch_base_id(t))
            .and_then(|b| offline::lookup(db, "switch", &format!("{b:016X}")).ok().flatten()),
        "psp" => q.probe_id.as_ref().and_then(|id| offline::lookup(db, "psp", id).ok().flatten()),
        _ => None,
    };
    let mut cands: Vec<MetaCandidate> = offline_hit.into_iter().map(|mut c| { c.console = Some("match".into()); c.platforms = vec![q.system.name.to_string()]; c }).collect();
    // 2. ScreenScraper by hash
    if screenscraper::configured(settings) {
        if let Ok(Some(c)) = screenscraper::lookup(db, http, settings, &q) { cands.push(c); }
    }
    // 3. the console's catalog, matched here by every name the game is known by (and every name
    // IGDB knows each game by): the best few are fetched in full
    if igdb::configured(settings) && cands.iter().all(|c| c.score < AUTO_SCORE) {
        let names = q.names();
        if let Ok(best) = catalog::best(db, q.system.id, &names, 5, 0.6) {
            let ids: Vec<String> = best.into_iter().map(|(id, _)| id).collect();
            if let Ok(mut v) = igdb::by_ids(db, http, settings, q.system, &ids, &q.title) {
                for c in v.iter_mut() { c.score = score_for(&names, c); }
                cands.append(&mut v);
            }
        }
    }
    // 4. title search: the whole name, then (while nothing is a sure match) the name without its
    // label in front, and its first section alone. Whatever the name searched, every result is
    // scored against the game's whole name.
    if cands.iter().all(|c| c.score < AUTO_SCORE) {
        let first = cands.len();
        // each name it's known by, whole; then each one's broader forms
        let mut names: Vec<String> = Vec::new();
        let forms: Vec<Vec<String>> = q.names().iter().map(|n| search_names(n)).collect();
        for k in 0..forms.iter().map(Vec::len).max().unwrap_or(0) {
            for f in &forms { if let Some(n) = f.get(k) { if !names.contains(n) { names.push(n.clone()); } } }
        }
        let known: Vec<String> = q.names().iter().map(|s| s.to_string()).collect();
        let known: Vec<&str> = known.iter().map(String::as_str).collect();
        for (k, name) in names.into_iter().enumerate() {
            // done once a listing has the name and is for this console; a name alone isn't enough
            // (SteamGridDB doesn't say the console: "Fantasy Zone II: The Tears of Opa-Opa" there
            // is the arcade game, and the Master System one is found by the next name)
            if k > 0 && cands[first..].iter().any(|c| c.score >= 0.97 && c.console.as_deref() == Some("match")) { break; }
            let mut qq = q.clone();
            qq.title = name;
            if igdb::configured(settings) { if let Ok(mut v) = igdb::search(db, http, settings, &qq) { cands.append(&mut v); } }
            if rawg::configured(settings) { if let Ok(mut v) = rawg::search(db, http, settings, &qq) { cands.append(&mut v); } }
            if screenscraper::configured(settings) { if let Ok(mut v) = screenscraper::search(db, http, settings, &qq) { cands.append(&mut v); } }
            if sgdb::configured(settings) { if let Ok(mut v) = sgdb::search(db, http, settings, &qq.title) { cands.append(&mut v); } }
            for c in cands[first..].iter_mut() { c.score = score_for(&known, c); }
        }
        let mut seen = std::collections::HashSet::new();
        cands.retain(|c| seen.insert((c.source.clone(), c.source_id.clone())));
    }
    cands.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    // SteamGridDB doesn't say which console a game is for: check its best few elsewhere
    check_consoles_of(db, http, settings, q.system, &mut cands, 3);
    Ok(decide(cands))
}

/// Save a chosen candidate on the game; fetch a cover from SteamGridDB if the candidate has none.
pub fn apply_match(db: &Db, http: &http::Http, settings: &Settings, cache_dir: &Path, game_id: i64, mut c: MetaCandidate) -> anyhow::Result<()> {
    if c.source == "rawg" && rawg::configured(settings) { let _ = rawg::details(db, http, settings, &mut c); }
    if !c.art.iter().any(|a| a.kind == ArtKind::Cover) && sgdb::configured(settings) {
        if let Ok(v) = sgdb::search(db, http, settings, &c.title) {
            if let Some(best) = v.into_iter().find(|x| x.score >= 0.85) {
                if let Ok(mut art) = sgdb::art(db, http, settings, &best.source_id, ArtKind::Cover) { c.art.append(&mut art); }
            }
        }
    }
    let mut local: Vec<(ArtKind, PathBuf)> = Vec::new();
    for kind in [ArtKind::Cover, ArtKind::Icon, ArtKind::Hero, ArtKind::Banner, ArtKind::Screenshot] {
        if let Some(a) = c.art.iter().find(|a| a.kind == kind) {
            if let Ok(p) = http.fetch_art(cache_dir, &c.source, &format!("{}-{:?}", c.source_id, kind).to_lowercase(), &a.url) { local.push((kind, p)); }
        }
    }
    // the box scan of the exact release, when the file was identified and its scan is stored
    if let Some((sys, rel)) = boxart::release_of(db, game_id)? {
        if let Some(p) = boxart::cached(cache_dir, &sys, &rel) { local.retain(|(k, _)| *k != ArtKind::Cover); local.push((ArtKind::Cover, p)); }
    }
    let art_json = serde_json::to_string(&local.iter().map(|(k, p)| (format!("{k:?}").to_lowercase(), crate::util::p2s(p))).collect::<std::collections::BTreeMap<_, _>>())?;
    db.conn().execute("UPDATE games SET meta_json=?1, art_json=?2, meta_state='matched' WHERE id=?3", (serde_json::to_string(&c)?, art_json, game_id))?;
    Ok(())
}

/// Title search across the configured online sources (used for "Search again" in Match review).
pub fn search_titles(db: &Db, http: &http::Http, settings: &Settings, game_id: i64, query: &str) -> anyhow::Result<Vec<MetaCandidate>> {
    if !any_source(settings) {
        anyhow::bail!("Searching by name needs a SteamGridDB, RAWG or IGDB key, or ScreenScraper, in Settings → Game info.");
    }
    let mut q = query_for(db, game_id)?;
    q.title = unpack(query.trim()).to_string();
    let mut out = Vec::new();
    // one source failing doesn't hide the others' results
    let mut errors = Vec::new();
    if igdb::configured(settings) { match igdb::search(db, http, settings, &q) { Ok(v) => out.extend(v), Err(e) => errors.push(format!("IGDB: {e:#}")) } }
    if rawg::configured(settings) { match rawg::search_any(db, http, settings, q.system, &q.title) { Ok(v) => out.extend(v), Err(e) => errors.push(format!("RAWG: {e:#}")) } }
    if screenscraper::configured(settings) { match screenscraper::search(db, http, settings, &q) { Ok(v) => out.extend(v), Err(e) => errors.push(format!("ScreenScraper: {e:#}")) } }
    if sgdb::configured(settings) { match sgdb::search(db, http, settings, &q.title) { Ok(v) => out.extend(v), Err(e) => errors.push(format!("SteamGridDB: {e:#}")) } }
    if out.is_empty() && !errors.is_empty() { anyhow::bail!("{}", errors.join("; ")); }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(12);
    check_consoles_of(db, http, settings, q.system, &mut out, 6);
    out.sort_by(|a, b| (a.console.as_deref() == Some("other")).cmp(&(b.console.as_deref() == Some("other"))).then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)));
    Ok(out)
}

/// A small preview image URL for a candidate (its own cover, or a SteamGridDB grid thumbnail).
pub fn candidate_preview(db: &Db, http: &http::Http, settings: &Settings, c: &MetaCandidate) -> anyhow::Result<Option<String>> {
    if let Some(a) = c.art.iter().find(|a| a.kind == ArtKind::Cover) { return Ok(Some(a.url.clone())); }
    if c.source == "steamgriddb" && sgdb::configured(settings) { return sgdb::thumb(db, http, settings, &c.source_id); }
    Ok(None)
}

pub fn set_review(db: &Db, game_id: i64, cands: &[MetaCandidate]) -> anyhow::Result<()> {
    db.conn().execute("UPDATE games SET meta_state='review', meta_json=?1 WHERE id=?2", (serde_json::to_string(&serde_json::json!({"candidates": cands}))?, game_id))?;
    Ok(())
}
