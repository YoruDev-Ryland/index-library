//! 3DS decryption using the 3DS-Decrypt Python scripts (decrypt_cia.py / decrypt_3ds.py).
use super::*;
use crate::parse::strip_decrypted_suffix;
use crate::probe::ProbeKind;
use crate::util::stem_of;

const DEFAULT_DIR: &[&str] = &["Nintendo - 3DS", "3DS-Decrypt"];

fn script_dir(s: &Settings) -> Option<PathBuf> { tool_path(s, "3ds-decrypt", DEFAULT_DIR) }

fn requirements(s: &Settings, script: &str) -> Vec<Requirement> {
    let py = program_runs(&s.python, &["--version"]);
    let crypto = py && program_runs(&s.python, &["-c", "import Crypto.Cipher.AES"]);
    let dir = script_dir(s);
    let has_script = dir.as_ref().map(|d| d.join(script).exists()).unwrap_or(false);
    vec![
        Requirement { name: format!("Python ({})", s.python), ok: py, hint: "Install Python 3 or set its path in Settings → Tools.".into() },
        Requirement { name: "pycryptodome".into(), ok: crypto, hint: "Install pycryptodome: `pip install pycryptodome` (Arch/CachyOS: `sudo pacman -S python-pycryptodome`).".into() },
        Requirement { name: script.into(), ok: has_script, hint: format!("Point Settings → Tools → 3ds-decrypt at the folder containing {script} (default: <library>/Nintendo - 3DS/3DS-Decrypt).") },
    ]
}

fn is_encrypted(f: &FileRow) -> bool {
    match f.probe.as_ref().and_then(|p| p.encrypted) { Some(e) => e, None => !f.status_is("decrypted", "true") }
}

/// Shared flow: local copy → run script → verify output decrypted → archive original → upload clean name.
fn decrypt(ctx: &mut ActionCtx, f: &FileRow, script: &str, in_place: bool) -> anyhow::Result<ActionOutcome> {
    let dir = script_dir(ctx.settings).ok_or_else(|| anyhow::anyhow!("3DS-Decrypt scripts not found; set Settings → Tools → 3ds-decrypt"))?;
    let local = ctx.temp_copy(&f.path)?;
    let script_path = dir.join(script);
    let python = ctx.settings.python.clone();
    ctx.run_cmd(&python, &[script_path.as_os_str(), local.as_os_str()], Some(&dir))?;
    let output = if in_place { local.clone() } else { local.with_file_name(format!("{}-decrypted.{}", stem_of(&f.name), f.ext)) };
    if !output.exists() { anyhow::bail!("the script finished but produced no output ({})", output.file_name().unwrap_or_default().to_string_lossy()); }
    match ctx.probe_local(ProbeKind::N3ds, &output)?.encrypted {
        Some(false) => {}
        Some(true) => anyhow::bail!("output is still encrypted; original left untouched"),
        None => anyhow::bail!("output is not a valid 3DS file; original left untouched"),
    }
    let ext = &f.name[f.name.len() - f.ext.len()..];
    let clean = format!("{}.{ext}", strip_decrypted_suffix(&stem_of(&f.name)));
    let dst = f.path.with_file_name(&clean);
    if dst != f.path && ctx.fs.exists(&dst) { anyhow::bail!("{clean} already exists next to the original"); }
    let archived = ctx.archive_original(&f.path, "replaced by decrypted copy")?;
    if let Err(e) = ctx.place_output(&output, &dst, f) {
        ctx.log("upload failed, restoring the original");
        if let Err(re) = ctx.restore_archived(&archived) {
            // The archived file is now the only copy: never let it expire.
            let _ = crate::archive::pin(ctx.db, ctx.fs, archived.id, "upload failed and restore failed — this is the only copy; restore it from the Archive");
            return Err(e.context(format!("could not restore the original either ({re:#}); it is kept in the archive and will not be deleted")));
        }
        return Err(e);
    }
    ctx.set_status(f.game_id, "decrypted", "true")?;
    Ok(ActionOutcome::Done { output: dst })
}

pub struct DecryptCia;
impl Action for DecryptCia {
    fn id(&self) -> &'static str { "n3ds.decrypt_cia" }
    fn name(&self) -> &'static str { "Decrypt CIA" }
    fn description(&self) -> &'static str { "Decrypt a .cia with decrypt_cia.py; the encrypted original goes to the archive." }
    fn systems(&self) -> &'static [&'static str] { &["n3ds"] }
    fn applies(&self, f: &FileRow) -> bool { f.ext == "cia" && is_encrypted(f) }
    fn sets(&self) -> &'static [&'static str] { &["decrypted"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> { requirements(s, "decrypt_cia.py") }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> { decrypt(ctx, f, "decrypt_cia.py", false) }
}

pub struct Decrypt3ds;
impl Action for Decrypt3ds {
    fn id(&self) -> &'static str { "n3ds.decrypt_3ds" }
    fn name(&self) -> &'static str { "Decrypt .3ds" }
    fn description(&self) -> &'static str { "Decrypt a .3ds/.cci with decrypt_3ds.py (run on a local copy, since it works in place)." }
    fn systems(&self) -> &'static [&'static str] { &["n3ds"] }
    fn applies(&self, f: &FileRow) -> bool { matches!(f.ext.as_str(), "3ds" | "cci") && is_encrypted(f) }
    fn sets(&self) -> &'static [&'static str] { &["decrypted"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> { requirements(s, "decrypt_3ds.py") }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> { decrypt(ctx, f, "decrypt_3ds.py", true) }
}
