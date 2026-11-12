//! "Unpack and compare": read every file inside an archive (streamed, nothing is written to
//! disk), fingerprint it, and for Switch images record their content pieces. The results go into
//! the archive's probe data so duplicate checks can compare the contents with other files.
use crate::config::Settings;
use crate::db::Db;
use crate::fsx::Transport;
use serde::Serialize;
use sha1::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct Inner { pub name: String, pub size: u64, pub crc: String, pub sha1: String, pub container: Option<String>, pub id: Option<String>, pub ncas: Vec<String> }

const HEAD: usize = 4 << 20;

/// Fingerprint a stream, keeping its first bytes for reading a Switch header.
fn digest(name: &str, r: &mut dyn Read, cancel: &AtomicBool, read: &AtomicU64) -> anyhow::Result<Inner> {
    let mut crc = crc32fast::Hasher::new();
    let mut sha = sha1::Sha1::new();
    let mut head = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) { anyhow::bail!("cancelled"); }
        let n = r.read(&mut buf)?;
        if n == 0 { break; }
        crc.update(&buf[..n]);
        sha.update(&buf[..n]);
        if head.len() < HEAD { head.extend_from_slice(&buf[..n.min(HEAD - head.len())]); }
        size += n as u64;
        read.fetch_add(n as u64, Ordering::Relaxed);
    }
    let img = crate::probe::image_from_head(&head);
    Ok(Inner {
        name: name.to_string(), size, crc: format!("{:08x}", crc.finalize()), sha1: sha.finalize().iter().map(|b| format!("{b:02x}")).collect(),
        container: img.as_ref().map(|i| i.0.clone()), id: img.as_ref().and_then(|i| i.1.clone()), ncas: img.map(|i| i.2).unwrap_or_default(),
    })
}

// PSP PKGs (PSN downloads) are AES-CTR encrypted with fixed, publicly known keys (the ones
// pkg2zip and NoPayStation tools use): the item table and the game data with the PSP key, the
// other items with the PS3 key.
const PKG_PSP_KEY: [u8; 16] = [0x07, 0xF2, 0xC6, 0x82, 0x90, 0xB5, 0x0D, 0x2C, 0x33, 0x81, 0x8D, 0x70, 0x9B, 0x60, 0xE6, 0x2B];
const PKG_PS3_KEY: [u8; 16] = [0x2E, 0x7B, 0x71, 0xD7, 0xC9, 0xC9, 0xA1, 0x4E, 0xA3, 0x22, 0x1F, 0x18, 0x88, 0x28, 0xB8, 0xF8];
type PkgCtr = ctr::Ctr128BE<aes::Aes128>;

/// Decrypting reader over one region of a PKG's data area.
struct PkgReader<'a> { fs: &'a dyn Transport, path: &'a Path, base: u64, pos: u64, end: u64, cipher: PkgCtr }
impl<'a> PkgReader<'a> {
    fn new(fs: &'a dyn Transport, path: &'a Path, key: &[u8; 16], riv: &[u8; 16], base: u64, start: u64, len: u64) -> Self {
        use ctr::cipher::{KeyIvInit, StreamCipherSeek};
        let mut cipher = PkgCtr::new(key.into(), riv.into());
        cipher.seek(start);
        PkgReader { fs, path, base, pos: start, end: start + len, cipher }
    }
}
impl Read for PkgReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use ctr::cipher::StreamCipher;
        if self.pos >= self.end { return Ok(0); }
        let n = (buf.len() as u64).min(self.end - self.pos).min(8 << 20) as usize;
        let mut b = self.fs.read_range(self.path, self.base + self.pos, n)?;
        if b.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "PKG ends early")); }
        self.cipher.apply_keystream(&mut b);
        buf[..b.len()].copy_from_slice(&b);
        self.pos += b.len() as u64;
        Ok(b.len())
    }
}

/// Every file inside a PSP PKG, decrypted and fingerprinted.
fn psp_pkg_digest(fs: &dyn Transport, p: &Path, cancel: &AtomicBool, read: &AtomicU64) -> anyhow::Result<Vec<Inner>> {
    let h = fs.read_range(p, 0, 0x100)?;
    if h.get(0..4) != Some(b"\x7FPKG") { anyhow::bail!("not a PKG file"); }
    if h[0xE7] & 7 != 1 { anyhow::bail!("only PSP PKGs can be opened (this one is for another console)"); }
    let be32 = |o: usize| u32::from_be_bytes(h[o..o + 4].try_into().unwrap()) as u64;
    let be64 = |o: usize| u64::from_be_bytes(h[o..o + 8].try_into().unwrap());
    let (items, data_off) = (be32(0x14), be64(0x20));
    let riv: [u8; 16] = h[0x70..0x80].try_into().unwrap();
    if items == 0 || items > 100_000 { anyhow::bail!("unexpected PKG item count"); }
    let mut table = Vec::new();
    PkgReader::new(fs, p, &PKG_PSP_KEY, &riv, data_off, 0, items * 32).read_to_end(&mut table)?;
    let mut out = Vec::new();
    for i in 0..items as usize {
        let e = &table[i * 32..i * 32 + 32];
        let g32 = |o: usize| u32::from_be_bytes(e[o..o + 4].try_into().unwrap()) as u64;
        let g64 = |o: usize| u64::from_be_bytes(e[o..o + 8].try_into().unwrap());
        let (name_off, name_len, off, size, flags) = (g32(0), g32(4), g64(8), g64(16), g32(24));
        let key = if flags & 0x1000_0000 != 0 { &PKG_PSP_KEY } else { &PKG_PS3_KEY };
        let mut name = Vec::new();
        PkgReader::new(fs, p, key, &riv, data_off, name_off, name_len.min(4096)).read_to_end(&mut name)?;
        let name = String::from_utf8_lossy(&name).trim_end_matches('\0').to_string();
        if matches!(flags & 0xFF, 4 | 18) { continue; } // folders
        out.push(digest(&name, &mut PkgReader::new(fs, p, key, &riv, data_off, off, size), cancel, read)?);
    }
    Ok(out)
}

/// The 7-Zip program: the configured one, else on PATH, else its usual Windows home.
pub fn seven_zip(s: &Settings) -> Option<String> {
    let configured = crate::actions::tool_program(s, "7z", "7z");
    for p in [configured, "7zz".into(), "7za".into(), r"C:\Program Files\7-Zip\7z.exe".into()] {
        if crate::actions::program_exists(&p) { return Some(p); }
    }
    None
}

/// Every file inside `archive`, fingerprinted. ZIPs are read directly; RAR and 7z go through 7-Zip
/// (multi-part sets are read from their first part).
pub fn unpack_digest(fs: &dyn Transport, settings: &Settings, archive: &Path, cancel: &AtomicBool, read: &AtomicU64) -> anyhow::Result<Vec<Inner>> {
    let ext = crate::util::ext_of(&archive.to_string_lossy());
    if ext == "pkg" { return psp_pkg_digest(fs, archive, cancel, read); }
    if ext == "zip" {
        let mut z = zip::ZipArchive::new(fs.open_seek(archive)?)?;
        let mut out = Vec::new();
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_dir() { continue; }
            let name = e.name().to_string();
            out.push(digest(&name, &mut e, cancel, read)?);
        }
        return Ok(out);
    }
    let prog = seven_zip(settings).ok_or_else(|| anyhow::anyhow!("needs 7-Zip to read .{ext} files — install it, or set Settings → Tools → 7z to the 7z program"))?;
    let list = Command::new(&prog).args(["l", "-slt", "-ba", "--"]).arg(archive).output()?;
    if !list.status.success() { anyhow::bail!("7-Zip could not list it: {}", String::from_utf8_lossy(&list.stderr).trim()); }
    let text = String::from_utf8_lossy(&list.stdout);
    let mut names = Vec::new();
    let (mut path, mut folder) = (None::<String>, false);
    for line in text.lines().chain(std::iter::once("")) {
        if let Some(v) = line.strip_prefix("Path = ") { path = Some(v.to_string()); }
        else if let Some(v) = line.strip_prefix("Folder = ") { folder = v.trim() == "+"; }
        else if line.trim().is_empty() {
            if let Some(p) = path.take() { if !folder { names.push(p); } }
            folder = false;
        }
    }
    if names.is_empty() { anyhow::bail!("7-Zip found nothing inside (is it password-protected?)"); }
    let mut out = Vec::new();
    for name in names {
        let mut child = Command::new(&prog).args(["e", "-so", "-y", "-p-", "--"]).arg(archive).arg(&name)
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let mut stdout = child.stdout.take().unwrap();
        let d = digest(&name, &mut stdout, cancel, read);
        if d.is_err() { let _ = child.kill(); }
        let status = child.wait()?;
        let d = d?;
        if !status.success() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() { let _ = e.read_to_string(&mut err); }
            anyhow::bail!("7-Zip could not unpack {name}: {}", err.lines().find(|l| !l.trim().is_empty()).unwrap_or("error").trim());
        }
        out.push(d);
    }
    Ok(out)
}

/// Files whose contents "Unpack and compare" can read: archives, and PSP PKGs.
pub fn can_unpack(system: &str, ext: &str) -> bool { matches!(ext, "zip" | "rar" | "7z") || (system == "psp" && ext == "pkg") }

/// Notes a release or unpacking tool adds next to the game ("x.log", "readme.txt"): not game
/// content, so they don't make a copy unique.
pub fn is_note(name: &str) -> bool {
    matches!(crate::util::ext_of(name).as_str(), "log" | "txt" | "nfo" | "diz" | "url" | "sfv" | "md5" | "sha1" | "htm" | "html")
}

/// Store an archive's unpacked fingerprints in its probe data.
pub fn save(db: &Db, file_id: i64, digests: &[Inner]) -> anyhow::Result<()> {
    let (probe, size, mtime): (Option<String>, i64, i64) = db.conn().query_row("SELECT probe_json, size, mtime FROM files WHERE id=?1", [file_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut pj: serde_json::Value = probe.as_deref().and_then(|j| serde_json::from_str(j).ok()).unwrap_or_else(|| serde_json::json!({"kind": "Archive", "id": null, "encrypted": null, "extra": {}}));
    if !pj["extra"].is_object() { pj["extra"] = serde_json::json!({}); }
    pj["extra"]["unpacked"] = serde_json::json!({
        "key": format!("{size}:{mtime}"),
        "members": digests.iter().map(|d| serde_json::json!({"name": d.name, "size": d.size, "crc": d.crc, "sha1": d.sha1, "container": d.container, "id": d.id, "ncas": d.ncas})).collect::<Vec<_>>(),
    });
    db.conn().execute("UPDATE files SET probe_json=?1 WHERE id=?2", (serde_json::to_string(&pj)?, file_id))?;
    Ok(())
}

/// Archives among `paths` that have no up-to-date unpacked fingerprints.
pub fn needs_unpack(db: &Db, file_ids: &[i64]) -> anyhow::Result<Vec<(i64, PathBuf, u64)>> {
    let mut out = Vec::new();
    let mut q = db.conn().prepare("SELECT path, ext, size, mtime, json_extract(probe_json,'$.extra.unpacked.key'), system_id FROM files WHERE id=?1")?;
    for id in file_ids {
        let Ok((path, ext, size, mtime, key, sys)) = q.query_row([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?))) else { continue };
        if !can_unpack(&sys, &ext) { continue; }
        // later parts of a set are read with the first
        if crate::util::archive_set_key(&path) != path && !path.to_lowercase().contains(".part1.") && !path.to_lowercase().contains(".part01.") { continue; }
        if key.as_deref() == Some(format!("{size}:{mtime}").as_str()) { continue; }
        out.push((*id, PathBuf::from(path), size.max(0) as u64));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctr::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};

    /// A minimal PSP PKG: items encrypted as on PSN (data with the PSP key, others with PS3's).
    fn pkg(items: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let riv = [7u8; 16];
        let n = items.len();
        let mut names = Vec::new();
        let mut name_at = Vec::new();
        for (name, _, _) in items { name_at.push((n * 32 + names.len()) as u64); names.extend_from_slice(name.as_bytes()); while names.len() % 16 != 0 { names.push(0); } }
        let mut data_at = Vec::new();
        let mut cursor = (n * 32 + names.len()) as u64;
        for (_, d, _) in items { data_at.push(cursor); cursor += ((d.len() + 15) / 16 * 16) as u64; }
        let mut area = vec![0u8; cursor as usize];
        let enc = |area: &mut Vec<u8>, key: &[u8; 16], at: u64, bytes: &[u8]| {
            let mut c = PkgCtr::new(key.into(), (&riv).into());
            c.seek(at);
            let mut b = bytes.to_vec();
            c.apply_keystream(&mut b);
            area[at as usize..at as usize + b.len()].copy_from_slice(&b);
        };
        let mut table = Vec::new();
        for (i, (name, d, psp)) in items.iter().enumerate() {
            table.extend_from_slice(&(name_at[i] as u32).to_be_bytes());
            table.extend_from_slice(&(name.len() as u32).to_be_bytes());
            table.extend_from_slice(&data_at[i].to_be_bytes());
            table.extend_from_slice(&(d.len() as u64).to_be_bytes());
            table.extend_from_slice(&(if *psp { 0x9000_0206u32 } else { 0x8000_0003 }).to_be_bytes());
            table.extend_from_slice(&[0; 4]);
        }
        enc(&mut area, &PKG_PSP_KEY, 0, &table);
        for (i, (name, d, psp)) in items.iter().enumerate() {
            let key = if *psp { &PKG_PSP_KEY } else { &PKG_PS3_KEY };
            enc(&mut area, key, name_at[i], name.as_bytes());
            enc(&mut area, key, data_at[i], d);
        }
        let mut h = vec![0u8; 0x100];
        h[0..4].copy_from_slice(b"\x7FPKG");
        h[0x14..0x18].copy_from_slice(&(n as u32).to_be_bytes());
        h[0x20..0x28].copy_from_slice(&0x100u64.to_be_bytes());
        h[0x28..0x30].copy_from_slice(&(area.len() as u64).to_be_bytes());
        h[0x70..0x80].copy_from_slice(&riv);
        h[0xE7] = 1;
        h.extend_from_slice(&area);
        h
    }

    #[test]
    fn opens_psp_pkgs() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Game.pkg");
        std::fs::write(&p, pkg(&[("PARAM.SFO", b"sfo", false), ("USRDIR/CONTENT/EBOOT.PBP", b"the game itself", true)])).unwrap();
        let d = psp_pkg_digest(&crate::fsx::LocalFs, &p, &AtomicBool::new(false), &AtomicU64::new(0)).unwrap();
        let names: Vec<&str> = d.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["PARAM.SFO", "USRDIR/CONTENT/EBOOT.PBP"]);
        let sha = |b: &[u8]| sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(d[1].sha1, sha(b"the game itself"));
        assert_eq!(d[0].sha1, sha(b"sfo"));
    }

    #[test]
    fn notes_are_not_content() {
        assert!(is_note("Crimson Gem Saga/Crimson Gem Saga.log"));
        assert!(!is_note("USRDIR/CONTENT/EBOOT.PBP"));
    }
}
