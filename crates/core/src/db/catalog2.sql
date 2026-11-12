-- Each stored catalog is told apart from the one before (the matching index follows it).
ALTER TABLE catalogs ADD COLUMN generation TEXT;
