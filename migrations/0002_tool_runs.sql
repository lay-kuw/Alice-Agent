-- Tool execution log. No arguments or results: those stay in the thread's
-- messages and disappear with it. Rows of a thread are deleted with it.
CREATE TABLE tool_runs (
    id          INTEGER PRIMARY KEY,
    thread_id   TEXT REFERENCES threads(id) ON DELETE CASCADE,
    skill       TEXT NOT NULL,
    tool        TEXT NOT NULL,
    source      TEXT NOT NULL,   -- native / mcp / text
    status      TEXT NOT NULL,   -- ok / error / denied / declined / cancelled / unavailable
    duration_ms INTEGER NOT NULL,
    error       TEXT,
    created_at  INTEGER NOT NULL
);
CREATE INDEX tool_runs_time ON tool_runs(created_at);
CREATE INDEX tool_runs_thread ON tool_runs(thread_id);
