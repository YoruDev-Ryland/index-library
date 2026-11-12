//! Your physical collection: one row per copy (a cart, a disc, a boxed game) of an exact
//! release. A game exists while it has files or copies, so a game you only own physically is
//! in the library like any other; when its files turn up they join the same game.
use crate::db::Db;
use crate::parse::parse;
use crate::systems::{by_id, GroupKind};
use crate::userdata::GameRef;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

pub use index_sync_protocol::ShelfCopy as Copy;

/// Condition grades, best first.
pub const CONDITIONS: &[&str] = &["mint", "near_mint", "very_good", "good", "fair", "poor"];

/// What you record about a copy (everything but which release it is).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CopyFields {
    pub edition: Option<String>,
    pub cart: bool,
    #[serde(rename = "box")]
    pub box_: bool,
    pub manual: bool,
    pub inserts: bool,
    pub sealed: bool,
    pub condition: Option<String>,
    pub location: Option<String>,
    pub price_cents: Option<i64>,
    pub bought: Option<String>,
    pub notes: Option<String>,
}

/// What a copy is, by the parts you have: Sealed, CIB (media, box and manual), Boxed (media and
/// box), Loose (media only), Box only, Manual only.
pub fn parts_label(c: &Copy) -> &'static str {
    match (c.sealed, c.cart, c.box_, c.manual) {
        (true, ..) => "Sealed",
        (_, true, true, true) => "CIB",
        (_, true, true, false) => "Boxed",
        (_, true, false, _) => "Loose",
        (_, false, true, _) => "Box only",
        (_, false, false, true) => "Manual only",
        _ => "Loose",
    }
}

/// Labels best first, for "the best copy you have".
pub const LABELS: &[&str] = &["Sealed", "CIB", "Boxed", "Loose", "Box only", "Manual only"];

fn clean(s: Option<String>) -> Option<String> { s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty()) }

fn row(r: &rusqlite::Row) -> rusqlite::Result<Copy> {
    Ok(Copy {
        id: r.get("id")?, game: serde_json::from_str(&r.get::<_, String>("ref_json")?).unwrap_or_default(), release: r.get("release")?,
        region: r.get("region")?, edition: r.get("edition")?, cart: r.get::<_, i64>("cart")? != 0, box_: r.get::<_, i64>("box")? != 0,
        manual: r.get::<_, i64>("manual")? != 0, inserts: r.get::<_, i64>("inserts")? != 0, sealed: r.get::<_, i64>("sealed")? != 0,
        condition: r.get("condition")?, location: r.get("location")?, price_cents: r.get("price_cents")?, bought: r.get("bought")?,
        notes: r.get("notes")?, deleted: r.get::<_, i64>("deleted")? != 0,
    })
}

/// A copy by id (removed ones too).
pub fn get(db: &Db, id: &str) -> anyhow::Result<Option<Copy>> {
    Ok(db.conn().query_row("SELECT * FROM shelf_copies WHERE id=?1", [id], row).optional()?)
}

/// The copies of a game, oldest first.
pub fn copies_of(db: &Db, game_id: i64) -> anyhow::Result<Vec<Copy>> {
    let r: String = db.conn().query_row("SELECT system_id || ':' || group_key FROM games WHERE id=?1", [game_id], |r| r.get(0))?;
    let mut q = db.conn().prepare("SELECT * FROM shelf_copies WHERE ref=?1 AND deleted=0 ORDER BY created_at")?;
    let v = q.query_map([r], row)?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// The key a game of this release gets: what a file named after it would group under, so its
/// files join it. Consoles grouped by folder or Switch title id can't be predicted from a name,
/// and get their own key (`shelf:`), joined to files later (`join`). A Vita release from
/// NoPayStation's list takes its title id, as Vita files do.
pub fn key_for(db: &Db, system_id: &str, release: &str, region: Option<&str>) -> anyhow::Result<(String, String, crate::parse::ParsedName)> {
    let sys = by_id(system_id).ok_or_else(|| anyhow::anyhow!("unknown console {system_id}"))?;
    let name = match region.filter(|r| !r.trim().is_empty() && !release.contains('(')) { Some(r) => format!("{release} ({})", r.trim()), None => release.to_string() };
    let p = parse(sys.parsers, &name);
    let title = p.display().to_string();
    let key = crate::grouping::release_key(&p);
    let key = match sys.group {
        GroupKind::PkgTitle => {
            let reg = p.regions.first().map(|r| match r.as_str() { "USA" => "US", "Europe" => "EU", "Japan" => "JP", "Asia" => "ASIA", x => x }.to_string()).unwrap_or_default();
            let tid: Option<String> = db.conn().query_row("SELECT id FROM titledb WHERE source='psv' AND name=?1 ORDER BY (region=?2) DESC LIMIT 1", (&p.title, &reg), |r| r.get(0)).optional()?;
            tid.map(|t| format!("tid:{t}")).unwrap_or(format!("shelf:{key}"))
        }
        GroupKind::SwitchTitle => {
            // Switch files group under the first 12 hex digits of their title id
            let tid: Option<String> = db.conn().query_row("SELECT id FROM titledb WHERE source='switch' AND name=?1 ORDER BY length(id), id LIMIT 1", [&p.title], |r| r.get(0)).optional()?;
            tid.filter(|t| t.len() >= 12).map(|t| format!("tid:{}", &t[..12].to_uppercase())).unwrap_or(format!("shelf:{key}"))
        }
        GroupKind::FolderPerGame | GroupKind::CueBin => format!("shelf:{key}"),
        _ => key,
    };
    Ok((key, title, p))
}

/// The game a copy of this release belongs to: the one already here, else a new shelf-only game.
pub fn game_for_release(db: &Db, system_id: &str, release: &str, region: Option<&str>) -> anyhow::Result<i64> {
    let (key, title, p) = key_for(db, system_id, release, region)?;
    let c = db.conn();
    if let Some(id) = c.query_row("SELECT id FROM games WHERE system_id=?1 AND group_key=?2", (system_id, &key), |r| r.get(0)).optional()? { return Ok(id); }
    // a file game verified as this release
    if let Some(id) = c.query_row("SELECT game_id FROM files WHERE system_id=?1 AND dat_status='verified' AND dat_game=?2 AND game_id IS NOT NULL LIMIT 1", (system_id, release), |r| r.get(0)).optional()? { return Ok(id); }
    c.execute("INSERT INTO games(system_id, group_key, title, parsed_json) VALUES(?1,?2,?3,?4)", (system_id, &key, &title, serde_json::to_string(&p)?))?;
    Ok(c.last_insert_rowid())
}

fn store(db: &Db, c: &Copy, ts: i64) -> anyhow::Result<()> {
    db.conn().execute(
        "INSERT INTO shelf_copies(id, ref, ref_json, system_id, release, region, edition, cart, box, manual, inserts, sealed, condition, location, price_cents, bought, notes, created_at, updated_at, deleted)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18,?19)
         ON CONFLICT(id) DO UPDATE SET ref=excluded.ref, ref_json=excluded.ref_json, system_id=excluded.system_id, release=excluded.release, region=excluded.region,
           edition=excluded.edition, cart=excluded.cart, box=excluded.box, manual=excluded.manual, inserts=excluded.inserts, sealed=excluded.sealed,
           condition=excluded.condition, location=excluded.location, price_cents=excluded.price_cents, bought=excluded.bought, notes=excluded.notes,
           updated_at=excluded.updated_at, deleted=excluded.deleted",
        rusqlite::params![c.id, c.game.local(), serde_json::to_string(&c.game)?, c.game.system, c.release, c.region, c.edition, c.cart, c.box_, c.manual, c.inserts, c.sealed,
            c.condition, c.location, c.price_cents, c.bought, c.notes, ts, c.deleted])?;
    Ok(())
}

fn with_fields(mut c: Copy, f: CopyFields) -> anyhow::Result<Copy> {
    let condition = clean(f.condition);
    if let Some(x) = &condition { if !CONDITIONS.contains(&x.as_str()) { anyhow::bail!("unknown condition {x}"); } }
    c.edition = clean(f.edition); c.cart = f.cart; c.box_ = f.box_; c.manual = f.manual; c.inserts = f.inserts; c.sealed = f.sealed;
    c.condition = condition; c.location = clean(f.location); c.price_cents = f.price_cents.filter(|p| *p >= 0);
    c.bought = clean(f.bought); c.notes = clean(f.notes);
    Ok(c)
}

/// Add a copy of a release (found in the console's list, or typed).
pub fn add(db: &Db, system_id: &str, release: &str, region: Option<&str>, f: CopyFields) -> anyhow::Result<Copy> {
    let release = release.trim();
    if release.is_empty() { anyhow::bail!("which game is it?"); }
    let gid = game_for_release(db, system_id, release, region)?;
    let game: GameRef = crate::userdata::game_ref(db, gid)?;
    let c = with_fields(Copy { id: uuid::Uuid::new_v4().to_string(), game, release: release.to_string(), region: clean(region.map(String::from)), ..Default::default() }, f)?;
    store(db, &c, crate::shared::now_ms())?;
    Ok(c)
}

/// Change what's recorded about a copy (not which release it is).
pub fn update(db: &Db, id: &str, f: CopyFields) -> anyhow::Result<Copy> {
    let c = get(db, id)?.filter(|c| !c.deleted).ok_or_else(|| anyhow::anyhow!("no such copy"))?;
    let c = with_fields(c, f)?;
    store(db, &c, crate::shared::now_ms())?;
    Ok(c)
}

/// Remove a copy (kept as removed, so other devices remove it too).
pub fn remove(db: &Db, id: &str) -> anyhow::Result<()> {
    let n = db.conn().execute("UPDATE shelf_copies SET deleted=1, updated_at=?2 WHERE id=?1", (id, crate::shared::now_ms()))?;
    if n == 0 { anyhow::bail!("no such copy"); }
    Ok(())
}

/// A copy from another device: kept when newer than ours. Its game is found, or made.
pub fn apply(db: &Db, c: &Copy, ts: i64) -> anyhow::Result<()> {
    let cur: Option<i64> = db.conn().query_row("SELECT updated_at FROM shelf_copies WHERE id=?1", [&c.id], |r| r.get(0)).optional()?;
    if cur.is_some_and(|t| t > ts) { return Ok(()); }
    let mut c = c.clone();
    // the game with exactly this key; else the one for its release (never one that only shares
    // its name: that may be another region)
    let exact: Option<i64> = db.conn().query_row("SELECT id FROM games WHERE system_id=?1 AND group_key=?2", (&c.game.system, &c.game.key), |r| r.get(0)).optional()?;
    let gid = match exact { Some(g) => g, None if !c.deleted => game_for_release(db, &c.game.system, &c.release, c.region.as_deref())?, None => 0 };
    if gid > 0 { c.game = crate::userdata::game_ref(db, gid)?; }
    store(db, &c, ts)?;
    if cur.is_none() { db.conn().execute("UPDATE shelf_copies SET created_at=?2 WHERE id=?1", (&c.id, ts))?; }
    Ok(())
}

/// Every copy, removed ones too, with when it last changed (for sync).
pub fn all(db: &Db) -> anyhow::Result<Vec<(Copy, i64)>> {
    let mut q = db.conn().prepare("SELECT * FROM shelf_copies")?;
    let v = q.query_map([], |r| Ok((row(r)?, r.get::<_, i64>("updated_at")?)))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// The places copies are kept, for suggestions.
pub fn locations(db: &Db) -> anyhow::Result<Vec<String>> {
    let mut q = db.conn().prepare("SELECT location, COUNT(*) n FROM shelf_copies WHERE deleted=0 AND location IS NOT NULL GROUP BY location ORDER BY n DESC, location")?;
    let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// Join games you own physically to the file games of the same release on a console: a file
/// verified as a copy's release, else the one file game with the same title and regions. Its
/// copies, play status, Collection entries and game info move to the file game. Returns how
/// many games were joined.
pub fn join(db: &Db, system_id: &str) -> anyhow::Result<usize> {
    let sys = by_id(system_id).ok_or_else(|| anyhow::anyhow!("unknown console {system_id}"))?;
    let c = db.conn();
    let mut q = c.prepare("SELECT g.id, g.system_id || ':' || g.group_key FROM games g WHERE g.system_id=?1
        AND NOT EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id)
        AND EXISTS (SELECT 1 FROM shelf_copies s WHERE s.deleted=0 AND s.ref = g.system_id || ':' || g.group_key)")?;
    let lonely: Vec<(i64, String)> = q.query_map([system_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    drop(q);
    if lonely.is_empty() { return Ok(0); }
    // file games by title and regions
    let mut q = c.prepare("SELECT g.id, g.title, COALESCE(json_extract(g.parsed_json,'$.regions'),'[]') FROM games g WHERE g.system_id=?1 AND EXISTS (SELECT 1 FROM files f WHERE f.game_id=g.id)")?;
    let mut by_title: std::collections::HashMap<(String, Vec<String>), Vec<i64>> = std::collections::HashMap::new();
    for r in q.query_map([system_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (id, title, regions) = r?;
        let mut regs: Vec<String> = serde_json::from_str(&regions).unwrap_or_default();
        regs.sort();
        by_title.entry((crate::completion::title_key(&title), regs)).or_default().push(id);
    }
    drop(q);
    let mut joined = 0;
    for (gid, gref) in lonely {
        let mut q = c.prepare("SELECT release FROM shelf_copies WHERE ref=?1 AND deleted=0")?;
        let releases: Vec<String> = q.query_map([&gref], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(q);
        let mut target: Option<i64> = None;
        for rel in &releases {
            target = c.query_row("SELECT game_id FROM files WHERE system_id=?1 AND dat_status='verified' AND dat_game=?2 AND game_id IS NOT NULL LIMIT 1", (system_id, rel), |r| r.get(0)).optional()?;
            if target.is_some() { break; }
            let p = parse(sys.parsers, rel);
            let mut regs = p.regions.clone();
            regs.sort();
            if let Some(v) = by_title.get(&(crate::completion::title_key(p.display()), regs.clone())).or_else(|| by_title.get(&(crate::completion::title_key(&p.title), regs))) {
                if v.len() == 1 { target = Some(v[0]); break; }
                // several (a Rev 1 beside the release): the one with the same revision and tags
                // (a "[!]" verified-good mark aside)
                let same = |g: &i64| -> bool {
                    let pj: Option<String> = c.query_row("SELECT parsed_json FROM games WHERE id=?1", [g], |r| r.get(0)).ok().flatten();
                    let Some(q) = pj.and_then(|j| serde_json::from_str::<crate::parse::ParsedName>(&j).ok()) else { return false };
                    let tags = |x: &crate::parse::ParsedName| { let mut t: Vec<String> = x.tags.iter().filter(|t| *t != "verified-good").map(|t| t.to_lowercase()).collect(); t.sort(); t };
                    q.rev == p.rev && q.version == p.version && tags(&q) == tags(&p)
                };
                let hits: Vec<i64> = v.iter().copied().filter(|g| same(g)).collect();
                if hits.len() == 1 { target = Some(hits[0]); break; }
            }
        }
        if let Some(to) = target { merge(db, gid, to)?; joined += 1; }
    }
    Ok(joined)
}

/// Move everything about game `from` (its copies, play status, Collection entries, and its game
/// info when `to` has none) to game `to`, and remove `from`.
pub fn merge(db: &Db, from: i64, to: i64) -> anyhow::Result<()> {
    let c = db.conn();
    let fr = crate::userdata::game_ref(db, from)?;
    let tr = crate::userdata::game_ref(db, to)?;
    let (fk, tk, tj) = (fr.local(), tr.local(), serde_json::to_string(&tr)?);
    let tx = c.unchecked_transaction()?;
    // moving a copy to another game isn't an edit: its time stays (a newer edit or removal made
    // elsewhere must still win)
    tx.execute("UPDATE shelf_copies SET ref=?2, ref_json=?3 WHERE ref=?1", (&fk, &tk, &tj))?;
    // play status: the newer of the two stays
    let ts = |k: &str| -> rusqlite::Result<Option<i64>> { tx.query_row("SELECT updated_at FROM play WHERE ref=?1", [k], |r| r.get(0)).optional() };
    if let Some(f) = ts(&fk)? {
        if ts(&tk)?.map_or(true, |t| f > t) {
            tx.execute("DELETE FROM play WHERE ref=?1", [&tk])?;
            tx.execute("UPDATE play SET ref=?2, ref_json=?3 WHERE ref=?1", (&fk, &tk, &tj))?;
        } else { tx.execute("DELETE FROM play WHERE ref=?1", [&fk])?; }
    }
    tx.execute("UPDATE collection_items SET ref=?2, ref_json=?3 WHERE ref=?1", (&fk, &tk, &tj))?;
    tx.execute("UPDATE games SET meta_json=(SELECT meta_json FROM games WHERE id=?1), meta_state=(SELECT meta_state FROM games WHERE id=?1), art_json=COALESCE(art_json,(SELECT art_json FROM games WHERE id=?1))
        WHERE id=?2 AND meta_state IS NULL AND (SELECT meta_state FROM games WHERE id=?1) IS NOT NULL", (from, to))?;
    tx.execute("DELETE FROM status WHERE game_id=?1", [from])?;
    tx.execute("DELETE FROM games WHERE id=?1", [from])?;
    tx.commit()?;
    Ok(())
}

/// The consoles that have copies.
pub fn systems(db: &Db) -> anyhow::Result<Vec<String>> {
    let mut q = db.conn().prepare("SELECT DISTINCT system_id FROM shelf_copies WHERE deleted=0")?;
    let v = q.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// The release a copy of a game is, when it's one from the console's list (a name the box scans
/// and lookups know). None for typed titles and games without copies.
pub fn list_release(db: &Db, game_id: i64) -> anyhow::Result<Option<String>> {
    Ok(db.conn().query_row(
        "SELECT c.release FROM shelf_copies c JOIN games g ON c.ref = g.system_id || ':' || g.group_key WHERE g.id=?1 AND c.deleted=0
           AND EXISTS (SELECT 1 FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=g.system_id AND r.game_name=c.release) ORDER BY c.created_at LIMIT 1",
        [game_id], |r| r.get(0)).optional()?)
}

/// After a copy is removed: its game goes too when it was only on the shelf and has no other copy.
pub fn drop_if_empty(db: &Db, copy_id: &str) -> anyhow::Result<()> {
    db.conn().execute("DELETE FROM games WHERE NOT EXISTS (SELECT 1 FROM files f WHERE f.game_id=games.id)
        AND NOT EXISTS (SELECT 1 FROM shelf_copies c WHERE c.deleted=0 AND c.ref = games.system_id || ':' || games.group_key)
        AND EXISTS (SELECT 1 FROM shelf_copies c WHERE c.id=?1 AND c.ref = games.system_id || ':' || games.group_key)", [copy_id])?;
    Ok(())
}
