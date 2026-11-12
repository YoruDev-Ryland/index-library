//! `lo` — command-line front end over lo-core (same database and settings as the app).
use clap::{Parser, Subcommand};
use lo_core::hash::HashLevel;
use lo_core::jobs::JobEvent;
use lo_core::library::{GameFilter, HashScope, Library};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "ix", about = "Index — ROM library tools", version)]
struct Cli {
    /// Keep settings, database and cache under this directory instead of the standard locations.
    #[arg(long, global = true)]
    home: Option<PathBuf>,
    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show or change library roots and settings location.
    Config { #[arg(long)] add_root: Option<PathBuf>, #[arg(long)] remove_root: Option<PathBuf> },
    /// Index the library (listing only; fast).
    Scan { #[arg(long)] full: bool },
    /// Per-system totals.
    Stats,
    /// List games.
    Games { #[arg(long)] system: Option<String>, #[arg(long)] search: Option<String>, #[arg(long)] limit: Option<i64> },
    /// Preview (or apply) clean names.
    Rename { #[arg(long)] system: Option<String>, #[arg(long)] apply: bool },
    /// Undo a batch (see `lo history`).
    Undo { batch: String },
    History,
    /// Compute CRC32/SHA1/MD5 fingerprints.
    Hash { #[arg(long)] system: Option<String>, #[arg(long, default_value = "normal")] level: String },
    /// Import a No-Intro/Redump DAT for a system and verify.
    Dat { system: String, file: PathBuf },
    Verify { system: String },
    /// Actions: list, check requirements, run.
    Action { #[command(subcommand)] cmd: ActionCmd },
    /// Archived originals.
    Archive { #[command(subcommand)] cmd: ArchiveCmd },
    /// --compare: unpack archives and fingerprint everything in groups not yet compared.
    Dupes { #[arg(long)] system: Option<String>, #[arg(long)] compare: bool },
    Junk,
    /// Everything known about one game (JSON).
    Game { id: i64 },
    /// Games whose folders don't follow their system's layout (--apply moves them).
    Layout { #[arg(long)] system: Option<String>, #[arg(long)] apply: bool },
    /// Import an offline title database (psp = NoPayStation TSV, switch = titledb JSON).
    Import { kind: String, file: PathBuf },
    /// Look up game info/artwork for a system.
    Meta { #[arg(long)] system: String },
}

#[derive(Subcommand)]
enum ActionCmd {
    List,
    Check { id: String },
    /// Run on every applicable file of a system.
    Run { id: String, #[arg(long)] system: String, #[arg(long)] yes: bool },
}

#[derive(Subcommand)]
enum ArchiveCmd { List, PurgeExpired, Empty { #[arg(long)] yes: bool } }

fn gb(b: i64) -> String { format!("{:.2} GB", b as f64 / 1e9) }

pub fn run(args: &[String], out: &mut dyn Write) -> anyhow::Result<()> {
    let cli = Cli::parse_from(args);
    let sink: lo_core::jobs::Sink = Arc::new(|e: JobEvent| match e {
        JobEvent::Progress { done, total, message, .. } if total > 0 => eprint!("\r  {done}/{total} {message}        "),
        JobEvent::Log { line, .. } => eprintln!("\r  {line}"),
        JobEvent::Finished { ok: false, error, .. } => eprintln!("\r  failed: {}", error.unwrap_or_default()),
        _ => {}
    });
    let lib = match &cli.home { Some(h) => Library::open_at(h, sink)?, None => Library::open(sink)? };
    let wait = |lib: &Library| { lib.jobs.wait_idle(Duration::from_secs(60 * 60 * 24)); eprintln!(); };
    macro_rules! emit { ($v:expr, $text:expr) => { if cli.json { writeln!(out, "{}", serde_json::to_string_pretty(&$v)?)?; } else { $text } } }
    match cli.cmd {
        Cmd::Config { add_root, remove_root } => {
            let mut s = lib.settings();
            if let Some(r) = add_root { if !s.library_roots.contains(&r) { s.library_roots.push(r); } }
            if let Some(r) = remove_root { s.library_roots.retain(|x| *x != r); }
            lib.save_settings(s.clone())?;
            writeln!(out, "settings: {}", lib.settings_path().display())?;
            for r in &s.library_roots { writeln!(out, "root: {}", r.display())?; }
        }
        Cmd::Scan { full } => {
            let t = std::time::Instant::now();
            lib.scan(full);
            wait(&lib);
            let r = lib.last_scan().unwrap_or_default();
            emit!(r, writeln!(out, "scanned in {:.1?}: {} folders listed, {} unchanged; +{} −{} ~{} files; {} errors", t.elapsed(), r.dirs_listed, r.dirs_skipped, r.files_added, r.files_removed, r.files_changed, r.errors.len())?);
            for u in &r.unknown_folders { if !cli.json { writeln!(out, "  unrecognized folder (map it in settings): {}", u.display())?; } }
        }
        Cmd::Stats => {
            let s = lib.stats()?;
            emit!(s, {
                writeln!(out, "{:<28} {:>7} {:>7} {:>11} {:>8}  status", "system", "games", "files", "size", "verified")?;
                for x in &s.systems { writeln!(out, "{:<28} {:>7} {:>7} {:>11} {:>8}  {}", x.name, x.games, x.files, gb(x.bytes as i64), x.verified, x.status_counts.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" "))?; }
                writeln!(out, "archive: {} files, {}; reclaimable duplicates: {}", s.archive_count, gb(s.archive_bytes as i64), gb(s.dupe_bytes as i64))?;
            });
        }
        Cmd::Games { system, search, limit } => {
            let p = lib.games(&GameFilter { system, search, ..Default::default() }, Default::default(), 0, limit.unwrap_or(100))?;
            emit!(p, { for g in &p.rows { writeln!(out, "{:>6}  {:<10} {} [{}] {}", g.id, g.system_id, g.title, g.regions.join(", "), g.status.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "))?; } writeln!(out, "{} of {}", p.rows.len(), p.total)?; });
        }
        Cmd::Rename { system, apply } => {
            let ids = lib.game_ids_for(&GameFilter { system, ..Default::default() })?;
            let plan = lib.plan_renames(&ids)?;
            let todo: Vec<_> = plan.iter().filter(|i| i.problem.is_none()).cloned().collect();
            if !cli.json {
                for i in &plan { if i.problem != Some(lo_core::rename::Problem::Unchanged) {
                    writeln!(out, "{} {}\n   → {}{}", if i.problem.is_none() { "✓" } else { "!" }, i.src.file_name().unwrap().to_string_lossy(), i.dst.file_name().unwrap().to_string_lossy(), i.problem.as_ref().map(|p| format!("   [{p:?}]")).unwrap_or_default())?;
                } }
                writeln!(out, "{} renames ready, {} blocked", todo.len(), plan.iter().filter(|i| !matches!(i.problem, None | Some(lo_core::rename::Problem::Unchanged))).count())?;
            } else { writeln!(out, "{}", serde_json::to_string_pretty(&plan)?)?; }
            if apply {
                let r = lib.apply_renames(&todo)?;
                writeln!(out, "applied {} (batch {}), {} failed", r.done.len(), r.batch_id, r.failed.len())?;
                for (op, e) in &r.failed { writeln!(out, "  ✗ {}: {e}", op.src.display())?; }
            } else if !todo.is_empty() { writeln!(out, "(preview only — add --apply to rename)")?; }
        }
        Cmd::Undo { batch } => { let r = lib.undo(&batch)?; writeln!(out, "undid {} ops, {} failed", r.done.len(), r.failed.len())?; }
        Cmd::History => { let h = lib.history()?; emit!(h, for b in &h { writeln!(out, "{}  {}  {} ops{}", b.batch_id, b.label, b.ops, if b.undone > 0 { " (undone)" } else { "" })?; }); }
        Cmd::Hash { system, level } => {
            lib.set_hash_level(HashLevel::parse(&level).ok_or_else(|| anyhow::anyhow!("level: background | normal | fast | max"))?);
            lib.hash(system.map(HashScope::System).unwrap_or(HashScope::All))?;
            wait(&lib);
        }
        Cmd::Dat { system, file } => { let s = lib.import_dat(&system, &file)?; let v = lib.verify(&system)?; writeln!(out, "{} ({}): {} games; verified {}, bad {}, unknown {}", s.name, s.version, s.games, v.verified, v.bad, v.unknown)?; }
        Cmd::Verify { system } => { let v = lib.verify(&system)?; writeln!(out, "verified {}, bad {}, unknown {}", v.verified, v.bad, v.unknown)?; }
        Cmd::Action { cmd } => match cmd {
            ActionCmd::List => for a in lib.actions() { writeln!(out, "{:<20} {:<22} [{}] {}", a.id, a.name, a.systems.join(","), a.description)?; },
            ActionCmd::Check { id } => for r in lib.requirements(&id)? { writeln!(out, "{} {}{}", if r.ok { "✓" } else { "✗" }, r.name, if r.ok { String::new() } else { format!(" — {}", r.hint) })?; },
            ActionCmd::Run { id, system, yes } => {
                let games = lib.game_ids_for(&GameFilter { system: Some(system), ..Default::default() })?;
                let files = lib.file_ids(&games)?;
                let hits: Vec<i64> = lib.actions_for(&files)?.into_iter().filter(|a| a.action.id == id).flat_map(|a| a.file_ids).collect();
                writeln!(out, "{} files to process", hits.len())?;
                if hits.is_empty() { return Ok(()); }
                if !yes { writeln!(out, "(dry run — add --yes to run)")?; return Ok(()); }
                lib.run_action(&id, &hits)?;
                wait(&lib);
            }
        },
        Cmd::Archive { cmd } => match cmd {
            ArchiveCmd::List => { let l = lib.archive_list()?; emit!(l, for e in &l { writeln!(out, "{:>5}  {}  delete after {}  {}", e.id, gb(e.size), chrono_day(e.delete_after), e.path.display())?; }); }
            ArchiveCmd::PurgeExpired => { let ids: Vec<i64> = lib.archive_expired()?.iter().map(|e| e.id).collect(); let r = lib.archive_purge_expired(&ids)?; writeln!(out, "deleted {} files, freed {}", r.deleted, gb(r.bytes as i64))?; }
            ArchiveCmd::Empty { yes } => {
                if !yes { writeln!(out, "this permanently deletes every archived original; re-run with --yes")?; return Ok(()); }
                let r = lib.archive_purge_all()?; writeln!(out, "deleted {} files, freed {}", r.deleted, gb(r.bytes as i64))?;
            }
        },
        Cmd::Dupes { system, compare } => {
            if compare {
                let ids: Vec<i64> = lib.dupes(system.as_deref())?.iter().filter(|g| g.proofs.iter().any(|p| *p == lo_core::dupes::Proof::Unchecked)).flat_map(|g| g.files.clone()).collect();
                lib.compare_contents(&ids)?;
                wait(&lib);
            } let d = lib.dupes(system.as_deref())?; emit!(d, for g in &d { writeln!(out, "{:?}  {}  ({} files, {} reclaimable)", g.reason, g.title, g.files.len(), gb(g.reclaimable as i64))?; }); }
        Cmd::Game { id } => { let g = lib.game_detail(id)?; writeln!(out, "{}", serde_json::to_string_pretty(&g)?)?; }
        Cmd::Junk => { let j = lib.junk()?; emit!(j, for x in &j { writeln!(out, "{:?}  {}\n   {}", x.kind, x.path.display(), x.suggestion)?; }); }
        Cmd::Layout { system, apply } => {
            let r = lib.layout(system.as_deref())?;
            if apply {
                let ids: Vec<i64> = r.iter().flat_map(|s| s.items.iter().map(|i| i.game_id)).collect();
                let b = lib.fix_layout(&ids)?;
                writeln!(out, "{} done, {} failed (batch {})", b.done.len(), b.failed.len(), b.batch_id)?;
                for (op, e) in &b.failed { writeln!(out, "  {}: {e}", op.src.display())?; }
                return Ok(());
            }
            emit!(r, for s in &r {
                writeln!(out, "{}  layout {:?}{} (detected {:?}; loose {}, own folder {}, grouped {}, split {})", s.system_id, s.layout, if s.chosen { " (chosen)" } else { "" }, s.detected, s.loose, s.per_game, s.grouped, s.split)?;
                for i in &s.items {
                    writeln!(out, "  {:?}  {}  [{}] -> {}", i.problem, i.title, i.places.join(" | "), i.target.as_ref().map(|t| t.display().to_string()).unwrap_or("-".into()))?;
                    for m in &i.moves { writeln!(out, "     move {}
       -> {}", m.src.display(), m.dst.display())?; }
                    for d in &i.duplicates { writeln!(out, "     archive spare copy {}", d.src.display())?; }
                    for (p, why) in &i.left { writeln!(out, "     left {}  ({why})", p.display())?; }
                }
            });
        }
        Cmd::Import { kind, file } => { let n = lib.import_titledb(&kind, &file)?; writeln!(out, "imported {n} titles")?; }
        Cmd::Meta { system } => { let ids = lib.game_ids_for(&GameFilter { system: Some(system), ..Default::default() })?; lib.match_metadata(&ids); wait(&lib); }
    }
    Ok(())
}

fn chrono_day(ts: i64) -> String {
    lo_core::db::format_day(ts)
}
