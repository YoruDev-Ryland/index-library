# Adding systems and actions

Both are one new piece of code plus one registry line. Ask Claude "add an action for X" and point it here.

## A new system

Add a line to `crates/core/src/systems/defs.rs`:

```rust
sys!("wii", "Wii", ["Nintendo - Wii"], ["iso","wbfs","rvz"], NI, None, Single, DISC_T, Some(5), Some(16), None),
```

Fields: id, display name, folder names it matches (case/punctuation-insensitive), file extensions,
name parsers, optional header probe, grouping (`Single`, `CueBin`, `FolderPerGame`, `SwitchTitle`),
rename template, IGDB platform id, ScreenScraper system id, SteamGridDB platform.

Folders that don't match can be mapped in `settings.toml`:

```toml
[folder_overrides]
"My Wii Games" = { kind = "system", system = "wii" }
"Old Stuff" = { kind = "ignored" }
```

Rename templates: `{field}` inserts a value; `{ ( $field)}`-style groups are dropped when the field is
empty, e.g. `{title}{ ($regions)}{ (Rev $rev)}{ (Disc $disc)}`. Fields: title, regions, langs, rev,
version, disc, tags, title_id, serial.

A header probe (exact ids / encryption flags from the first few KB) goes in `crates/core/src/probe/`.

## A new action

1. Create `crates/core/src/actions/<name>.rs` implementing `Action`:

```rust
use super::*;

pub struct MyAction;
impl Action for MyAction {
    fn id(&self) -> &'static str { "wii.to_rvz" }
    fn name(&self) -> &'static str { "Compress to RVZ" }
    fn description(&self) -> &'static str { "Convert to .rvz with dolphin-tool; original goes to the archive." }
    fn systems(&self) -> &'static [&'static str] { &["wii"] }
    fn applies(&self, f: &FileRow) -> bool { f.ext == "iso" && !f.status_is("compressed", "true") }
    fn sets(&self) -> &'static [&'static str] { &["compressed"] }
    fn requires(&self, s: &Settings) -> Vec<Requirement> {
        vec![tool_requirement(s, "dolphin-tool", "dolphin-tool", "Install Dolphin.")]
    }
    fn run(&self, ctx: &mut ActionCtx, f: &FileRow) -> anyhow::Result<ActionOutcome> {
        let local = ctx.temp_copy(&f.path)?;                       // NAS → local temp (space-checked)
        let out = local.with_extension("rvz");
        let prog = tool_program(ctx.settings, "dolphin-tool", "dolphin-tool");
        ctx.run_cmd(&prog, &["convert".as_ref(), "-f".as_ref(), "rvz".as_ref(),
                            "-i".as_ref(), local.as_os_str(), "-o".as_ref(), out.as_os_str()], None)?;
        let dst = f.path.with_extension("rvz");
        ctx.place_output(&out, &dst, f)?;                         // upload via .lo-part, index it
        ctx.archive_original(&f.path, "compressed to RVZ")?;      // 7-day hold, undoable
        ctx.set_status(f.game_id, "compressed", "true")?;         // shown as a chip, shared across PCs
        Ok(ActionOutcome::Done { output: dst })
    }
}
```

2. Register it in `REGISTRY` in `crates/core/src/actions/mod.rs` and add `pub mod <name>;`.

The UI picks it up automatically: it appears on matching games and in the selection bar, the tools it
needs are checked before it runs, and its status key gets a chip (add a label in
`app/src/components/StatusChips.svelte` for a nicer name).

Helpers on `ActionCtx`: `temp_copy`, `temp_dir`, `run_cmd` (streams output to the job log, kills on
cancel), `place_output`, `archive_original`, `restore_archived`, `set_status`, `probe_local`, `log`.
Temp files are cleaned up automatically, and a cross-PC lease is held on the file while the action runs.

Test with stand-in tools, as in `crates/core/tests/actions_3ds.rs` and `actions_more.rs`.
