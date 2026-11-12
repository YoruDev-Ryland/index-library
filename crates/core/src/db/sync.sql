-- Sync with a server (optional). sync_sent: what was last sent or received per record key, so
-- only changes go out; root: the library folder a game's record belongs to. sync_inbox: records
-- for games or files not in this library yet (tried again after each scan). sync_covers: the
-- sha256 of each cover as uploaded (at grid size).
CREATE TABLE sync_sent (key TEXT PRIMARY KEY, hash TEXT NOT NULL, ts INTEGER NOT NULL, root TEXT, body TEXT);
CREATE TABLE sync_inbox (key TEXT PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE sync_state (name TEXT PRIMARY KEY, value TEXT);
CREATE TABLE sync_covers (path TEXT PRIMARY KEY, mtime INTEGER NOT NULL, sha TEXT NOT NULL);
