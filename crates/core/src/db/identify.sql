-- Fingerprints of ROM data without its header (NES, SNES, Atari 7800, Lynx): No-Intro lists those
-- games by their data alone. Files of those systems are fingerprinted again to fill them in.
ALTER TABLE files ADD COLUMN headerless_crc32 TEXT;
ALTER TABLE files ADD COLUMN headerless_sha1 TEXT;
CREATE INDEX files_headerless_sha1 ON files(headerless_sha1);
UPDATE files SET hash_key = NULL WHERE system_id IN ('nes', 'snes', 'atari7800', 'lynx');
