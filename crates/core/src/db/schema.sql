CREATE TABLE dirs (
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, parent_id INTEGER, system_id TEXT,
  mtime INTEGER, scanned_at INTEGER, error TEXT, seen_scan INTEGER
);
CREATE TABLE files (
  id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, dir_id INTEGER NOT NULL, system_id TEXT NOT NULL,
  name TEXT NOT NULL, ext TEXT NOT NULL, is_dir INTEGER NOT NULL DEFAULT 0, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  game_id INTEGER, probe_json TEXT, probe_key TEXT, crc32 TEXT, sha1 TEXT, md5 TEXT, hash_key TEXT,
  dat_status TEXT, dat_game TEXT, dat_rom TEXT, seen_scan INTEGER
);
CREATE INDEX files_system ON files(system_id);
CREATE INDEX files_game ON files(game_id);
CREATE INDEX files_sha1 ON files(sha1);
CREATE INDEX files_crc ON files(crc32);
CREATE INDEX files_dir ON files(dir_id);
CREATE TABLE games (
  id INTEGER PRIMARY KEY, system_id TEXT NOT NULL, group_key TEXT NOT NULL, title TEXT NOT NULL,
  parsed_json TEXT, canonical_id TEXT, meta_json TEXT, art_json TEXT, meta_state TEXT,
  UNIQUE(system_id, group_key)
);
CREATE INDEX games_system ON games(system_id);
CREATE TABLE status (
  game_id INTEGER NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, updated_at INTEGER NOT NULL,
  PRIMARY KEY(game_id, key)
);
CREATE TABLE ops (
  id INTEGER PRIMARY KEY, batch_id TEXT NOT NULL, machine_id TEXT NOT NULL, kind TEXT NOT NULL,
  src TEXT, dst TEXT, created_at INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0, label TEXT, undo_data BLOB
);
CREATE INDEX ops_batch ON ops(batch_id);
CREATE TABLE archive (
  id INTEGER PRIMARY KEY, system_id TEXT, path TEXT NOT NULL UNIQUE, original_path TEXT NOT NULL,
  archived_at INTEGER NOT NULL, delete_after INTEGER NOT NULL, reason TEXT, action TEXT, size INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE journal_cursor (machine_id TEXT PRIMARY KEY, offset INTEGER NOT NULL);
CREATE TABLE dats (id INTEGER PRIMARY KEY, system_id TEXT NOT NULL, name TEXT, version TEXT);
CREATE TABLE dat_roms (
  dat_id INTEGER NOT NULL, game_name TEXT NOT NULL, rom_name TEXT NOT NULL, size INTEGER,
  crc32 TEXT, sha1 TEXT, md5 TEXT
);
CREATE INDEX dat_roms_sha1 ON dat_roms(sha1);
CREATE INDEX dat_roms_crc ON dat_roms(crc32);
CREATE TABLE meta_cache (source TEXT NOT NULL, key TEXT NOT NULL, json TEXT NOT NULL, fetched_at INTEGER NOT NULL, PRIMARY KEY(source, key));
CREATE TABLE titledb (source TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, region TEXT, extra_json TEXT, PRIMARY KEY(source, id));
CREATE TABLE jobs (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, args_json TEXT, state TEXT NOT NULL, progress REAL, created_at INTEGER NOT NULL);
CREATE TABLE devices (id INTEGER PRIMARY KEY, serial TEXT UNIQUE, name TEXT, transport TEXT, last_seen INTEGER, info_json TEXT);
