-- Your own data about games: play status and collections. Games are referred to by
-- "<system>:<group key>" (the same on every PC); ref_json keeps enough to find the game again
-- if its key ever differs (title id, title).
CREATE TABLE play (
  ref TEXT PRIMARY KEY, ref_json TEXT NOT NULL,
  status TEXT, score INTEGER, notes TEXT, started TEXT, finished TEXT, hours REAL,
  favorite INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL
);
CREATE TABLE collections (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, notes TEXT, deleted INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, items_updated_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE collection_items (
  collection_id TEXT NOT NULL, pos INTEGER NOT NULL, ref TEXT NOT NULL, ref_json TEXT NOT NULL, note TEXT,
  PRIMARY KEY(collection_id, pos)
);
CREATE INDEX collection_items_ref ON collection_items(ref);
