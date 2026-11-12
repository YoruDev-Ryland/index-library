# Identify: one answer to "what exactly is this file?"

Naming, game info, artwork and grouping each used to work out a game's identity on their own,
mostly from the file name. This plan makes identification one step, with one stored answer per
game, which everything else reads.

## Measured on the real library (Nov 2026, read-only copy)

- 9,769 of 9,791 games already have fingerprints. No DATs had been loaded, so none were verified.
- With libretro-database's No-Intro / Redump lists, most cartridge systems verify 90–99%:
  DS 1,715 of 1,755, GBA 840/882, Genesis 665/708, 2600 563/566, GB+GBC 904/929, PCE 343/355.
- Gaps: NES 0/655, SNES 283/778, Atari 7800 1/66, Lynx: the ROMs carry a copier/format header
  that No-Intro hashes without. PSP 2/769: CSO images, not plain ISOs. Vita 0/4,566: digital
  packages; the No-Intro list is cartridges only.
- 392 of the 661 games waiting in Match review are fingerprint-verified.
- libretro thumbnails serve the box scan under the exact DAT name
  (`thumbnails.libretro.com/<DAT system>/Named_Boxarts/<DAT game>.png`).

Steps 1–2, checked on 30 random real files per system: NES 30/30 and SNES 30/30 identified
(NES 0 before), Atari 7800 29/30 (0 before), Lynx 29/30.

## The identity, per game

The strongest evidence wins, and the game shows which one it is:

1. **Fingerprint** — the file's SHA-1/CRC in the system's DAT → the exact release
   ("Super Huey UH-IX (USA)"), region, serial. Certain.
2. **Header ID** — Switch title ID (titledb), PSP disc ID (PSP_GAMES.tsv), Vita content ID. Certain.
3. **Catalog match** — the release name (or, without 1–2, the file name) matched against the
   console's catalog on our side, with IGDB's alternative names. Sure or not; unsure goes to
   Match review.

`games.canonical_id` holds the release name from 1 or 2; `meta_json` the catalog entry from 3, with
how it was found.

## What reads it

- **Covers (grid):** the libretro box scan for a verified release; otherwise IGDB / SteamGridDB.
- **Banners, heroes:** SteamGridDB, for variety.
- **Info:** the catalog entry.
- **Names:** the verified release name (already: `rename.rs` source "dat").
- **Grouping:** unchanged for now. Merging regions / splitting mis-grouped games comes later, on
  top of this data.
- **Game drawer:** "Identified by fingerprint (No-Intro)", "by title ID", or "by name".

## Steps

1. ✓ **DATs by default.** Download the lists automatically once fingerprints exist (and refresh
   monthly); verify after hashing. Arcade sets (Neo Geo) are named by FBNeo's list, by set name
   (`aof` → Art of Fighting; a variant like `kof98n` by its parent): 243 of 268 sets.
2. ✓ **Headerless fingerprints.** NES (16-byte iNES), SNES (512-byte copier, when size % 1024 = 512),
   Atari 7800 (128-byte A78), Lynx (64-byte LNX): hash the ROM data without the header too.
3. ✓ **Lookups from the identity.** Searched and compared by the release name as well as the file
   name (the better of the two counts; a release name that is only part of the file name counts
   no more than a part of a name). IGDB's alternative names count like its names ("Sagaia" →
   Darius II). The search goes on until a name is confirmed for the console.
4. ✓ **Exact covers.** The libretro box scan of each verified release is the grid cover (about 9
   in 10 releases have one; stored as JPEG, ≤ 800 px tall, ~90 KB); banners stay SteamGridDB.
5. ✓ **Per-console catalog.** IGDB's games per console with their other names (62k games, ~4 MB
   for this library; PC, at 213k, is left to title search), refreshed monthly, matched locally
   before any title search. Its best agrees with 8,005 of 8,017 existing IGDB matches, and where
   it differs it is mostly right (PC Engine "Adventure Island" is Dragon's Curse).
6. **Vita / PSP IDs.** Content IDs and disc IDs through NoPayStation / Redump lists.
7. **Drawer and review:** show how each game was identified; review shows only what's left.
