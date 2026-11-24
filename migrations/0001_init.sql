-- Alice Agent schema v1.
-- Only conversation-derived data lives here. Settings and credentials are in
-- config.toml / secrets.toml, so wiping these tables is a complete reset.

CREATE TABLE threads (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    model       TEXT,                       -- NULL = server default model
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE messages (
    id            TEXT PRIMARY KEY,
    thread_id     TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,          -- ordering within the thread
    role          TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
    content       TEXT NOT NULL DEFAULT '',
    tool_calls    TEXT,                      -- JSON array (assistant)
    tool_call_id  TEXT,                      -- tool result -> call id
    status        TEXT NOT NULL DEFAULT 'complete'
                  CHECK (status IN ('complete', 'streaming', 'cancelled', 'error')),
    client        TEXT NOT NULL DEFAULT '',  -- cli / web / ios / telegram / discord
    model         TEXT,
    provider_state TEXT,                     -- provider-specific replay data (JSON)
    meta          TEXT,                      -- timings / usage (JSON)
    created_at    INTEGER NOT NULL
);
CREATE INDEX messages_thread ON messages(thread_id, seq);

-- Cross-thread memory. Content is derived from memory_sources.
CREATE TABLE memories (
    id          INTEGER PRIMARY KEY,
    key         TEXT NOT NULL UNIQUE,
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Every memory is backed by one or more user messages.
CREATE TABLE memory_sources (
    memory_id   INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    message_id  TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    thread_id   TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,              -- the fact as stated in this source
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (memory_id, message_id)
);
CREATE INDEX memory_sources_thread ON memory_sources(thread_id);
CREATE INDEX memory_sources_message ON memory_sources(message_id);

-- Which assistant messages had which memories injected (provenance / audit).
CREATE TABLE memory_uses (
    message_id  TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    memory_id   INTEGER NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY (message_id, memory_id)
);
CREATE INDEX memory_uses_memory ON memory_uses(memory_id);

-- Trigram tokenizer handles Japanese and other unsegmented scripts.
CREATE VIRTUAL TABLE memories_fts USING fts5(
    key, content, content='memories', content_rowid='id', tokenize='trigram'
);
CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, key, content) VALUES (new.id, new.key, new.content);
END;
CREATE TRIGGER memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, key, content) VALUES ('delete', old.id, old.key, old.content);
END;
CREATE TRIGGER memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, key, content) VALUES ('delete', old.id, old.key, old.content);
    INSERT INTO memories_fts(rowid, key, content) VALUES (new.id, new.key, new.content);
END;

CREATE TABLE attachments (
    id          TEXT PRIMARY KEY,
    thread_id   TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    message_id  TEXT REFERENCES messages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX attachments_thread ON attachments(thread_id);

-- Per-chat state for Telegram / Discord (current thread selection).
CREATE TABLE channel_state (
    channel     TEXT NOT NULL,              -- telegram / discord
    chat_id     TEXT NOT NULL,
    thread_id   TEXT REFERENCES threads(id) ON DELETE SET NULL,
    model       TEXT,
    PRIMARY KEY (channel, chat_id)
);

-- Messages a bot posted to an external platform, so they can be removed
-- (best effort) when the thread is deleted.
CREATE TABLE channel_messages (
    thread_id   TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    channel     TEXT NOT NULL,
    chat_id     TEXT NOT NULL,
    external_id TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX channel_messages_thread ON channel_messages(thread_id);
