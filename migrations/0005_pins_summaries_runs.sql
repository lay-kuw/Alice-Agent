-- Starred threads stay at the top of the list.
ALTER TABLE threads ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
-- A short summary of the history that no longer fits the model's window,
-- covering messages up to summary_seq.
ALTER TABLE threads ADD COLUMN summary TEXT;
ALTER TABLE threads ADD COLUMN summary_seq INTEGER NOT NULL DEFAULT 0;
-- The last runs of each scheduled agent (kept to ten per agent).
CREATE TABLE agent_runs (
    id          INTEGER PRIMARY KEY,
    agent_id    TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER,
    status      TEXT NOT NULL,   -- running / ok / quiet / waiting / error / stopped / busy
    error       TEXT,
    thread_id   TEXT,
    manual      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX agent_runs_agent ON agent_runs(agent_id, started_at);
