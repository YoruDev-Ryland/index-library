use std::path::{Path, PathBuf, MAIN_SEPARATOR};

pub fn p2s(p: &Path) -> String { p.to_string_lossy().into_owned() }

/// "path + separator", used for prefix matching a subtree in SQL.
pub fn subtree_prefix(p: &Path) -> String {
    let mut s = p2s(p);
    if !s.ends_with(MAIN_SEPARATOR) { s.push(MAIN_SEPARATOR); }
    s
}

/// Lowercased extension; handles the double extension "nkit.iso".
pub fn ext_of(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.ends_with(".nkit.iso") { return "nkit.iso".into(); }
    Path::new(&lower).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Extension as it should survive a rename, original case: "nsp.part1.rar", "nsp.rar", or plain "nsp".
pub fn full_ext(name: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(
        r"(?i)\.((?:(?:nsp|xci|nsz|xcz|iso|cso|bin|cue|chd|rvz|gcm|wbfs|3ds|cia|pkg|vpk)\.)?(?:part\d+\.)?(?:rar|zip|7z))$").unwrap());
    if let Some(c) = RE.captures(name) { if c.get(0).unwrap().start() > 0 { return c[1].to_string(); } }
    let e = ext_of(name);
    if e.is_empty() { String::new() } else { name[name.len() - e.len()..].to_string() }
}

/// Name without `full_ext`: "Game [x].nsp.part1.rar" → "Game [x]".
pub fn full_stem(name: &str) -> String {
    let e = full_ext(name);
    if e.is_empty() { name.to_string() } else { name[..name.len() - e.len() - 1].to_string() }
}

/// "a.part2.rar" and "a.part1.rar" belong to one archive set: both give "a.rar".
pub fn archive_set_key(path: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(?i)\.part\d+(\.rar)$").unwrap());
    RE.replace(path, "$1").into_owned()
}

/// Switch title id → its application (base) id: updates are base|0x800, DLC base+0x1000+n.
pub fn switch_base_id(tid: &str) -> Option<u64> {
    let v = u64::from_str_radix(tid, 16).ok()?;
    (tid.len() == 16).then_some(v & !0x1FFF)
}

/// What a file is to its game: "game" (or a game's base), "disc", "update" or "dlc", a short
/// label for it, and a sort order. Switch files are read from their title id (…000 base,
/// …800 update, …1xxx DLC); everything else from tags in the name.
pub fn file_role(system: &str, name: &str, probe_id: Option<&str>) -> (&'static str, String, u32) {
    let hex16 = |t: &str| t.len() == 16 && t.chars().all(|c| c.is_ascii_hexdigit());
    let tid = probe_id.filter(|t| hex16(t)).map(str::to_string)
        .or_else(|| name.split(|c: char| !c.is_ascii_hexdigit()).find(|t| hex16(t)).map(str::to_string));
    if system == "switch" {
        if let Some(low) = tid.and_then(|t| u64::from_str_radix(&t, 16).ok()).map(|v| v & 0x1FFF) {
            if low == 0 { return ("game", "Base game".into(), 0); }
            if low == 0x800 { return ("update", "Update".into(), 100); }
            if low & 0x1000 != 0 { return ("dlc", format!("DLC {}", low & 0xFFF), 200 + (low & 0xFFF) as u32); }
        }
    }
    let l = name.to_lowercase();
    let tagged = |words: &[&str]| words.iter().any(|w| l.contains(&format!("({w}")) || l.contains(&format!("[{w}")));
    if tagged(&["update", "upd", "patch"]) { return ("update", "Update".into(), 100); }
    if tagged(&["dlc", "add-on", "addon"]) { return ("dlc", "DLC".into(), 200); }
    if let Some(i) = l.find("(disc ") {
        let n: String = l[i + 6..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = n.parse::<u32>() { return ("disc", format!("Disc {n}"), n); }
    }
    ("game", "Game".into(), 0)
}

pub fn stem_of(name: &str) -> String {
    let ext = ext_of(name);
    if ext.is_empty() { return name.to_string(); }
    name[..name.len() - ext.len() - 1].to_string()
}

pub fn with_name(p: &Path, name: &str) -> PathBuf {
    p.parent().map(|d| d.join(name)).unwrap_or_else(|| PathBuf::from(name))
}

pub fn now_nanos() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exts() {
        assert_eq!(ext_of("A (USA).ZIP"), "zip");
        assert_eq!(ext_of("g.nkit.iso"), "nkit.iso");
        assert_eq!(stem_of("g.nkit.iso"), "g");
        assert_eq!(stem_of("Mario Kart 7 (USA)-decrypted.cia"), "Mario Kart 7 (USA)-decrypted");
        assert_eq!(stem_of("noext"), "noext");
    }
}

#[cfg(test)]
mod full_ext_tests {
    use super::*;
    #[test] fn compound() {
        assert_eq!(full_ext("A [x][US].nsp.part1.rar"), "nsp.part1.rar");
        assert_eq!(full_ext("A.NSP.rar"), "NSP.rar");
        assert_eq!(full_ext("A.part3.rar"), "part3.rar");
        assert_eq!(full_ext("Firmware.22.5.0.zip"), "zip");
        assert_eq!(full_ext("A.nsp"), "nsp");
        assert_eq!(full_stem("A [x].nsp.part1.rar"), "A [x]");
        assert_eq!(archive_set_key("/d/A.nsp.part2.rar"), "/d/A.nsp.rar");
        assert_eq!(switch_base_id("0100ABF008969002"), Some(0x0100ABF008968000));
        assert_eq!(switch_base_id("0100849000BDA800"), Some(0x0100849000BDA000));
    }
}

/// file:// URI for a local path (percent-encoded, keeping '/').
pub fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy();
    let enc: String = s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect();
    format!("file://{enc}")
}

/// Show a file selected in the system's file manager (or open a folder when `p` is one).
pub fn reveal(p: &Path) -> anyhow::Result<()> {
    use std::process::Command;
    let is_dir = p.is_dir();
    #[cfg(target_os = "windows")]
    {
        let arg = if is_dir { p.as_os_str().to_owned() } else { let mut a = std::ffi::OsString::from("/select,"); a.push(p.as_os_str()); a };
        Command::new("explorer").arg(arg).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        if is_dir { Command::new("open").arg(p).spawn()?; } else { Command::new("open").arg("-R").arg(p).spawn()?; }
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // the freedesktop file-manager interface selects the file (Dolphin, Nautilus, Nemo, …)
        if !is_dir {
            let ok = Command::new("gdbus").args(["call", "--session", "--dest", "org.freedesktop.FileManager1", "--object-path", "/org/freedesktop/FileManager1",
                "--method", "org.freedesktop.FileManager1.ShowItems", &format!("['{}']", file_uri(p).replace('\'', "%27")), ""])
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            if ok { return Ok(()); }
        }
        let dir = if is_dir { p } else { p.parent().unwrap_or(p) };
        Command::new("xdg-open").arg(dir).spawn()?;
        Ok(())
    }
}

/// Text from a file whatever wrote it: UTF-8 (a byte-order mark is fine), else Windows-1252,
/// which is what Excel and older trackers save CSV in.
pub fn text_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| match b {
            0x80 => '€', 0x82 => '‚', 0x83 => 'ƒ', 0x84 => '„', 0x85 => '…', 0x86 => '†', 0x87 => '‡', 0x88 => 'ˆ', 0x89 => '‰', 0x8A => 'Š', 0x8B => '‹', 0x8C => 'Œ', 0x8E => 'Ž',
            0x91 => '‘', 0x92 => '’', 0x93 => '“', 0x94 => '”', 0x95 => '•', 0x96 => '–', 0x97 => '—', 0x98 => '˜', 0x99 => '™', 0x9A => 'š', 0x9B => '›', 0x9C => 'œ', 0x9E => 'ž', 0x9F => 'Ÿ',
            b => b as char,
        }).collect(),
    }
}
