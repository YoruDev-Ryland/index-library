# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(A Tauri desktop app — Svelte front end in a system webview — for Linux and Windows.)

## Users

The owner and a few other people in the same household or circle, each opening the app from their own PC against one shared game library on a NAS. They know emulation well: systems, regions, dumps, NSP/XCI, DATs.

## Product Purpose

Index keeps a large emulator game library (about 2 TB, ~13,000 games across ~28 systems) clean and pleasant to live in. It indexes the library, cleans up names, finds duplicates and proves which copies are safe to remove, files games into a consistent folder layout, verifies dumps, runs per-system actions (decrypt, compress), looks up game info and art, and tracks what you play: status, scores, reviews and collections with a play order.

Success: the library stays tidy without fear of losing anything unique, and the app is where you go to decide and remember what to play.

## Positioning

It works on the real files in place and never throws data away on a guess: every removal is proven (fingerprints, content pieces, unpacking) and lands in a held archive first, and every change can be undone. State is shared between PCs through journals stored in the library itself.

## Operating Context

- Used about equally for maintenance (duplicates, cleanup, renames, verification — in bursts) and for browsing and tracking play (collections, play status, game details).
- Long lists are the norm: thousands of games per view, dozens of duplicate groups, long file names with brackets, ids and versions.
- Desktop windows, mouse and keyboard; the owner runs KDE on Linux.

## Capabilities and Constraints

- Library on a network share; listing and reading are slow, so heavy work runs as background jobs with a jobs bar.
- Personal settings and API keys stay on each PC; shared state (journals, archive manifests, play data, collections) lives in the library.
- Follows the system light/dark preference.

## Product Principles

1. Never lose unique data: prove before removing, archive before deleting, undo everything.
2. Show what is true about each file in plain words — what it is, where it is, why something is suggested.
3. Consistency across the whole collection, not one-off fixes.
4. Prefer what runs as-is: the copy that plays without unpacking or installing.
5. The library is also a place to enjoy: collections, play order, scores and notes are first-class.
