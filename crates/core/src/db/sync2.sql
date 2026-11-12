-- A change waiting to be acknowledged keeps the time (and so the id) it was first sent with, so
-- a retry after a lost reply is stored once.
CREATE TABLE sync_pending (key TEXT PRIMARY KEY, hash TEXT NOT NULL, ts INTEGER NOT NULL);
