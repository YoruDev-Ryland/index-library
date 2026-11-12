//! PSP: PSN .pkg → playable compressed .cso.
//!
//! Uses `pkg2zip -x -c9 <pkg>` (open source, Linux + Windows), which writes
//! `pspemu/ISO/<name> [<id>].cso`. If it produces an .iso instead and `maxcso` is
//! available, the ISO is compressed. The PSNPKGDecryptor .exe is GUI-only and is
//! not automated.
use super::*;
use crate::rename::sanitize_filename;
use crate::util::stem_of;

pub struct PkgToCso;

fn region_name(code: &str) -> Option<&'static str> {
    Some(match code.to_uppercase().as_str() { "US" => "USA", "EU" => "Europe", "JP" => "Japan", "ASIA" => "Asia", "INT" => "World", _ => return None })
}

fn output_name(ctx: &ActionCtx, f: &FileRow) -> String {
    let content_id = f.probe.as_ref().and_then(|p| p.id.clone());
    let from_db: Option<(String, Option<String>)> = content_id.and_then(|cid| ctx.db.conn()
        .query_row("SELECT name, region FROM titledb WHERE source='psp' AND id=?1", [cid], |r| Ok((r.get(0)?, r.get(1)?))).optional().ok().flatten());
    let base = match from_db {
        Some((name, region)) => match region.as_deref().and_then(region_name) { Some(r) if !name.contains(&format!("({r})")) => format!("{name} ({r})"), _ => name },
        None => stem_of(&f.name),
    };
    sanitize_filename(&base)
}

impl Action for PkgToCso {
    fn id(&self) -> &'static str { "psp.pkg_to_cso" }
    fn name(&self) -> &'static str { "Decrypt PKG → CSO" }
    fn description(&self) -> &'static str { "Extract a PSN .pkg to a compressed .cso with pkg2zip; the .pkg goes to the archive." }
    fn systems(&self) -> &'static [&'static str] { &["psp"] }
    fn applies(&self, f: &FileRow) -> bool { f.ext == "pkg" && !f.status_is("decrypted", "true") }
    fn sets(&self) -> &'static [&'static str] { &["decrypted"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> {
        vec![tool_requirement(s, "pkg2zip", "pkg2zip", "Install pkg2zip (github.com/lusid1/pkg2zip; AUR: pkg2zip).")]
    }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> {
        let local = ctx.temp_copy(&f.path)?;
        let work = local.parent().unwrap().to_path_buf();
        let prog = tool_program(ctx.settings, "pkg2zip", "pkg2zip");
        ctx.run_cmd(&prog, &["-x".as_ref(), "-c9".as_ref(), local.as_os_str()], Some(&work))?;
        let produced = walk_files(&work);
        let find = |ext: &str| produced.iter().find(|p| p.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false)).cloned();
        let cso = match (find("cso"), find("iso")) {
            (Some(c), _) => c,
            (None, Some(iso)) => {
                let maxcso = tool_program(ctx.settings, "maxcso", "maxcso");
                if !program_exists(&maxcso) { anyhow::bail!("pkg2zip produced an ISO; install maxcso (or set Settings → Tools → maxcso) to compress it"); }
                let out = iso.with_extension("cso");
                ctx.run_cmd(&maxcso, &[iso.as_os_str(), "-o".as_ref(), out.as_os_str()], Some(&work))?;
                out
            }
            (None, None) if produced.iter().any(|p| p.file_name().map(|n| n.eq_ignore_ascii_case("EBOOT.PBP")).unwrap_or(false)) => {
                anyhow::bail!("this PKG unpacks to an EBOOT (PSP Minis / PS1 Classic), not an ISO — not supported yet; the .pkg was left in place")
            }
            _ => anyhow::bail!("pkg2zip produced no ISO/CSO"),
        };
        let folder = system_folder(&ctx.journal.roots, &f.path).unwrap_or_else(|| f.path.parent().unwrap().to_path_buf());
        let dst = folder.join(format!("{}.cso", output_name(ctx, f)));
        if ctx.fs.exists(&dst) { anyhow::bail!("{} already exists", dst.file_name().unwrap().to_string_lossy()); }
        ctx.place_output(&cso, &dst, f)?;
        ctx.archive_original(&f.path, "converted to CSO")?;
        ctx.set_status(f.game_id, "decrypted", "true")?;
        Ok(ActionOutcome::Done { output: dst })
    }
}
