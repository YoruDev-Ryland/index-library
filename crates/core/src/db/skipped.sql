-- Suggestions you've said no to: a match you skipped in review, a cleanup item you ignored, a
-- duplicate group you're keeping, a folder you want left as it is, a name you're keeping.
-- They aren't suggested again until you bring them back (or, for a game's match, look it up
-- yourself). Keys describe the exact situation, so a changed one is suggested afresh.
CREATE TABLE skipped (
  kind TEXT NOT NULL,      -- match | junk | dupe | layout | rename
  key TEXT NOT NULL,
  label TEXT NOT NULL DEFAULT '',
  system_id TEXT,
  at INTEGER NOT NULL,
  PRIMARY KEY(kind, key)
);
