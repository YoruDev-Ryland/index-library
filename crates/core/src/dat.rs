//! Logiqx XML DAT import (No-Intro / Redump) and verification.
use crate::db::Db;
use crate::util::stem_of;
use quick_xml::events::Event;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct DatSummary { pub name: String, pub version: String, pub games: u64, pub roms: u64 }

#[derive(Debug, Clone, Default, Serialize)]
pub struct VerifyReport { pub verified: u64, pub bad: u64, pub unknown: u64 }

struct Rom { game: String, name: String, size: Option<i64>, crc: Option<String>, sha1: Option<String>, md5: Option<String> }

/// Replaces any DAT previously imported for `system_id`.
/// ClrMamePro text DATs (libretro-database's format): `game ( name "…" rom ( name "…" size N crc … ) )`.
fn parse_clrmamepro(text: &str) -> (String, String, u64, Vec<Rom>) {
    // tokens: ( ) "quoted" bare
    let mut toks: Vec<String> = Vec::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '(' | ')' => toks.push(c.to_string()),
            '"' => { let mut w = String::new(); for d in it.by_ref() { if d == '"' { break; } w.push(d); } toks.push(format!("\"{w}")); }
            c if c.is_whitespace() => {}
            c => { let mut w = c.to_string(); while let Some(&d) = it.peek() { if d.is_whitespace() || d == '(' || d == ')' { break; } w.push(d); it.next(); } toks.push(w); }
        }
    }
    let val = |t: &str| t.strip_prefix('"').unwrap_or(t).to_string();
    let (mut name, mut version, mut games) = (String::new(), String::new(), 0u64);
    let mut roms = Vec::new();
    let mut i = 0;
    // (block kind, game name) per open parenthesis
    let mut stack: Vec<(String, Option<String>)> = Vec::new();
    let mut last_word = String::new();
    while i < toks.len() {
        let t = &toks[i];
        if t == "(" { stack.push((last_word.clone(), None)); if last_word == "game" || last_word == "machine" { games += 1; } i += 1; continue; }
        if t == ")" { stack.pop(); i += 1; continue; }
        let kind = stack.last().map(|x| x.0.clone()).unwrap_or_default();
        if kind == "rom" {
            // rom ( name "x" size 1 crc ab md5 cd sha1 ef … ) — read pairs until ")"
            let game = stack.iter().rev().find_map(|x| x.1.clone()).unwrap_or_default();
            let mut fields = std::collections::HashMap::new();
            while i + 1 < toks.len() && toks[i] != ")" { fields.insert(toks[i].clone(), val(&toks[i + 1])); i += 2; }
            roms.push(Rom { game, name: fields.get("name").cloned().unwrap_or_default(), size: fields.get("size").and_then(|x| x.parse().ok()),
                crc: fields.get("crc").map(|x| x.to_lowercase()), sha1: fields.get("sha1").map(|x| x.to_lowercase()), md5: fields.get("md5").map(|x| x.to_lowercase()) });
            continue;
        }
        if i + 1 < toks.len() && toks[i + 1] != "(" && toks[i + 1] != ")" {
            let v = val(&toks[i + 1]);
            match (kind.as_str(), t.as_str()) {
                ("clrmamepro", "name") => name = v,
                ("clrmamepro", "version") => version = v,
                ("game" | "machine", "name") => { if let Some(top) = stack.last_mut() { top.1 = Some(v); } }
                _ => {}
            }
            i += 2;
            continue;
        }
        last_word = t.clone();
        i += 1;
    }
    (name, version, games, roms)
}

/// The No-Intro / Redump list for a system in libretro-database (open, no key).
pub fn libretro_dat(system_id: &str) -> Option<&'static str> {
    Some(match system_id {
        "atari2600" => "no-intro/Atari - 2600", "atari5200" => "no-intro/Atari - 5200", "atari7800" => "no-intro/Atari - 7800",
        "lynx" => "no-intro/Atari - Lynx", "ngpc" => "no-intro/SNK - Neo Geo Pocket Color",
        "n3ds" => "no-intro/Nintendo - Nintendo 3DS", "nds" => "no-intro/Nintendo - Nintendo DS",
        "gb" => "no-intro/Nintendo - Game Boy", "gba" | "gbahacks" => "no-intro/Nintendo - Game Boy Advance", "gbc" => "no-intro/Nintendo - Game Boy Color",
        "gamecube" => "redump/Nintendo - GameCube", "nes" => "no-intro/Nintendo - Nintendo Entertainment System",
        "snes" => "no-intro/Nintendo - Super Nintendo Entertainment System", "virtualboy" => "no-intro/Nintendo - Virtual Boy",
        "pce" => "no-intro/NEC - PC Engine - TurboGrafx 16", "sega32x" => "no-intro/Sega - 32X", "gamegear" => "no-intro/Sega - Game Gear",
        "genesis" => "no-intro/Sega - Mega Drive - Genesis", "sms" => "no-intro/Sega - Master System - Mark III", "sg1000" => "no-intro/Sega - SG-1000",
        "ps1" => "redump/Sony - PlayStation", "psp" => "redump/Sony - PlayStation Portable", "psvita" => "no-intro/Sony - PlayStation Vita",
        "vectrex" => "no-intro/GCE - Vectrex",
        _ => return None,
    })
}

/// FBNeo's arcade list (libretro-database): the title of every arcade set, by set name.
pub const ARCADE_LIST: &str = "fbneo-split/FBNeo - Arcade Games";

/// Each set in an arcade list: its name ("aof", from aof.zip) and its title ("Art of Fighting /
/// Ryuuko no Ken (NGM-044)(NGH-044)").
pub fn arcade_sets(path: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let (_, _, _, roms) = parse_clrmamepro(&String::from_utf8_lossy(&std::fs::read(path)?));
    Ok(roms.into_iter().filter_map(|r| {
        let set = r.name.strip_suffix(".zip").or(r.name.strip_suffix(".7z"))?.to_string();
        (!r.game.is_empty()).then_some((set, r.game))
    }).collect())
}

pub fn libretro_url(rel: &str) -> String {
    let enc: String = rel.bytes().map(|b| match b { b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' => (b as char).to_string(), _ => format!("%{b:02X}") }).collect();
    format!("https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/{enc}.dat")
}

pub fn import_dat(db: &Db, system_id: &str, path: &Path) -> anyhow::Result<DatSummary> {
    let head = std::fs::read(path)?;
    if !head.iter().find(|b| !b.is_ascii_whitespace()).map(|b| *b == b'<').unwrap_or(false) {
        let (name, version, games, roms) = parse_clrmamepro(&String::from_utf8_lossy(&head));
        if roms.is_empty() { anyhow::bail!("no games found in {}", path.display()); }
        return store_dat(db, system_id, name, version, games, roms);
    }
    let mut r = quick_xml::Reader::from_file(path)?;
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let (mut name, mut version) = (String::new(), String::new());
    let mut stack: Vec<String> = Vec::new();
    let mut game: Option<String> = None;
    let mut roms: Vec<Rom> = Vec::new();
    let mut games = 0u64;
    loop {
        match r.read_event_into(&mut buf)? {
            Event::Start(e) | Event::Empty(e) if matches!(e.name().as_ref(), b"game" | b"machine" | b"rom") => {
                let mut attrs = std::collections::HashMap::new();
                for a in e.attributes().flatten() {
                    attrs.insert(String::from_utf8_lossy(a.key.as_ref()).into_owned(), a.unescape_value()?.into_owned());
                }
                if e.name().as_ref() == b"rom" {
                    if let Some(g) = &game {
                        roms.push(Rom {
                            game: g.clone(), name: attrs.get("name").cloned().unwrap_or_default(),
                            size: attrs.get("size").and_then(|s| s.parse().ok()),
                            crc: attrs.get("crc").map(|s| s.to_lowercase()), sha1: attrs.get("sha1").map(|s| s.to_lowercase()), md5: attrs.get("md5").map(|s| s.to_lowercase()),
                        });
                    }
                } else {
                    game = attrs.get("name").cloned();
                    games += 1;
                }
            }
            Event::End(e) if matches!(e.name().as_ref(), b"game" | b"machine") => game = None,
            Event::Start(e) => stack.push(String::from_utf8_lossy(e.name().as_ref()).into_owned()),
            Event::End(_) => { stack.pop(); }
            Event::Text(t) => {
                let s = t.unescape()?.into_owned();
                if stack.len() >= 2 && stack[stack.len() - 2] == "header" {
                    match stack.last().map(|s| s.as_str()) { Some("name") => name = s, Some("version") => version = s, _ => {} }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    store_dat(db, system_id, name, version, games, roms)
}

fn store_dat(db: &Db, system_id: &str, name: String, version: String, games: u64, roms: Vec<Rom>) -> anyhow::Result<DatSummary> {
    let tx = db.conn().unchecked_transaction()?;
    tx.execute("DELETE FROM dat_roms WHERE dat_id IN (SELECT id FROM dats WHERE system_id=?1)", [system_id])?;
    tx.execute("DELETE FROM dats WHERE system_id=?1", [system_id])?;
    tx.execute("INSERT INTO dats(system_id, name, version) VALUES(?1,?2,?3)", (system_id, &name, &version))?;
    let dat_id = tx.last_insert_rowid();
    {
        let mut ins = tx.prepare("INSERT INTO dat_roms(dat_id, game_name, rom_name, size, crc32, sha1, md5) VALUES(?1,?2,?3,?4,?5,?6,?7)")?;
        for r in &roms { ins.execute((dat_id, &r.game, &r.name, r.size, &r.crc, &r.sha1, &r.md5))?; }
    }
    tx.commit()?;
    Ok(DatSummary { name, version, games, roms: roms.len() as u64 })
}

/// Marks every hashed file of the system verified / bad / unknown.
pub fn verify(db: &Db, system_id: &str) -> anyhow::Result<VerifyReport> {
    let c = db.conn();
    let tx = c.unchecked_transaction()?;
    type Row = (i64, String, String, i64, String, String, Option<i64>, Option<String>, Option<String>);
    let files: Vec<Row> = {
        let mut q = tx.prepare("SELECT id, name, ext, size, crc32, sha1, game_id, headerless_crc32, headerless_sha1 FROM files WHERE system_id=?1 AND sha1 IS NOT NULL")?;
        let v = q.query_map([system_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?.collect::<Result<_, _>>()?;
        v
    };
    let mut rep = VerifyReport::default();
    let mut by_sha = tx.prepare("SELECT game_name, rom_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1 AND r.sha1=?2 LIMIT 1")?;
    let mut by_crc = tx.prepare("SELECT game_name, size, rom_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1 AND r.crc32=?2")?;
    let mut by_name = tx.prepare("SELECT 1 FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1 AND (lower(r.game_name)=lower(?2) OR lower(r.rom_name)=lower(?3)) LIMIT 1")?;
    for (id, name, ext, size, crc, sha1, game, hcrc, hsha) in files {
        // the whole file, then the ROM data without its header (NES, SNES, 7800, Lynx)
        let mut hit: Option<(String, String)> = [Some(&sha1), hsha.as_ref()].into_iter().flatten()
            .find_map(|h| by_sha.query_row((system_id, h), |r| Ok((r.get(0)?, r.get(1)?))).ok());
        if hit.is_none() {
            // CRC fallback; size check only for raw files (zip size != rom size)
            for c in [Some(&crc), hcrc.as_ref()].into_iter().flatten() {
                let rows: Vec<(String, Option<i64>, String)> = by_crc.query_map((system_id, c), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
                hit = rows.into_iter().find(|(_, s, _)| ext == "zip" || hcrc.is_some() || s.map(|s| s == size).unwrap_or(true)).map(|(g, _, r)| (g, r));
                if hit.is_some() { break; }
            }
        }
        let (status, dat_game, dat_rom) = match hit {
            Some((g, r)) => { rep.verified += 1; ("verified", Some(g), Some(r)) }
            None if by_name.exists((system_id, stem_of(&name), &name))? => { rep.bad += 1; ("bad", None, None) }
            None => { rep.unknown += 1; ("unknown", None, None) }
        };
        tx.execute("UPDATE files SET dat_status=?1, dat_game=?2, dat_rom=?3 WHERE id=?4", (status, &dat_game, &dat_rom, id))?;
        if let (Some(g), Some(gid)) = (&dat_game, game) {
            tx.execute("UPDATE games SET canonical_id=?1 WHERE id=?2", (g, gid))?;
        }
    }
    drop((by_sha, by_crc, by_name));
    tx.commit()?;
    Ok(rep)
}

/// DAT games with no verified file in the library, sorted.
pub fn missing(db: &Db, system_id: &str) -> anyhow::Result<Vec<String>> {
    let mut q = db.conn().prepare(
        "SELECT DISTINCT r.game_name FROM dat_roms r JOIN dats d ON d.id=r.dat_id WHERE d.system_id=?1
         AND r.game_name NOT IN (SELECT dat_game FROM files WHERE system_id=?1 AND dat_status='verified' AND dat_game IS NOT NULL)
         ORDER BY r.game_name")?;
    let v = q.query_map([system_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

#[cfg(test)]
mod cmp_tests {
    #[test]
    fn reads_clrmamepro_text() {
        let t = r#"clrmamepro ( name "Nintendo - Game Boy Advance" version "2026" )
game ( name "Metroid Fusion (USA)" region "USA" serial "AMTE"
	rom ( name "Metroid Fusion (USA).gba" size 8388608 crc 6C5404A1 md5 AF5040FC sha1 5DE8536A serial "AMTE" ) )
game ( name "Zelda (Europe)" rom ( name "Zelda (Europe).gba" size 4 crc 0BAD ) )"#;
        let (name, version, games, roms) = super::parse_clrmamepro(t);
        assert_eq!((name.as_str(), version.as_str(), games, roms.len()), ("Nintendo - Game Boy Advance", "2026", 2, 2));
        assert_eq!((roms[0].game.as_str(), roms[0].name.as_str(), roms[0].crc.as_deref(), roms[0].size), ("Metroid Fusion (USA)", "Metroid Fusion (USA).gba", Some("6c5404a1"), Some(8388608)));
        assert_eq!(roms[1].game, "Zelda (Europe)");
    }
}
