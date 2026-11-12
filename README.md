# Index

A desktop app for organising a large emulator game library, including one on a NAS, and the
physical games on your shelf.

- **Know what you have.** It scans your consoles' folders (listing only, quick over a network) and
  fingerprints files to verify them against the No-Intro and Redump lists.
- **Clean it up.** It fixes names, finds duplicates and junk, and tidies folder layouts. Every
  change is previewed, journaled and undoable.
- **Game info and art.** Names, details, box scans and artwork come from the release a file was
  verified as, and from IGDB, SteamGridDB, RAWG or ScreenScraper with your keys.
- **Completion.** Each console's full list, one entry per game, shows what you're missing.
- **Your shelf.** Physical copies, each with parts (loose, CIB, sealed), condition, edition, where
  it's kept and what you paid, mixed into the same library.
- **Play status and collections**, synced between PCs over the NAS, or through the optional
  [sync server](https://github.com/YoruDev-Ryland/index-sync-server) for phones and new PCs.

## Download

Get the latest build for Linux, Windows or macOS from
[Releases](https://github.com/YoruDev-Ryland/index-library/releases). The builds aren't
code-signed, so Windows and macOS ask once before the first run; the release notes say how to get
past it.

## Build from source

You need Rust (stable), Node.js 22 and, on Linux, the WebKitGTK development packages (see
[Tauri's prerequisites](https://tauri.app/start/prerequisites/)).

```bash
cd app
npm install
npm run tauri dev        # development
npm run tauri build      # AppImage / .deb (Linux), NSIS installer (Windows)
```

Command-line tool (same database and settings as the app):

```bash
cargo run --release -p lo-cli -- --help   # the command is `ix`
ix scan                  # index (listing only; seconds over SMB)
ix stats
ix rename --system nes   # preview; add --apply to rename
ix action list
ix action run n3ds.decrypt_cia --system n3ds          # dry run; add --yes
ix archive list | purge-expired | empty --yes
```

`INDEX_HOME=/some/dir` (app) or `--home /some/dir` (CLI) keeps settings, database and cache in one
folder, for portable use or for testing against a copy of a library.

## Where things live

| What | Where |
|---|---|
| Settings (library roots, tool paths, API keys) | `~/.config/index-library/settings.toml` (Windows: `%APPDATA%\index-library\config`) |
| Index database (a cache, rebuilt by scanning) | `~/.local/share/index-library/library.db` |
| Artwork cache | `~/.cache/index-library/art/` |
| Shared state for multiple PCs | `<library>/.index-library/` (journal + locks) |
| Archived originals (7-day hold) | `<library>/<system>/_archive/` with `manifest.json` |

## Safety

- A scan only lists folders; probes read at most a few KB per file; hashing is opt-in and throttled.
- Every rename/move/archive is journaled and undoable (Change history). Nothing is renamed without a preview.
- Actions run on a local copy; the original moves to `_archive` only after the output is verified, and
  stays there for the hold period (Settings → Keep archived originals for).

## Extending

See [docs/EXTENDING.md](docs/EXTENDING.md) for adding a system or an action.

## Layout

```
crates/core   all logic (scanner, parsers, probes, hashing, DATs, rename/ops/undo, archive,
              shared NAS state, jobs, actions, metadata) — no UI dependency
crates/cli    `ix` command-line front end
app/          Tauri shell (src-tauri) + Svelte UI (src)
```

## Releasing

Set the version in `Cargo.toml`, `app/package.json` and `app/src-tauri/tauri.conf.json`, commit,
then run `scripts/publish.sh <version>`. That pushes a release commit and its tag, and GitHub
Actions builds and publishes every platform.

## License

MIT. See [LICENSE](LICENSE).
