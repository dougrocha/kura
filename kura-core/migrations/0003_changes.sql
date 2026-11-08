-- Append-only log of every edit, written in the same transaction as the edit
-- (see `store::WriteTx::record`). It is the source for a future live feed
-- (`Library::watch`, SSE with Last-Event-ID = seq) and doubles as an audit trail.
CREATE TABLE changes (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    image_hash TEXT NOT NULL,
    detail TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
