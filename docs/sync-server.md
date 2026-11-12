# Sync server

The optional sync server lives in its own repository, **index-sync-server**
(github.com/YoruDev-Ryland/index-sync-server). Its image is `ghcr.io/yorudev-ryland/index-sync-server`.
Setup, configuration and the API are documented there.

In this repository:

- `crates/core/src/sync/` is the desktop's side: what it sends, applying what it receives, and restoring a new PC.
- The record format comes from that repository's `index-sync-protocol` crate.
- `crates/core/tests/sync_recovery.rs` runs the real server in-process, end to end.
- Settings → Sync (`app/src/components/SyncSettings.svelte`) is where a PC connects.
