-- Images are now stored on disk as `<hash>.<extension>`, so the path is derived
-- instead of stored. The files themselves are moved by
-- `db::relocate_legacy_files` before this migration runs.

-- Keep the extension: the text after the last '.' of the old path.
-- rtrim(x, <x without dots>) strips everything after the last '.'.
ALTER TABLE images ADD COLUMN extension TEXT NOT NULL DEFAULT '';
UPDATE images
SET extension = lower(replace(file_path, rtrim(file_path, replace(file_path, '.', '')), ''));
ALTER TABLE images DROP COLUMN file_path;

-- Names identify images in the CLI and API, so they must be unique.
CREATE UNIQUE INDEX images_name_unique ON images(name COLLATE NOCASE);

-- Rebuild tags: databases created before the cascade was added don't have
-- ON DELETE CASCADE, and CREATE TABLE IF NOT EXISTS never updated them.
CREATE TABLE tags_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    image_hash TEXT NOT NULL REFERENCES images(hash) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(image_hash, tag)
);
INSERT INTO tags_new (id, image_hash, tag, created_at)
SELECT id, image_hash, tag, created_at FROM tags
WHERE image_hash IN (SELECT hash FROM images);
DROP TABLE tags;
ALTER TABLE tags_new RENAME TO tags;

CREATE INDEX tags_tag ON tags(tag);
