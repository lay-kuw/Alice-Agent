//! SQLite storage. A single connection guarded by a mutex: personal use has
//! one writer at a time anyway, and serialised access keeps the deletion /
//! generation race reasoning simple. Never hold the lock across `.await`.

use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_init.sql"),
    include_str!("../../migrations/0002_tool_runs.sql"),
    include_str!("../../migrations/0003_agents.sql"),
];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

pub type DbResult<T> = Result<T, rusqlite::Error>;

impl Db {
    pub fn open(path: &Path) -> DbResult<Db> {
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn)
    }

    pub fn open_in_memory() -> DbResult<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> DbResult<Db> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA secure_delete = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!(
                "BEGIN; {sql}\nPRAGMA user_version = {}; COMMIT;",
                i + 1
            ))?;
        }
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        // A poisoned lock only means another task panicked mid-call; the
        // connection itself is still valid (transactions roll back on drop).
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub model: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Message {
    pub id: String,
    pub thread_id: String,
    pub seq: i64,
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub status: String,
    pub client: String,
    pub model: Option<String>,
    #[serde(skip)]
    pub provider_state: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    pub created_at: i64,
}

pub struct NewMessage<'a> {
    pub thread_id: &'a str,
    pub role: &'a str,
    pub content: &'a str,
    pub tool_calls: Option<&'a Value>,
    pub tool_call_id: Option<&'a str>,
    pub status: &'a str,
    pub client: &'a str,
    pub model: Option<&'a str>,
}

fn row_thread(r: &rusqlite::Row) -> rusqlite::Result<Thread> {
    Ok(Thread {
        id: r.get(0)?,
        title: r.get(1)?,
        model: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

fn json_col(s: Option<String>) -> Option<Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

const MSG_COLS: &str = "id, thread_id, seq, role, content, tool_calls, tool_call_id, status, client, model, provider_state, meta, created_at";

fn row_message(r: &rusqlite::Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: r.get(0)?,
        thread_id: r.get(1)?,
        seq: r.get(2)?,
        role: r.get(3)?,
        content: r.get(4)?,
        tool_calls: json_col(r.get(5)?),
        tool_call_id: r.get(6)?,
        status: r.get(7)?,
        client: r.get(8)?,
        model: r.get(9)?,
        provider_state: json_col(r.get(10)?),
        meta: json_col(r.get(11)?),
        created_at: r.get(12)?,
    })
}

pub fn create_thread(c: &Connection, title: &str, model: Option<&str>) -> DbResult<Thread> {
    let now = now_ms();
    let t = Thread {
        id: new_id(),
        title: title.to_string(),
        model: model.map(str::to_string),
        created_at: now,
        updated_at: now,
    };
    c.execute(
        "INSERT INTO threads (id, title, model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![t.id, t.title, t.model, t.created_at, t.updated_at],
    )?;
    Ok(t)
}

pub fn list_threads(c: &Connection) -> DbResult<Vec<Thread>> {
    let mut st = c.prepare_cached(
        "SELECT id, title, model, created_at, updated_at FROM threads ORDER BY updated_at DESC",
    )?;
    st.query_map([], row_thread)?.collect()
}

pub fn get_thread(c: &Connection, id: &str) -> DbResult<Option<Thread>> {
    c.query_row(
        "SELECT id, title, model, created_at, updated_at FROM threads WHERE id = ?1",
        [id],
        row_thread,
    )
    .optional()
}

/// Resolve a full ID or a unique prefix (min 4 chars) to a thread ID.
pub fn resolve_thread_id(c: &Connection, id_or_prefix: &str) -> DbResult<Option<String>> {
    if let Some(t) = get_thread(c, id_or_prefix)? {
        return Ok(Some(t.id));
    }
    if id_or_prefix.len() < 4 || !id_or_prefix.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    let mut st = c.prepare_cached("SELECT id FROM threads WHERE id LIKE ?1 || '%' LIMIT 2")?;
    let ids: Vec<String> = st.query_map([id_or_prefix], |r| r.get(0))?.collect::<DbResult<_>>()?;
    Ok(if ids.len() == 1 { ids.into_iter().next() } else { None })
}

pub fn update_thread(
    c: &Connection,
    id: &str,
    title: Option<&str>,
    model: Option<Option<&str>>,
) -> DbResult<Option<Thread>> {
    if let Some(t) = title {
        c.execute("UPDATE threads SET title = ?2, updated_at = ?3 WHERE id = ?1", params![id, t, now_ms()])?;
    }
    if let Some(m) = model {
        c.execute("UPDATE threads SET model = ?2, updated_at = ?3 WHERE id = ?1", params![id, m, now_ms()])?;
    }
    get_thread(c, id)
}

pub fn touch_thread(c: &Connection, id: &str) -> DbResult<()> {
    c.execute("UPDATE threads SET updated_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
    Ok(())
}

pub fn insert_message(c: &Connection, m: &NewMessage) -> DbResult<Message> {
    let id = new_id();
    let now = now_ms();
    let seq: i64 = c.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE thread_id = ?1",
        [m.thread_id],
        |r| r.get(0),
    )?;
    let tool_calls = m.tool_calls.map(|v| v.to_string());
    c.execute(
        "INSERT INTO messages (id, thread_id, seq, role, content, tool_calls, tool_call_id, status, client, model, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![id, m.thread_id, seq, m.role, m.content, tool_calls, m.tool_call_id, m.status, m.client, m.model, now],
    )?;
    Ok(Message {
        id,
        thread_id: m.thread_id.to_string(),
        seq,
        role: m.role.to_string(),
        content: m.content.to_string(),
        tool_calls: m.tool_calls.cloned(),
        tool_call_id: m.tool_call_id.map(str::to_string),
        status: m.status.to_string(),
        client: m.client.to_string(),
        model: m.model.map(str::to_string),
        provider_state: None,
        meta: None,
        created_at: now,
    })
}

pub struct MessageUpdate<'a> {
    pub content: &'a str,
    pub tool_calls: Option<&'a Value>,
    pub status: &'a str,
    pub provider_state: Option<&'a Value>,
    pub meta: Option<&'a Value>,
}

/// Returns false if the message no longer exists (thread deleted meanwhile).
pub fn update_message(c: &Connection, id: &str, u: &MessageUpdate) -> DbResult<bool> {
    let n = c.execute(
        "UPDATE messages SET content = ?2, tool_calls = ?3, status = ?4, provider_state = ?5, meta = ?6 WHERE id = ?1",
        params![
            id,
            u.content,
            u.tool_calls.map(|v| v.to_string()),
            u.status,
            u.provider_state.map(|v| v.to_string()),
            u.meta.map(|v| v.to_string()),
        ],
    )?;
    Ok(n == 1)
}

pub fn list_messages(c: &Connection, thread_id: &str) -> DbResult<Vec<Message>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {MSG_COLS} FROM messages WHERE thread_id = ?1 ORDER BY seq"
    ))?;
    st.query_map([thread_id], row_message)?.collect()
}

pub fn get_message(c: &Connection, id: &str) -> DbResult<Option<Message>> {
    c.query_row(&format!("SELECT {MSG_COLS} FROM messages WHERE id = ?1"), [id], row_message)
        .optional()
}

/// Mark generations interrupted by a crash or restart as cancelled.
pub fn recover_streaming(c: &Connection) -> DbResult<usize> {
    c.execute("UPDATE messages SET status = 'cancelled' WHERE status = 'streaming'", [])
}

#[derive(Serialize)]
pub struct Counts {
    pub threads: i64,
    pub messages: i64,
    pub memories: i64,
    pub attachments: i64,
}

pub fn counts(c: &Connection) -> DbResult<Counts> {
    let q = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
    Ok(Counts {
        threads: q("SELECT COUNT(*) FROM threads")?,
        messages: q("SELECT COUNT(*) FROM messages")?,
        memories: q("SELECT COUNT(*) FROM memories")?,
        attachments: q("SELECT COUNT(*) FROM attachments")?,
    })
}

/// Flush WAL into the main file and truncate it so deleted content does not
/// linger in the write-ahead log.
pub fn checkpoint(c: &Connection) -> DbResult<()> {
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(())
}

/// Merge FTS segments so deleted terms are physically removed from the index.
pub fn optimize_fts(c: &Connection) -> DbResult<()> {
    c.execute("INSERT INTO memories_fts(memories_fts) VALUES ('optimize')", [])?;
    Ok(())
}

pub fn vacuum(c: &Connection) -> DbResult<()> {
    c.execute_batch("VACUUM")?;
    checkpoint(c)
}

pub fn db_size_bytes(path: &Path) -> u64 {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let p = format!("{}{}", path.display(), suffix);
        total += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    }
    total
}

/// Tokens reported by providers for assistant messages since `since_ms`,
/// per model: (model, input, output, responses). Deleted threads are not counted.
pub fn token_usage(c: &Connection, since_ms: i64) -> rusqlite::Result<Vec<(String, i64, i64, i64)>> {
    let mut st = c.prepare_cached(
        "SELECT COALESCE(model, ''),
                SUM(COALESCE(json_extract(meta, '$.usage.input_tokens'), 0)),
                SUM(COALESCE(json_extract(meta, '$.usage.output_tokens'), 0)),
                COUNT(*)
         FROM messages
         WHERE role = 'assistant' AND created_at >= ?1 AND json_valid(meta)
         GROUP BY 1 ORDER BY 3 DESC",
    )?;
    st.query_map([since_ms], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()
}

/// Recent tool runs, newest first.
pub fn tool_runs(c: &Connection, limit: usize) -> rusqlite::Result<Vec<Value>> {
    let mut st = c.prepare_cached(
        "SELECT r.created_at, r.thread_id, t.title, r.skill, r.tool, r.source, r.status, r.duration_ms, r.error
         FROM tool_runs r LEFT JOIN threads t ON t.id = r.thread_id ORDER BY r.id DESC LIMIT ?1",
    )?;
    st.query_map([limit as i64], |r| {
        Ok(serde_json::json!({
            "ts": r.get::<_, i64>(0)?, "thread_id": r.get::<_, Option<String>>(1)?, "thread_title": r.get::<_, Option<String>>(2)?,
            "skill": r.get::<_, String>(3)?, "tool": r.get::<_, String>(4)?, "source": r.get::<_, String>(5)?,
            "status": r.get::<_, String>(6)?, "duration_ms": r.get::<_, i64>(7)?, "error": r.get::<_, Option<String>>(8)?,
        }))
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_and_message_crud() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        let t = create_thread(&c, "hello", None).unwrap();
        let m = insert_message(
            &c,
            &NewMessage {
                thread_id: &t.id,
                role: "user",
                content: "hi",
                tool_calls: None,
                tool_call_id: None,
                status: "complete",
                client: "cli",
                model: None,
            },
        )
        .unwrap();
        assert_eq!(m.seq, 1);
        assert_eq!(list_messages(&c, &t.id).unwrap().len(), 1);
        assert_eq!(resolve_thread_id(&c, &t.id[..6]).unwrap().as_deref(), Some(t.id.as_str()));
        c.execute("DELETE FROM threads WHERE id = ?1", [&t.id]).unwrap();
        assert!(get_message(&c, &m.id).unwrap().is_none());
        // Inserting into a deleted thread must fail (FK).
        let r = insert_message(
            &c,
            &NewMessage {
                thread_id: &t.id,
                role: "user",
                content: "late",
                tool_calls: None,
                tool_call_id: None,
                status: "complete",
                client: "cli",
                model: None,
            },
        );
        assert!(r.is_err());
    }
}
