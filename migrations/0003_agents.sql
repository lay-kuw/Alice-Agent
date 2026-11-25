-- Scheduled agents: an instruction Alice runs on her own at a set time
-- (and then every `every_min` minutes), replying in a chat thread.
CREATE TABLE agents (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    prompt      TEXT NOT NULL,
    -- NULL: a thread is made on the next run and kept.
    thread_id   TEXT REFERENCES threads(id) ON DELETE SET NULL,
    start_at    INTEGER NOT NULL,          -- first run (unix ms)
    every_min   INTEGER NOT NULL DEFAULT 0, -- 0 = once
    enabled     INTEGER NOT NULL DEFAULT 1,
    next_run    INTEGER,                   -- NULL once a one-off has run
    last_run    INTEGER,
    last_status TEXT,
    created_at  INTEGER NOT NULL
);
CREATE INDEX agents_next ON agents(next_run);
