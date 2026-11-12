-- Text that is written inside an image ("caption"), one row per SOURCE.
--
-- Two actors write captions: OCR (the Mac app) and a person (CLI, Mac edit).
-- They never share a row: OCR owns the 'ocr' row, a person owns the 'manual'
-- row, and the effective caption is merged where images are read (manual wins,
-- else ocr, else none). So an OCR re-run can't clobber a hand edit, and
-- clearing a hand edit falls back to what OCR read.
--
-- "Not read yet"  = no row for the image.
-- "Read, no text" = a row whose `text` is ''.
-- "Has text"      = a row with non-empty `text`.
--
-- Additive: no existing table changes. A binary older than this migration
-- refuses to open a library that has it (see `upgrade::run`).
CREATE TABLE captions (
    image_hash TEXT NOT NULL REFERENCES images(hash) ON DELETE CASCADE,
    source     TEXT NOT NULL CHECK (source IN ('ocr', 'manual')),
    -- Normalized by `CaptionText::normalize`: single-spaced, at most 1000 chars.
    text       TEXT NOT NULL,
    -- Recognizer tag such as 'vision:1'. Required for ocr rows (it says when a
    -- row is stale and should be read again), NULL for manual rows.
    engine     TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (image_hash, source),
    CHECK ((source = 'ocr') = (engine IS NOT NULL))
) WITHOUT ROWID;
