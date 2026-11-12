//! Filename → structured fields. Never panics on odd input.
use crate::systems::ParserKind;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedName {
    pub title: String,
    pub regions: Vec<String>,
    pub langs: Vec<String>,
    pub rev: Option<String>,
    pub version: Option<String>,
    pub disc: Option<u32>,
    pub tags: Vec<String>,
    pub title_id: Option<String>,
    pub serial: Option<String>,
    pub flags: Vec<String>,
    /// Human-friendly title ("The Addams Family") when it differs from the No-Intro form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
}

impl ParsedName {
    pub fn display(&self) -> &str { self.display_title.as_deref().unwrap_or(&self.title) }
}

impl ParsedName {
    /// Key used to group releases of the same game (title + regions), case-insensitive.
    pub fn group_key(&self) -> String {
        format!("{}|{}", self.title.to_lowercase(), self.regions.join(",").to_lowercase())
    }
}

const REGIONS: &[&str] = &[
    "USA", "Europe", "Japan", "World", "Asia", "Australia", "Brazil", "Canada", "China", "France",
    "Germany", "Hong Kong", "Italy", "Korea", "Netherlands", "Spain", "Sweden", "Taiwan", "UK",
    "Russia", "Scandinavia", "Denmark", "Finland", "Norway", "Poland", "Portugal", "Greece", "Mexico",
    "Argentina", "Latin America", "Unknown",
];
const REGION_ALIASES: &[(&str, &str)] = &[
    ("US", "USA"), ("EU", "Europe"), ("EUR", "Europe"), ("JP", "Japan"), ("JPN", "Japan"),
    ("PAL", "Europe"), ("NTSC-U", "USA"), ("NTSC-J", "Japan"),
];
const LANGS: &[&str] = &[
    "En", "Ja", "Fr", "De", "Es", "It", "Nl", "Pt", "Sv", "No", "Da", "Fi", "Zh", "Ko", "Pl", "Ru",
    "Ca", "Cs", "El", "Hu", "Tr", "Ar", "He", "Hr", "Sk", "Sl", "Ro", "Uk", "Th", "Vi", "Id",
];

fn goodtools_code(c: char) -> Option<&'static str> {
    Some(match c {
        'U' => "USA", 'J' => "Japan", 'E' => "Europe", 'W' => "World", 'K' => "Korea", 'F' => "France",
        'G' => "Germany", 'S' => "Spain", 'I' => "Italy", 'A' => "Australia", 'B' => "Brazil",
        'C' => "China", 'H' => "Netherlands", 'N' => "Netherlands", 'R' => "Russia", 'T' => "Taiwan",
        _ => return None,
    })
}

static RE_DECRYPTED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)[\s_\-.]*decrypted$").unwrap());
static RE_TITLE_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)0100[0-9a-f]{12}$").unwrap());
static RE_SWITCH_VER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^v(\d+)$").unwrap());
static RE_REV: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)rev\s*([0-9a-z.]+)$").unwrap());
static RE_VER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)v\s*(\d[\w.]*)$").unwrap());
static RE_DISC: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)(?:disc|disk|cd)\s*(\d+)(?:\s+of\s+\d+)?$").unwrap());
static RE_SERIAL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)([A-Z]{4})[-_ ]?(\d{3})\.?(\d{2})$").unwrap());
static RE_ARTICLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(.*?), (The|A|An)( - .*)?$").unwrap());
static RE_BARE_TID: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(?:^|\s)(0100[0-9a-f]{12})(?:\s+v(\d+))?(?:\s|$)").unwrap());
static RE_DOMAIN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?i)[a-z0-9-]+(\.[a-z0-9-]+)*\.[a-z]{2,6}$").unwrap());
/// Release-site and packaging labels that say nothing about the content.
const SCENE_TAGS: &[&str] = &["nsw2u", "eshop", "nsp", "xci", "nsz", "xcz", "dlc", "upd", "update", "base", "base game", "switch", "nx", "multi"];

fn is_scene_tag(g: &str) -> bool {
    let l = g.trim().to_lowercase();
    SCENE_TAGS.contains(&l.as_str()) || RE_DOMAIN.is_match(&l)
}

/// "i-am-setsuna" → "I Am Setsuna". Only all-lowercase slugs: mixed-case run-together names
/// ("Zelda-Update1.2.1-NSP-Site.com") carry information a guess would mangle.
fn deslug(t: &str) -> Option<String> {
    if t.contains(' ') || !t.contains(['-', '_']) || !t.chars().any(|c| c.is_alphabetic()) || t.chars().any(|c| c.is_uppercase()) { return None; }
    let words: Vec<&str> = t.split(['-', '_']).filter(|w| !w.is_empty()).collect();
    if words.len() < 2 { return None; }
    Some(words.iter().map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default() }).collect::<Vec<String>>().join(" "))
}

/// Comparison form of a title: case, accents, ™/®, and punctuation ignored.
/// "Pokémon™ Legends꞉ Arceus" and "pokemon-legends-arceus" both give "pokemon legends arceus".
pub fn norm_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let c = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a', 'è' | 'é' | 'ê' | 'ë' => 'e', 'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o', 'ù' | 'ú' | 'û' | 'ü' => 'u', 'ñ' => 'n', 'ç' => 'c', 'ý' | 'ÿ' => 'y',
            '™' | '®' | '©' | '\'' | '’' | '‘' => continue,
            c => c,
        };
        if c.is_alphanumeric() { out.push(c) } else if !out.ends_with(' ') { out.push(' ') }
    }
    out.trim().to_string()
}

static RE_SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Split "Title (a) [b] (c)" into the title and the bracket groups, tolerating unbalanced brackets.
fn split_groups(s: &str) -> (String, Vec<(char, String)>) {
    let mut title = String::new();
    let mut groups = Vec::new();
    let mut chars = s.char_indices().peekable();
    let mut in_title = true;
    while let Some((i, c)) = chars.next() {
        let close = match c { '(' => ')', '[' => ']', _ => '\0' };
        if close != '\0' {
            if let Some(end) = s[i + 1..].find(close) {
                let inner = &s[i + 1..i + 1 + end];
                if !inner.contains(['(', '[']) {
                    groups.push((c, inner.trim().to_string()));
                    in_title = false;
                    // advance past the group
                    while let Some(&(j, _)) = chars.peek() { if j <= i + 1 + end { chars.next(); } else { break; } }
                    continue;
                }
            }
        }
        if in_title { title.push(c); }
    }
    (title, groups)
}

/// "Name (USA)-decrypted" → "Name (USA)"; unchanged if there is no suffix.
pub fn strip_decrypted_suffix(stem: &str) -> String {
    match RE_DECRYPTED.find(stem) { Some(m) if m.start() > 0 => stem[..m.start()].to_string(), _ => stem.to_string() }
}

pub fn parse(kinds: &[ParserKind], stem: &str) -> ParsedName {
    let mut p = ParsedName::default();
    let mut s = stem.trim().to_string();
    if let Some(m) = RE_DECRYPTED.find(&s) {
        if m.start() > 0 {
            s.truncate(m.start());
            p.flags.push("decrypted-suffix".into());
        }
    }
    if kinds.contains(&ParserKind::Mame) && kinds.len() == 1 {
        p.title = s;
        return p;
    }
    let goodtools = kinds.contains(&ParserKind::GoodTools);
    let switch = kinds.contains(&ParserKind::Switch);
    // "[BIOS] Sega CD 2 (USA)": a leading group labels the file; the title follows it
    let mut lead = Vec::new();
    while let Some(rest) = s.strip_prefix('[') {
        let Some(end) = rest.find(']') else { break };
        if rest[end + 1..].trim().is_empty() { break; }
        lead.push(('[', rest[..end].trim().to_string()));
        s = rest[end + 1..].trim_start().to_string();
    }
    let (mut title, mut groups) = split_groups(&s);
    if !lead.is_empty() { lead.append(&mut groups); groups = lead; }
    if switch {
        // "Pokemon Let's Go Eevee 0100187003A36000 v0"
        if let Some(c) = RE_BARE_TID.captures(&title) {
            p.title_id = Some(c[1].to_uppercase());
            if let Some(v) = c.get(2) { p.version = Some(v.as_str().to_string()); }
            let m = c.get(0).unwrap();
            title = format!("{} {}", &title[..m.start()], &title[m.end()..]);
        }
    }
    for (open, g) in groups {
        if g.is_empty() { continue; }
        if switch && is_scene_tag(&g) { continue; }
        if RE_TITLE_ID.is_match(&g) { p.title_id = Some(g.to_uppercase()); continue; }
        if open == '[' {
            if let Some(c) = RE_SWITCH_VER.captures(&g) { p.version = Some(c[1].to_string()); continue; }
        }
        if let Some(c) = RE_SERIAL.captures(&g) {
            p.serial = Some(format!("{}-{}{}", c[1].to_uppercase(), &c[2], &c[3]));
            continue;
        }
        if g == "!" { p.tags.push("verified-good".into()); continue; }
        let items: Vec<&str> = g.split(',').map(|x| x.trim()).collect();
        let as_regions: Option<Vec<String>> = items.iter().map(|it| {
            REGIONS.iter().find(|r| r.eq_ignore_ascii_case(it)).map(|r| r.to_string())
                .or_else(|| REGION_ALIASES.iter().find(|(a, _)| a.eq_ignore_ascii_case(it)).map(|(_, r)| r.to_string()))
        }).collect();
        if let Some(r) = as_regions { if p.regions.is_empty() { p.regions = r; continue; } }
        if goodtools && p.regions.is_empty() && g.len() <= 4 && g.chars().all(|c| c.is_ascii_uppercase()) {
            let mapped: Option<Vec<&str>> = g.chars().map(goodtools_code).collect();
            if let Some(m) = mapped {
                for r in m { if !p.regions.iter().any(|x| x == r) { p.regions.push(r.to_string()); } }
                continue;
            }
        }
        let lang_items: Vec<&str> = g.split([',', '+']).map(|x| x.trim()).collect();
        if lang_items.iter().all(|it| LANGS.iter().any(|l| l.eq_ignore_ascii_case(it))) {
            p.langs = lang_items.iter().map(|s| s.to_string()).collect();
            continue;
        }
        if let Some(c) = RE_REV.captures(&g) { p.rev = Some(c[1].to_string()); continue; }
        if let Some(c) = RE_VER.captures(&g) { p.version = Some(c[1].to_string()); continue; }
        if let Some(c) = RE_DISC.captures(&g) { p.disc = c[1].parse().ok(); continue; }
        p.tags.push(g);
    }
    let mut t = RE_SPACES.replace_all(title.trim(), " ").to_string();
    while t.ends_with(" -") || t.ends_with('-') || t.ends_with('_') { t.pop(); t = t.trim_end().to_string(); }
    if switch {
        if let Some(d) = deslug(&t) { t = d; p.flags.push("slug".into()); }
    }
    if let Some(c) = RE_ARTICLE.captures(&t) {
        let rest = c.get(3).map(|m| m.as_str()).unwrap_or("");
        p.display_title = Some(format!("{} {}{}", &c[2], &c[1], rest));
    }
    p.title = t;
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use ParserKind::*;
    const NI: &[ParserKind] = &[NoIntro, GoodTools];
    #[test] fn nointro_proto_date() {
        let p = parse(NI, "Alien vs Predator (USA) (Proto) (1993-12-17)");
        assert_eq!(p.title, "Alien vs Predator");
        assert_eq!(p.regions, vec!["USA"]);
        assert_eq!(p.tags, vec!["Proto", "1993-12-17"]);
    }
    #[test] fn multi_region() {
        assert_eq!(parse(NI, "A.P.B. (USA, Europe)").regions, vec!["USA", "Europe"]);
    }
    #[test] fn goodtools_codes() {
        assert_eq!(parse(NI, "1942 (JU)").regions, vec!["Japan", "USA"]);
        assert_eq!(parse(NI, "10-Yard Fight (U)").regions, vec!["USA"]);
        let p = parse(NI, "Zelda (U) [!]");
        assert!(p.tags.contains(&"verified-good".to_string()));
    }
    #[test] fn ds_short_region() {
        assert_eq!(parse(NI, "007 - Blood Stone (US)").regions, vec!["USA"]);
    }
    #[test] fn langs() {
        let p = parse(NI, "Baseball Stars Color (World) (En,Ja)");
        assert_eq!(p.regions, vec!["World"]);
        assert_eq!(p.langs, vec!["En", "Ja"]);
    }
    #[test] fn rev_and_disc() {
        assert_eq!(parse(NI, "Metroid Prime (USA) (Rev 2)").rev.as_deref(), Some("2"));
        assert_eq!(parse(NI, "Medal of Honor - Rising Sun (USA) (Disc 1)").disc, Some(1));
        assert_eq!(parse(NI, "Atari PAM System Test (USA) (v1.02)").version.as_deref(), Some("1.02"));
    }
    #[test] fn switch_ids() {
        let p = parse(&[Switch, NoIntro], "Atelier Ayesha꞉ The Alchemist of Dusk DX [0100D9D00EE8C000][v0]");
        assert_eq!(p.title, "Atelier Ayesha꞉ The Alchemist of Dusk DX");
        assert_eq!(p.title_id.as_deref(), Some("0100D9D00EE8C000"));
        assert_eq!(p.version.as_deref(), Some("0"));
    }
    #[test] fn leading_bios_label() {
        let p = parse(NI, "[BIOS] Sega CD 2 (USA) (v2.00)");
        assert_eq!((p.title.as_str(), p.version.as_deref(), p.tags.clone()), ("Sega CD 2", Some("2.00"), vec!["BIOS".to_string()]));
        assert_eq!(parse(NI, "[BIOS] 32X SH-2 Master (USA)").title, "32X SH-2 Master");
        // a name that is only a bracket stays as it was
        assert_eq!(parse(NI, "[BIOS]").title, "");
    }
    #[test] fn switch_bare_ids_scene_tags_and_slugs() {
        let sw = &[ParserKind::Switch, ParserKind::NoIntro];
        let p = parse(sw, "Pokemon Let's Go Eevee 0100187003A36800 v131072");
        assert_eq!((p.title.as_str(), p.title_id.as_deref(), p.version.as_deref()), ("Pokemon Let's Go Eevee", Some("0100187003A36800"), Some("131072")));
        let p = parse(sw, "Pokemon Sword [The Crown Tundra][0100ABF008969002][US][v0]");
        assert_eq!((p.title.as_str(), p.tags.clone(), p.regions.clone()), ("Pokemon Sword", vec!["The Crown Tundra".to_string()], vec!["USA".to_string()]));
        let p = parse(sw, "Pokemon Shield [01008DB008C2C800][v458752](nsw2u)");
        assert!(p.tags.is_empty(), "{:?}", p.tags);
        let p = parse(sw, "The Legend of Zelda [0100F2C0115B6000][v0][US](nsw2u.com)");
        assert!(p.tags.is_empty(), "{:?}", p.tags);
        let p = parse(sw, "Pokemon Shield [DLC][01008DB008C2D001][v0]");
        assert!(p.tags.is_empty(), "{:?}", p.tags);
        assert_eq!(parse(sw, "i-am-setsuna").title, "I Am Setsuna");
        assert_eq!(parse(sw, "Firmware.22.5.0").title, "Firmware.22.5.0");
        assert_eq!(norm_title("Pokémon™ Legends꞉ Arceus"), norm_title("Pokemon Legends Arceus"));
        assert_eq!(norm_title("Pokémon™꞉ Let’s Go, Eevee!"), "pokemon lets go eevee");
    }
    #[test] fn decrypted_suffix() {
        let p = parse(NI, "Mario Kart 7 (USA)-decrypted");
        assert_eq!(p.title, "Mario Kart 7");
        assert_eq!(p.regions, vec!["USA"]);
        assert!(p.flags.contains(&"decrypted-suffix".to_string()));
        let q = parse(NI, "Attack of the Friday Monsters! A Tokyo Tale (USA) Decrypted");
        assert_eq!(q.title, "Attack of the Friday Monsters! A Tokyo Tale");
        assert!(q.flags.contains(&"decrypted-suffix".to_string()));
        let r = parse(NI, "Bravely Default-decrypted");
        assert_eq!(r.title, "Bravely Default");
    }
    #[test] fn hack_tag() {
        assert_eq!(parse(NI, "Pokemon - Gaia (FireRed Hack)").tags, vec!["FireRed Hack"]);
    }
    #[test] fn article_moved() {
        let p = parse(NI, "Addams Family, The - Pugsley's Scavenger Hunt (USA, Europe)");
        assert_eq!(p.title, "Addams Family, The - Pugsley's Scavenger Hunt", "file-name form stays No-Intro style");
        assert_eq!(p.display(), "The Addams Family - Pugsley's Scavenger Hunt");
        assert_eq!(parse(NI, "Activision Decathlon, The").display(), "The Activision Decathlon");
    }
    #[test] fn ps_serials() {
        let p = parse(&[NoIntro, GoodTools, Serial], "007 Racing [U] [SLUS_013.00]");
        assert_eq!(p.serial.as_deref(), Some("SLUS-01300"));
        assert_eq!(p.regions, vec!["USA"]);
        assert_eq!(parse(&[NoIntro, GoodTools, Serial], "2Xtreme (U) (SCUS-94508)").serial.as_deref(), Some("SCUS-94508"));
    }
    #[test] fn mame_kept() {
        assert_eq!(parse(&[Mame], "2020bb").title, "2020bb");
    }
    #[test] fn junk_does_not_panic() {
        parse(NI, ""); parse(NI, "((("); parse(NI, ")))[[["); parse(NI, "a (b"); parse(NI, "CON");
    }
}
