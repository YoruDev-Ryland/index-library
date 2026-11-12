//! Compression: PS1 .cue/.bin → .chd (chdman), GameCube disc → .rvz (dolphin-tool).
use super::*;
use crate::util::stem_of;

/// Track files named in a .cue (FILE "x.bin" BINARY), resolved next to the cue.
pub fn cue_tracks(fs: &dyn Transport, cue: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let m = fs.stat(cue)?;
    let text = String::from_utf8_lossy(&fs.read_range(cue, 0, m.size as usize)?).into_owned();
    let dir = cue.parent().unwrap_or(Path::new(""));
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.to_uppercase().starts_with("FILE") { continue; }
        let name = match (l.find('"'), l.rfind('"')) {
            (Some(a), Some(b)) if b > a => l[a + 1..b].to_string(),
            _ => l.split_whitespace().nth(1).unwrap_or("").to_string(),
        };
        if !name.is_empty() { out.push(dir.join(name)); }
    }
    Ok(out)
}

/// Point any .m3u next to `old` at `new` (multi-disc playlists) — journaled and undoable.
fn update_playlists(ctx: &mut ActionCtx, old: &Path, new: &Path) -> anyhow::Result<()> {
    let Some(dir) = old.parent() else { return Ok(()) };
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let pairs = vec![(name(old), name(new))];
    let lists: Vec<PathBuf> = ctx.fs.list(dir)?.into_iter().filter(|e| !e.is_dir && crate::util::ext_of(&e.name) == "m3u").map(|e| dir.join(e.name)).collect();
    let ops: Vec<Op> = lists.into_iter().map(|l| Op::rewrite_refs(l.clone(), l, pairs.clone())).collect();
    if !ops.is_empty() { apply(ctx.db, ctx.fs, ctx.journal, ops, "update playlist")?; }
    Ok(())
}

pub struct Ps1Chd;
impl Action for Ps1Chd {
    fn id(&self) -> &'static str { "compress.ps1_chd" }
    fn name(&self) -> &'static str { "Compress to CHD" }
    fn description(&self) -> &'static str { "Convert a .cue/.bin set to a single .chd with chdman; the originals go to the archive." }
    fn systems(&self) -> &'static [&'static str] { &["ps1"] }
    fn applies(&self, f: &FileRow) -> bool { f.ext == "cue" && !f.status_is("compressed", "true") }
    fn sets(&self) -> &'static [&'static str] { &["compressed"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> {
        vec![tool_requirement(s, "chdman", "chdman", "Install chdman (part of MAME tools; Arch: `sudo pacman -S mame-tools`).")]
    }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> {
        let tracks = cue_tracks(ctx.fs, &f.path)?;
        if tracks.is_empty() { anyhow::bail!("the .cue lists no track files"); }
        for t in &tracks { if !ctx.fs.exists(t) { anyhow::bail!("missing track file {}", t.file_name().unwrap_or_default().to_string_lossy()); } }
        let local_cue = ctx.temp_copy(&f.path)?;
        let work = local_cue.parent().unwrap().to_path_buf();
        for t in &tracks { ctx.fs.copy(t, &work.join(t.file_name().unwrap()), &|_| {})?; }
        let out = work.join(format!("{}.chd", stem_of(&f.name)));
        let prog = tool_program(ctx.settings, "chdman", "chdman");
        ctx.run_cmd(&prog, &["createcd".as_ref(), "-i".as_ref(), local_cue.as_os_str(), "-o".as_ref(), out.as_os_str()], Some(&work))?;
        if !out.exists() { anyhow::bail!("chdman produced no output"); }
        ctx.log("verifying the CHD");
        ctx.run_cmd(&prog, &["verify".as_ref(), "-i".as_ref(), out.as_os_str()], Some(&work)).map_err(|e| e.context("the CHD failed verification; originals left untouched"))?;
        let dst = f.path.with_file_name(out.file_name().unwrap());
        ctx.place_output(&out, &dst, f)?;
        ctx.archive_original(&f.path, "compressed to CHD")?;
        for t in &tracks { ctx.archive_original(t, "compressed to CHD")?; }
        update_playlists(ctx, &f.path, &dst)?;
        ctx.set_status(f.game_id, "compressed", "true")?;
        Ok(ActionOutcome::Done { output: dst })
    }
}

pub struct GcRvz;
impl Action for GcRvz {
    fn id(&self) -> &'static str { "compress.gc_rvz" }
    fn name(&self) -> &'static str { "Compress to RVZ" }
    fn description(&self) -> &'static str { "Convert a GameCube disc image to .rvz with dolphin-tool; the original goes to the archive." }
    fn systems(&self) -> &'static [&'static str] { &["gamecube"] }
    fn applies(&self, f: &FileRow) -> bool { matches!(f.ext.as_str(), "iso" | "gcm" | "ciso" | "gcz" | "wbfs" | "nkit.iso") && !f.status_is("compressed", "true") }
    fn sets(&self) -> &'static [&'static str] { &["compressed"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> {
        vec![tool_requirement(s, "dolphin-tool", "dolphin-tool", "Install Dolphin (it ships dolphin-tool; Arch: `sudo pacman -S dolphin-emu`).")]
    }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> {
        let local = ctx.temp_copy(&f.path)?;
        let out = local.with_file_name(format!("{}.rvz", stem_of(&f.name)));
        let prog = tool_program(ctx.settings, "dolphin-tool", "dolphin-tool");
        ctx.run_cmd(&prog, &["convert".as_ref(), "-f".as_ref(), "rvz".as_ref(), "-c".as_ref(), "zstd".as_ref(), "-l".as_ref(), "5".as_ref(), "-b".as_ref(), "131072".as_ref(), "-i".as_ref(), local.as_os_str(), "-o".as_ref(), out.as_os_str()], None)?;
        if !out.exists() { anyhow::bail!("dolphin-tool produced no output"); }
        ctx.log("verifying the RVZ");
        ctx.run_cmd(&prog, &["verify".as_ref(), "-i".as_ref(), out.as_os_str()], None).map_err(|e| e.context("the RVZ failed verification; original left untouched"))?;
        let dst = f.path.with_file_name(out.file_name().unwrap());
        ctx.place_output(&out, &dst, f)?;
        ctx.archive_original(&f.path, "compressed to RVZ")?;
        ctx.set_status(f.game_id, "compressed", "true")?;
        Ok(ActionOutcome::Done { output: dst })
    }
}
