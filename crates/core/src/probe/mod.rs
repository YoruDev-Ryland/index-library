//! Header probes: a few small ranged reads per file, never whole files.
use crate::fsx::Transport;
pub use crate::systems::ProbeKind;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub kind: String,
    pub id: Option<String>,
    pub encrypted: Option<bool>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

pub fn should_probe(kind: ProbeKind, ext: &str) -> bool {
    match kind {
        ProbeKind::N3ds => matches!(ext, "3ds" | "cci" | "cia" | "cxi"),
        ProbeKind::Nds => ext == "nds",
        ProbeKind::Gba => ext == "gba",
        ProbeKind::PspPkg => ext == "pkg",
        ProbeKind::Switch => matches!(ext, "nsp" | "nsz" | "xci" | "xcz" | "rar" | "zip"),
    }
}

/// Bumped when probes learn to read more, so files read by an older version are read again.
const PROBE_VERSION: &str = ":p2";

fn u32le(b: &[u8], o: usize) -> Option<u32> { b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())) }
fn u32be(b: &[u8], o: usize) -> Option<u32> { b.get(o..o + 4).map(|x| u32::from_be_bytes(x.try_into().unwrap())) }
fn u16be(b: &[u8], o: usize) -> Option<u16> { b.get(o..o + 2).map(|x| u16::from_be_bytes(x.try_into().unwrap())) }
fn u64le(b: &[u8], o: usize) -> Option<u64> { b.get(o..o + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap())) }
fn u64be(b: &[u8], o: usize) -> Option<u64> { b.get(o..o + 8).map(|x| u64::from_be_bytes(x.try_into().unwrap())) }
fn ascii(b: &[u8], o: usize, n: usize) -> Option<String> {
    let s = b.get(o..o + n)?;
    let t: String = s.iter().take_while(|c| **c != 0).map(|c| *c as char).collect();
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_graphic() || c == ' ') { Some(t) } else { None }
}
fn align64(x: u64) -> u64 { (x + 63) & !63 }

/// Reads an NCCH header at `off`; returns (program id, encrypted).
fn ncch_at(fs: &dyn Transport, p: &Path, off: u64) -> io::Result<Option<(u64, bool)>> {
    let h = fs.read_range(p, off, 0x200)?;
    if h.get(0x100..0x104) != Some(b"NCCH") { return Ok(None); }
    let pid = u64le(&h, 0x118).unwrap_or(0);
    let nocrypto = h[0x18F] & 0x04 != 0;
    Ok(Some((pid, !nocrypto)))
}

fn probe_3ds(fs: &dyn Transport, p: &Path, pr: &mut Probe) -> io::Result<()> {
    let head = fs.read_range(p, 0, 0x200)?;
    if head.get(0x100..0x104) == Some(b"NCSD") {
        pr.extra = serde_json::json!({"container": "ncsd"});
        // media unit = 0x200 << NCSD flags[6]
        let unit = 0x200u64 << (head.get(0x188 + 6).copied().unwrap_or(0).min(8));
        let off = u32le(&head, 0x120).unwrap_or(0) as u64 * unit;
        if let Some((pid, enc)) = ncch_at(fs, p, off)? {
            pr.id = Some(format!("{pid:016X}"));
            pr.encrypted = Some(enc);
        }
        return Ok(());
    }
    if head.get(0x100..0x104) == Some(b"NCCH") {
        if let Some((pid, enc)) = ncch_at(fs, p, 0)? { pr.id = Some(format!("{pid:016X}")); pr.encrypted = Some(enc); }
        return Ok(());
    }
    // CIA
    let (Some(hdr), Some(cert), Some(tik), Some(tmd_size)) = (u32le(&head, 0), u32le(&head, 8), u32le(&head, 0xC), u32le(&head, 0x10)) else { return Ok(()) };
    if hdr != 0x2020 || tmd_size == 0 { return Ok(()); }
    pr.extra = serde_json::json!({"container": "cia"});
    let tmd_off = align64(hdr as u64) + align64(cert as u64) + align64(tik as u64);
    let content_off = tmd_off + align64(tmd_size as u64);
    let tmd = fs.read_range(p, tmd_off, (tmd_size as usize).min(0x10000))?;
    let sig_len = match u32be(&tmd, 0) {
        Some(0x10000) | Some(0x10003) => 0x200 + 0x3C,
        Some(0x10001) | Some(0x10004) => 0x100 + 0x3C,
        Some(0x10002) | Some(0x10005) => 0x3C + 0x40,
        _ => return Ok(()),
    };
    let th = 4 + sig_len;
    if let Some(tid) = u64be(&tmd, th + 0x4C) { pr.id = Some(format!("{tid:016X}")); }
    let chunk = th + 0xC4 + 0x900;
    let Some(ctype) = u16be(&tmd, chunk + 6) else { return Ok(()) };
    if ctype & 1 != 0 {
        pr.encrypted = Some(true);
        return Ok(());
    }
    if let Some((_, enc)) = ncch_at(fs, p, content_off)? { pr.encrypted = Some(enc); }
    Ok(())
}

/// Probe every file of `system` that has no probe for its current size/mtime.
/// Sets status `decrypted` on the game when encryption is known.
pub fn run_probes(db: &crate::db::Db, fs: &dyn Transport, system: &crate::systems::SystemDef, cancel: &std::sync::atomic::AtomicBool) -> anyhow::Result<u64> {
    let Some(kind) = system.probe else { return Ok(0) };
    let rows: Vec<(i64, String, String, i64, i64, Option<i64>)> = {
        let mut q = db.conn().prepare("SELECT id, path, ext, size, mtime, game_id FROM files WHERE system_id=?1 AND (probe_key IS NULL OR probe_key != size || ':' || mtime || ?2)")?;
        let r = q.query_map([system.id, PROBE_VERSION], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        r
    };
    let mut n = 0;
    for (id, path, ext, size, mtime, _game) in rows {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) { anyhow::bail!("cancelled"); }
        let key = format!("{size}:{mtime}{PROBE_VERSION}");
        if !should_probe(kind, &ext) {
            db.conn().execute("UPDATE files SET probe_key=?1 WHERE id=?2", (&key, id))?;
            continue;
        }
        let pr = match probe_file(kind, fs, Path::new(&path)) { Ok(p) => p, Err(_) => continue };
        db.conn().execute("UPDATE files SET probe_json=?1, probe_key=?2 WHERE id=?3", (serde_json::to_string(&pr)?, &key, id))?;
        n += 1;
    }
    // Derive game-level status: decrypted only if every probed file of the game is decrypted.
    db.conn().execute_batch(&format!(
        "INSERT INTO status(game_id, key, value, updated_at)
         SELECT game_id, 'decrypted',
                CASE WHEN SUM(CASE WHEN json_extract(probe_json,'$.encrypted') = 1 THEN 1 ELSE 0 END) = 0 THEN 'true' ELSE 'false' END,
                {now}
         FROM files WHERE system_id='{sid}' AND game_id IS NOT NULL AND json_extract(probe_json,'$.encrypted') IS NOT NULL
         GROUP BY game_id
         ON CONFLICT(game_id, key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at WHERE status.value != excluded.value",
        now = crate::db::now(), sid = system.id))?;
    let _ = n;
    Ok(n)
}

fn xml_tag<'a>(x: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let s = x.find(&open)? + open.len();
    let e = x[s..].find('<')?;
    Some(x[s..s + e].trim())
}
fn hex16(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    (s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit())).then(|| s.to_uppercase())
}

/// NSP/NSZ are PFS0 archives: the file table names a ticket "<title id><key gen>.tik" and/or a
/// plaintext "<x>.cnmt.xml" with type, id and version. XCI/XCZ start with a cart header whose
/// contents are encrypted, so only the format is reported for those.
fn probe_switch(fs: &dyn Transport, p: &Path, pr: &mut Probe) -> io::Result<()> {
    let head = fs.read_range(p, 0, 0x200)?;
    if let Some(list) = archive_members(fs, p, &head) {
        // an archive: what's inside, read from the inner files where they're stored uncompressed
        let mut ids: Vec<String> = Vec::new();
        let mut members = Vec::new();
        for m in &list {
            let inner = m.stored_at.and_then(|at| read_image(fs, p, at).ok().flatten());
            let named = crate::parse::parse(&[crate::systems::ParserKind::Switch], &crate::util::full_stem(m.name.rsplit(['/', '\\']).next().unwrap_or(""))).title_id;
            let id = inner.as_ref().and_then(|i| i.id.clone()).or(named);
            if let Some(i) = &id { ids.push(i.clone()); }
            let mut j = serde_json::json!({"name": m.name, "size": m.size, "crc": m.crc, "id": id});
            if let Some(i) = inner {
                j["container"] = i.container.into();
                if let Some(v) = i.version { j["version"] = v.into(); }
                if !i.ncas.is_empty() { j["ncas"] = i.ncas.into(); }
            }
            members.push(j);
        }
        ids.sort(); ids.dedup();
        pr.extra = serde_json::json!({"container": list.first().map(|m| m.kind).unwrap_or("archive"), "members": members});
        if ids.len() == 1 {
            pr.id = ids.pop();
            if let Some(v) = pr.extra["members"][0]["version"].as_str().map(|s| s.to_string()) { if list.len() == 1 { pr.extra["version"] = v.into(); } }
        } else if !ids.is_empty() { pr.extra["ids"] = ids.into(); }
        return Ok(());
    }
    let Some(img) = read_image(fs, p, 0)? else { return Ok(()) };
    pr.id = img.id.clone();
    let mut extra = serde_json::json!({"container": img.container});
    if let Some(v) = img.version { extra["version"] = v.into(); }
    if let Some(t) = img.ty { extra["type"] = t.into(); }
    if !img.ncas.is_empty() { extra["ncas"] = img.ncas.into(); }
    if img.tickets.len() > 1 { extra["ids"] = img.tickets.into(); }
    pr.extra = extra;
    Ok(())
}

/// What a Switch image (NSP or XCI) at `base` in a file holds. XCI content is encrypted, so for
/// those only the content pieces (NCA names, which are content hashes) are known.
struct Image { container: &'static str, id: Option<String>, version: Option<String>, ty: Option<String>, ncas: Vec<String>, tickets: Vec<String> }

fn read_image(fs: &dyn Transport, p: &Path, base: u64) -> io::Result<Option<Image>> {
    read_image_at(&|o, l| fs.read_range(p, o, l), base)
}

/// Bytes at an offset: a file on disk, or the start of a stream held in memory.
type At<'a> = &'a dyn Fn(u64, usize) -> io::Result<Vec<u8>>;

/// Content pieces (NCA ids) and content id of an NSP/XCI whose first bytes are `head`.
pub fn image_from_head(head: &[u8]) -> Option<(String, Option<String>, Vec<String>)> {
    let at = |o: u64, l: usize| -> io::Result<Vec<u8>> { let o = o as usize; Ok(head.get(o..(o + l).min(head.len())).map(|b| b.to_vec()).unwrap_or_default()) };
    read_image_at(&at, 0).ok().flatten().map(|i| (i.container.to_string(), i.id, i.ncas))
}

fn read_image_at(read: At, base: u64) -> io::Result<Option<Image>> {
    let head = read(base, 0x200)?;
    if head.get(0x100..0x104) == Some(b"HEAD") {
        let mut img = Image { container: "xci", id: None, version: None, ty: None, ncas: vec![], tickets: vec![] };
        // the cart's file tables are plain: the content pieces sit in the "secure" partition
        if let Some(root) = u64le(&head, 0x130) {
            if let Ok(parts) = hfs0_at(read, base + root) {
                if let Some((_, off, _)) = parts.iter().find(|(n, _, _)| n == "secure") {
                    if let Ok(files) = hfs0_at(read, *off) {
                        img.ncas = files.iter().filter_map(|(n, _, _)| n.to_lowercase().strip_suffix(".nca").map(|s| s.to_string())).collect();
                        img.ncas.sort();
                    }
                }
            }
        }
        return Ok(Some(img));
    }
    if head.get(0..4) != Some(b"PFS0") { return Ok(None); }
    let (Some(n), Some(strsz)) = (u32le(&head, 4), u32le(&head, 8)) else { return Ok(None) };
    if n == 0 || n > 4096 || strsz > 0x10_0000 { return Ok(None); }
    let table_len = 16 + n as usize * 24 + strsz as usize;
    let table = if table_len <= head.len() { head.clone() } else { read(base, table_len)? };
    if table.len() < table_len { return Ok(None); }
    let strs = &table[16 + n as usize * 24..table_len];
    let mut img = Image { container: "nsp", id: None, version: None, ty: None, ncas: vec![], tickets: vec![] };
    let mut cnmt_xml = None;
    for i in 0..n as usize {
        let e = 16 + i * 24;
        let (Some(off), Some(size), Some(so)) = (u64le(&table, e), u64le(&table, e + 8), u32le(&table, e + 16)) else { continue };
        let Some(rest) = strs.get(so as usize..) else { continue };
        let name = String::from_utf8_lossy(&rest[..rest.iter().position(|c| *c == 0).unwrap_or(rest.len())]).to_lowercase();
        if let Some(stem) = name.strip_suffix(".nca").or(name.strip_suffix(".ncz")) { img.ncas.push(stem.to_string()); }
        if let Some(stem) = name.strip_suffix(".tik") { if let Some(t) = stem.get(..16).and_then(hex16) { img.tickets.push(t); } }
        if name.ends_with(".cnmt.xml") && cnmt_xml.is_none() { cnmt_xml = Some((base + table_len as u64 + off, size.min(0x8000) as usize)); }
    }
    if let Some((off, len)) = cnmt_xml {
        let x = read(off, len)?;
        let x = String::from_utf8_lossy(&x);
        if let Some(id) = xml_tag(&x, "Id").and_then(hex16) { img.id = Some(id); }
        img.version = xml_tag(&x, "Version").filter(|v| v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty()).map(|v| v.to_string());
        img.ty = xml_tag(&x, "Type").map(|t| t.to_string());
    }
    img.ncas.sort();
    img.tickets.sort();
    img.tickets.dedup();
    if img.id.is_none() { img.id = img.tickets.first().cloned(); }
    Ok(Some(img))
}

/// Entries of an HFS0 table (an XCI partition): (name, absolute data offset, size).
fn hfs0_at(read: At, at: u64) -> io::Result<Vec<(String, u64, u64)>> {
    let d = read(at, 0x8000)?;
    if d.get(0..4) != Some(b"HFS0") { return Err(io::Error::new(io::ErrorKind::InvalidData, "not hfs0")); }
    let (Some(n), Some(st)) = (u32le(&d, 4), u32le(&d, 8)) else { return Ok(vec![]) };
    let (n, st) = (n as usize, st as usize);
    if n > 1024 || 16 + n * 0x40 + st > d.len() { return Ok(vec![]); }
    let strs = &d[16 + n * 0x40..16 + n * 0x40 + st];
    let data = at + (16 + n * 0x40 + st) as u64;
    let mut out = Vec::new();
    for i in 0..n {
        let e = 16 + i * 0x40;
        let (Some(o), Some(sz), Some(so)) = (u64le(&d, e), u64le(&d, e + 8), u32le(&d, e + 16)) else { continue };
        let Some(rest) = strs.get(so as usize..) else { continue };
        out.push((String::from_utf8_lossy(&rest[..rest.iter().position(|c| *c == 0).unwrap_or(rest.len())]).into_owned(), data + o, sz));
    }
    Ok(out)
}

/// `stored_at`: where the member's bytes start when it's stored uncompressed (readable in place).
pub struct Member { pub kind: &'static str, pub name: String, pub size: u64, pub crc: Option<String>, pub stored_at: Option<u64> }

/// The file list of a RAR5 or ZIP archive, read from its headers (nothing is unpacked).
pub fn archive_members(fs: &dyn Transport, p: &Path, head: &[u8]) -> Option<Vec<Member>> {
    if head.starts_with(b"Rar!\x1a\x07\x01\x00") { return rar5_members(fs, p).ok(); }
    if head.starts_with(b"PK\x03\x04") {
        let r = fs.open_seek(p).ok()?;
        let z = zip::ZipArchive::new(r).ok()?;
        return Some((0..z.len()).filter_map(|i| z.name_for_index(i).map(|n| n.to_string()))
            .filter(|n| !n.ends_with('/'))
            .map(|name| Member { kind: "zip", name, size: 0, crc: None, stored_at: None }).collect());
    }
    None
}

fn vint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let c = *b.get(*at)?;
        *at += 1;
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 { return Some(v); }
    }
    None
}

fn rar5_members(fs: &dyn Transport, p: &Path) -> io::Result<Vec<Member>> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "rar header");
    let total = fs.stat(p)?.size;
    let mut pos = 8u64;
    let mut out = Vec::new();
    for _ in 0..4096 {
        if pos + 7 > total { break; }
        let b = fs.read_range(p, pos, 0x1000)?;
        let mut i = 4;
        let hsize = vint(&b, &mut i).ok_or_else(bad)?;
        let hstart = i;
        if hstart + hsize as usize > b.len() { break; }
        let htype = vint(&b, &mut i).ok_or_else(bad)?;
        let hflags = vint(&b, &mut i).ok_or_else(bad)?;
        if hflags & 1 != 0 { vint(&b, &mut i).ok_or_else(bad)?; }
        let dsize = if hflags & 2 != 0 { vint(&b, &mut i).ok_or_else(bad)? } else { 0 };
        match htype {
            2 => {
                let fflags = vint(&b, &mut i).ok_or_else(bad)?;
                let usize_ = vint(&b, &mut i).ok_or_else(bad)?;
                vint(&b, &mut i).ok_or_else(bad)?; // attributes
                if fflags & 2 != 0 { i += 4; }
                let crc = if fflags & 4 != 0 { u32le(&b, i).map(|c| format!("{c:08x}")) } else { None };
                if fflags & 4 != 0 { i += 4; }
                let comp = vint(&b, &mut i).ok_or_else(bad)?;
                vint(&b, &mut i).ok_or_else(bad)?; // host os
                let nlen = vint(&b, &mut i).ok_or_else(bad)? as usize;
                let name = String::from_utf8_lossy(b.get(i..i + nlen).ok_or_else(bad)?).into_owned();
                // directories, and the continuation of a file split across volumes, aren't new members
                // method 0 = stored: the file's bytes follow the header unchanged
                let stored_at = ((comp >> 7) & 7 == 0).then(|| pos + hstart as u64 + hsize);
                if fflags & 1 == 0 && hflags & 0x8 == 0 { out.push(Member { kind: "rar", name, size: usize_, crc, stored_at }); }
            }
            4 => break, // encrypted headers
            5 => break, // end of archive
            _ => {}
        }
        pos += (hstart as u64) + hsize + dsize;
    }
    Ok(out)
}

pub fn probe_file(kind: ProbeKind, fs: &dyn Transport, path: &Path) -> io::Result<Probe> {
    let mut pr = Probe { kind: format!("{kind:?}"), ..Default::default() };
    match kind {
        ProbeKind::N3ds => probe_3ds(fs, path, &mut pr)?,
        ProbeKind::Nds => { let h = fs.read_range(path, 0, 0x10)?; pr.id = ascii(&h, 0x0C, 4); }
        ProbeKind::Gba => { let h = fs.read_range(path, 0, 0xB0)?; pr.id = ascii(&h, 0xAC, 4); }
        ProbeKind::PspPkg => {
            let h = fs.read_range(path, 0, 0x60)?;
            if h.get(0..4) == Some(b"\x7FPKG") { pr.id = ascii(&h, 0x30, 36); }
        }
        ProbeKind::Switch => probe_switch(fs, path, &mut pr)?,
    }
    Ok(pr)
}

#[doc(hidden)]
pub mod testdata;
#[doc(hidden)]
pub fn testdata_cia(tid: u64, cia_enc: bool, nocrypto: bool) -> Vec<u8> { testdata::cia(tid, cia_enc, nocrypto) }
#[doc(hidden)]
pub fn testdata_ncsd(pid: u64, nocrypto: bool) -> Vec<u8> { testdata::ncsd(pid, nocrypto) }
#[cfg(test)]
mod tests {
    use super::*;
    use super::testdata::*;
    use crate::fsx::LocalFs;
    fn probe_bytes(kind: ProbeKind, name: &str, data: &[u8]) -> Probe {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(name);
        std::fs::write(&p, data).unwrap();
        probe_file(kind, &LocalFs, &p).unwrap()
    }
    #[test] fn ncsd_encrypted_and_not() {
        let e = probe_bytes(ProbeKind::N3ds, "a.3ds", &ncsd(0x0004000000055D00, false));
        assert_eq!(e.encrypted, Some(true));
        assert_eq!(e.id.as_deref(), Some("0004000000055D00"));
        let d = probe_bytes(ProbeKind::N3ds, "a.3ds", &ncsd(0x0004000000055D00, true));
        assert_eq!(d.encrypted, Some(false));
    }
    #[test] fn cia_variants() {
        let a = probe_bytes(ProbeKind::N3ds, "a.cia", &cia(0x0004000000030800, false, true));
        assert_eq!(a.encrypted, Some(false));
        assert_eq!(a.id.as_deref(), Some("0004000000030800"));
        let b = probe_bytes(ProbeKind::N3ds, "a.cia", &cia(0x0004000000030800, false, false));
        assert_eq!(b.encrypted, Some(true));
        let c = probe_bytes(ProbeKind::N3ds, "a.cia", &cia(0x0004000000030800, true, true));
        assert_eq!(c.encrypted, Some(true), "CIA-layer encryption counts");
    }
    #[test] fn nds_gba_pkg() {
        let mut nds = vec![0u8; 0x200]; nds[0x0C..0x10].copy_from_slice(b"AMCE");
        assert_eq!(probe_bytes(ProbeKind::Nds, "a.nds", &nds).id.as_deref(), Some("AMCE"));
        let mut gba = vec![0u8; 0x200]; gba[0xAC..0xB0].copy_from_slice(b"BPRE");
        assert_eq!(probe_bytes(ProbeKind::Gba, "a.gba", &gba).id.as_deref(), Some("BPRE"));
        let mut pkg = vec![0u8; 0x100]; pkg[0..4].copy_from_slice(b"\x7FPKG");
        pkg[0x30..0x30 + 36].copy_from_slice(b"UP0555-NPUF30007_00-BOMBERMAN940EH01");
        assert_eq!(probe_bytes(ProbeKind::PspPkg, "a.pkg", &pkg).id.as_deref(), Some("UP0555-NPUF30007_00-BOMBERMAN940EH01"));
    }
    #[test] fn garbage_is_not_a_panic() {
        let p = probe_bytes(ProbeKind::N3ds, "a.cia", b"short");
        assert_eq!(p.encrypted, None);
        let p = probe_bytes(ProbeKind::N3ds, "a.3ds", &vec![0xFFu8; 0x400]);
        assert_eq!(p.encrypted, None);
    }
    #[test] fn zip_skipped() {
        assert!(should_probe(ProbeKind::Nds, "zip") == false);
        assert!(should_probe(ProbeKind::Nds, "nds"));
    }
}
