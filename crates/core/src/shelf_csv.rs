//! Bring a spreadsheet of your physical games onto the shelf: read the CSV (whatever program
//! made it), guess which column is which, match each row to a release, then add the matches.
use crate::completion::title_key;
use crate::db::Db;
use crate::shelf::CopyFields;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Table { pub headers: Vec<String>, pub rows: Vec<Vec<String>> }

/// Read CSV text: a byte-order mark is skipped, the delimiter (`,` `;` or tab) is the one the
/// header line uses most, quotes work as spreadsheets write them, and empty rows are dropped.
pub fn parse(text: &str) -> anyhow::Result<Table> {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().next().unwrap_or("");
    let delim = [',', ';', '\t'].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or(',');
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut field, mut quoted, mut chars) = (Vec::new(), String::new(), false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => { field.push('"'); chars.next(); }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, c) if c == delim => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => { row.push(std::mem::take(&mut field)); rows.push(std::mem::take(&mut row)); }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() { row.push(field); rows.push(row); }
    let rows: Vec<Vec<String>> = rows.into_iter().map(|r| r.into_iter().map(|f| f.trim().to_string()).collect::<Vec<_>>()).filter(|r| r.iter().any(|f| !f.is_empty())).collect();
    let mut it = rows.into_iter();
    let headers = it.next().ok_or_else(|| anyhow::anyhow!("the file is empty"))?;
    Ok(Table { headers, rows: it.collect() })
}

/// Which column holds what (by position).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mapping {
    pub title: Option<usize>, pub console: Option<usize>, pub region: Option<usize>, pub edition: Option<usize>, pub parts: Option<usize>,
    pub condition: Option<usize>, pub location: Option<usize>, pub price: Option<usize>, pub date: Option<usize>, pub notes: Option<usize>,
}

/// Columns guessed from their headers.
pub fn guess_mapping(headers: &[String]) -> Mapping {
    let find = |names: &[&str]| headers.iter().position(|h| { let h = h.trim().to_lowercase(); names.iter().any(|n| h == *n) })
        .or_else(|| headers.iter().position(|h| { let h = h.trim().to_lowercase(); names.iter().any(|n| h.contains(n)) }));
    Mapping {
        title: find(&["title", "name", "game", "game title"]),
        console: find(&["console", "system", "platform"]),
        region: find(&["region", "country"]),
        edition: find(&["edition", "version", "variant"]),
        parts: find(&["parts", "complete", "completeness", "cib", "contents", "what i have"]),
        condition: find(&["condition", "grade"]),
        location: find(&["location", "where", "shelf", "storage", "stored"]),
        price: find(&["price", "paid", "cost"]),
        date: find(&["date", "bought", "purchased", "acquired"]),
        notes: find(&["notes", "note", "comments", "comment"]),
    }
}

/// A price as cents: currency signs and thousands separators ignored; "1.234,50" works too.
pub fn price_cents(s: &str) -> Option<i64> {
    let t: String = s.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
    if t.is_empty() { return None; }
    // the last separator followed by one or two digits is the decimal point
    let dec = t.rfind(['.', ',']).filter(|i| (1..=2).contains(&(t.len() - i - 1)));
    let (whole, frac) = match dec { Some(i) => (&t[..i], &t[i + 1..]), None => (t.as_str(), "") };
    let whole: String = whole.chars().filter(|c| c.is_ascii_digit()).collect();
    let w: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let f: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().ok()? };
    Some(w * 100 + f)
}

/// A date as YYYY-MM-DD, from that form, M/D/YYYY or D.M.YYYY.
pub fn date(s: &str) -> Option<String> {
    let s = s.trim();
    let n: Vec<&str> = s.split(['-', '/', '.']).collect();
    let (y, m, d) = match (n.as_slice(), s.contains('-'), s.contains('/')) {
        ([y, m, d], true, _) => (*y, *m, *d),
        // M/D/YYYY, unless the first number can only be a day
        ([a, b, y], _, true) => if a.parse::<u32>().unwrap_or(0) > 12 { (*y, *b, *a) } else { (*y, *a, *b) },
        ([d, m, y], false, false) => (*y, *m, *d),
        _ => return None,
    };
    let (y, m, d): (i32, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    chrono::NaiveDate::from_ymd_opt(y, m, d).map(|x| x.format("%Y-%m-%d").to_string())
}

/// Parts from words like "CIB", "complete", "sealed", "loose", "box only".
pub fn parts(s: &str, f: &mut CopyFields) {
    let s = s.to_lowercase();
    if s.contains("sealed") || s == "new" || s.contains("nib") { f.sealed = true; f.cart = true; f.box_ = true; f.manual = true; }
    else if s.contains("cib") || s.contains("complete") { f.cart = true; f.box_ = true; f.manual = true; }
    else if s.contains("box only") { f.box_ = true; }
    else if s.contains("manual only") { f.manual = true; }
    else if s.contains("box") { f.cart = true; f.box_ = true; f.manual = s.contains("manual") && !s.contains("no manual") && !s.contains("without manual"); }
    else { f.cart = true; }
}

/// A condition grade from what people write ("NM", "Very good", "VG+").
pub fn condition(s: &str) -> Option<String> {
    let t = s.trim().to_lowercase().replace(['+', '-'], "");
    Some(match t.trim() {
        "mint" | "m" => "mint", "near mint" | "nm" | "like new" => "near_mint", "very good" | "vg" | "excellent" | "ex" => "very_good",
        "good" | "g" => "good", "fair" | "f" | "acceptable" => "fair", "poor" | "p" | "bad" => "poor",
        _ => return None,
    }.to_string())
}

/// What people call consoles, beside Index's own names and folders.
const CONSOLE_NAMES: &[(&str, &str)] = &[
    ("super nintendo", "snes"), ("super famicom", "snes"), ("super nintendo entertainment system", "snes"),
    ("nintendo entertainment system", "nes"), ("famicom", "nes"), ("nintendo", "nes"),
    ("sega genesis", "genesis"), ("mega drive", "genesis"), ("sega mega drive", "genesis"), ("genesis", "genesis"),
    ("game boy", "gb"), ("gameboy", "gb"), ("game boy color", "gbc"), ("gameboy color", "gbc"), ("game boy advance", "gba"), ("gameboy advance", "gba"),
    ("nintendo ds", "nds"), ("ds", "nds"), ("nintendo 3ds", "n3ds"), ("3ds", "n3ds"), ("nintendo switch", "switch"), ("switch", "switch"),
    ("playstation", "ps1"), ("playstation 1", "ps1"), ("ps one", "ps1"), ("psx", "ps1"), ("ps1", "ps1"),
    ("playstation portable", "psp"), ("ps vita", "psvita"), ("playstation vita", "psvita"), ("vita", "psvita"),
    ("gamecube", "gamecube"), ("nintendo gamecube", "gamecube"), ("game gear", "gamegear"), ("sega game gear", "gamegear"),
    ("master system", "sms"), ("sega master system", "sms"), ("sega 32x", "sega32x"), ("32x", "sega32x"), ("turbografx 16", "pce"), ("pc engine", "pce"),
    ("atari 2600", "atari2600"), ("atari 5200", "atari5200"), ("atari 7800", "atari7800"), ("atari lynx", "lynx"), ("lynx", "lynx"),
    ("virtual boy", "virtualboy"), ("neo geo pocket color", "ngpc"), ("vectrex", "vectrex"), ("sg 1000", "sg1000"),
];

/// The console a spreadsheet names, if Index has it.
pub fn console(s: &str) -> Option<&'static str> {
    let k = title_key(s);
    if k.is_empty() { return None; }
    for sys in crate::systems::registry() {
        let names = std::iter::once(sys.id).chain(std::iter::once(sys.name)).chain(sys.folders.iter().copied()).chain(sys.folders.iter().filter_map(|f| f.split_once(" - ").map(|x| x.1)));
        if names.map(title_key).any(|n| n == k) { return Some(sys.id); }
    }
    CONSOLE_NAMES.iter().find(|(n, _)| title_key(n) == k).map(|(_, id)| *id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRow {
    /// the row's line in the file (1 = the first after the headers)
    pub line: usize,
    pub system: Option<String>,
    pub title: String,
    /// the release it matched, or the title for consoles without a list
    pub release: Option<String>,
    /// "exact" (the list's name), "close" (by title, and region when given), "typed" (no list:
    /// taken as written), "none"
    pub how: String,
    pub region: Option<String>,
    pub fields: CopyFields,
    /// a copy of this release with this edition and these notes is on the shelf already
    pub duplicate: bool,
}

/// Each row matched to a release, with what it says about the copy.
pub fn preview(db: &Db, t: &Table, m: &Mapping, default_system: Option<&str>, region_order: &[String]) -> anyhow::Result<Vec<ImportRow>> {
    let cell = |row: &Vec<String>, i: Option<usize>| i.and_then(|i| row.get(i)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut lists: std::collections::HashMap<String, Vec<(String, crate::parse::ParsedName)>> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (n, row) in t.rows.iter().enumerate() {
        let title = cell(row, m.title).unwrap_or_default();
        let system = cell(row, m.console).map(|c| console(&c).map(String::from)).unwrap_or_else(|| default_system.map(String::from));
        let region = cell(row, m.region);
        let mut f = CopyFields { edition: cell(row, m.edition), location: cell(row, m.location), notes: cell(row, m.notes), ..Default::default() };
        match cell(row, m.parts) { Some(p) => parts(&p, &mut f), None => f.cart = true }
        f.condition = cell(row, m.condition).and_then(|c| condition(&c));
        f.price_cents = cell(row, m.price).and_then(|p| price_cents(&p));
        f.bought = cell(row, m.date).and_then(|d| date(&d));
        let (mut release, mut how) = (None, "none".to_string());
        if let (Some(sys), false) = (&system, title.is_empty()) {
            if !lists.contains_key(sys) {
                let parsers = crate::systems::by_id(sys).map(|s| s.parsers).unwrap_or(&[]);
                let mut q = db.conn().prepare("SELECT DISTINCT r.game_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1")?;
                let names: Vec<String> = q.query_map([sys], |r| r.get(0))?.collect::<Result<_, _>>()?;
                lists.insert(sys.clone(), names.into_iter().map(|n| { let p = crate::parse::parse(parsers, &n); (n, p) }).collect());
            }
            let list = &lists[sys];
            if list.is_empty() {
                release = Some(title.clone());
                how = "typed".into();
            } else if let Some((name, _)) = list.iter().find(|(n, _)| n.eq_ignore_ascii_case(&title)) {
                release = Some(name.clone()); how = "exact".into();
            } else {
                let k = title_key(&title);
                // retail releases first: no beta, demo or other extra tags
                let mut cands: Vec<&(String, crate::parse::ParsedName)> = list.iter().filter(|(_, p)| title_key(&p.title) == k || title_key(p.display()) == k).collect();
                if let Some(r) = &region { cands.retain(|(_, p)| p.regions.iter().any(|x| x.eq_ignore_ascii_case(r))); }
                let rank = |p: &crate::parse::ParsedName| p.regions.iter().filter_map(|x| region_order.iter().position(|o| o == x)).min().unwrap_or(region_order.len());
                cands.sort_by(|a, b| a.1.tags.len().cmp(&b.1.tags.len()).then(rank(&a.1).cmp(&rank(&b.1))).then(a.1.rev.cmp(&b.1.rev)).then(a.0.cmp(&b.0)));
                if let Some((name, _)) = cands.first() { release = Some(name.clone()); how = "close".into(); }
            }
        }
        let duplicate = match (&system, &release) {
            (Some(s), Some(r)) => db.conn().query_row("SELECT COUNT(*) > 0 FROM shelf_copies WHERE deleted=0 AND system_id=?1 AND release=?2 AND edition IS ?3 AND notes IS ?4",
                (s, r, &f.edition, &f.notes), |x| x.get(0))?,
            _ => false,
        };
        out.push(ImportRow { line: n + 1, system, title, release, how, region, fields: f, duplicate });
    }
    Ok(out)
}

/// Add the matched rows that aren't on the shelf already. Returns how many were added.
pub fn import(db: &Db, rows: &[ImportRow]) -> anyhow::Result<usize> {
    // all or nothing: a failure part-way leaves the shelf as it was
    let tx = db.conn().unchecked_transaction()?;
    let mut n = 0;
    for r in rows.iter().filter(|r| !r.duplicate && r.how != "none") {
        let (Some(sys), Some(rel)) = (&r.system, &r.release) else { continue };
        let region = if r.how == "typed" { r.region.as_deref() } else { None };
        crate::shelf::add(db, sys, rel, region, r.fields.clone())?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}
