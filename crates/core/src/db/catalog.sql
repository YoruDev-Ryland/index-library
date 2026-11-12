-- Every game a source lists per console (IGDB: names and other names), matched locally.
CREATE TABLE catalog (system_id TEXT NOT NULL, source TEXT NOT NULL, id TEXT NOT NULL, name TEXT NOT NULL, aliases_json TEXT, year INTEGER, PRIMARY KEY(system_id, source, id));
CREATE TABLE catalogs (system_id TEXT NOT NULL, source TEXT NOT NULL, fetched_at INTEGER NOT NULL, entries INTEGER NOT NULL, PRIMARY KEY(system_id, source));
