-- Physical copies on your shelf, one row per copy. A game exists while it has files or a copy
-- here (deleted = 0). Removed copies stay (deleted = 1) so other devices learn they're gone.
CREATE TABLE shelf_copies (
  id TEXT PRIMARY KEY, ref TEXT NOT NULL, ref_json TEXT NOT NULL, system_id TEXT NOT NULL,
  release TEXT NOT NULL, region TEXT, edition TEXT,
  cart INTEGER NOT NULL DEFAULT 0, box INTEGER NOT NULL DEFAULT 0, manual INTEGER NOT NULL DEFAULT 0,
  inserts INTEGER NOT NULL DEFAULT 0, sealed INTEGER NOT NULL DEFAULT 0,
  condition TEXT, location TEXT, price_cents INTEGER, bought TEXT, notes TEXT,
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX shelf_copies_ref ON shelf_copies(ref);
