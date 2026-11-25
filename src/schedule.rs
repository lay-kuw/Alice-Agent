//! Scheduled agents: instructions Alice runs on her own at a set time, and
//! then every N minutes, posting into a chat thread like a message from the
//! user (client "agent"). Missed runs are not replayed: after downtime the
//! next run is the next slot from now.

use crate::agent::{self, TurnError, TurnInput};
use crate::app::{App, AppRef};
use crate::storage::{self, DbResult};
use crate::util::{new_id, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::json;
use std::time::Duration;

/// Shortest repeat, so a typo cannot run the model every minute all day.
pub const MIN_EVERY_MIN: i64 = 5;
const TICK: Duration = Duration::from_secs(20);
/// A thread that is busy is tried again this much later.
const BUSY_RETRY_MS: i64 = 60_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub start_at: i64,
    pub every_min: i64,
    pub enabled: bool,
    pub next_run: Option<i64>,
    pub last_run: Option<i64>,
    pub last_status: Option<String>,
    pub created_at: i64,
}

const COLS: &str = "a.id, a.name, a.prompt, a.thread_id, t.title, a.start_at, a.every_min, a.enabled, a.next_run, a.last_run, a.last_status, a.created_at";

fn row(r: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: r.get(0)?,
        name: r.get(1)?,
        prompt: r.get(2)?,
        thread_id: r.get(3)?,
        thread_title: r.get(4)?,
        start_at: r.get(5)?,
        every_min: r.get(6)?,
        enabled: r.get::<_, i64>(7)? != 0,
        next_run: r.get(8)?,
        last_run: r.get(9)?,
        last_status: r.get(10)?,
        created_at: r.get(11)?,
    })
}

pub fn list(c: &Connection) -> DbResult<Vec<Agent>> {
    let mut st = c.prepare_cached(&format!("SELECT {COLS} FROM agents a LEFT JOIN threads t ON t.id = a.thread_id ORDER BY a.created_at"))?;
    st.query_map([], row)?.collect()
}

pub fn get(c: &Connection, id: &str) -> DbResult<Option<Agent>> {
    c.query_row(&format!("SELECT {COLS} FROM agents a LEFT JOIN threads t ON t.id = a.thread_id WHERE a.id = ?1"), [id], row).optional()
}

/// The first slot at or after `now`: `start_at`, then every `every_min` minutes.
/// A one-off whose time has passed is due now (it has not run yet).
pub fn next_slot(start_at: i64, every_min: i64, now: i64) -> i64 {
    if start_at >= now || every_min <= 0 {
        return start_at;
    }
    let step = every_min * 60_000;
    start_at + (now - start_at + step - 1) / step * step
}

/// What a client sends to create or change an agent.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Fields {
    pub name: Option<String>,
    pub prompt: Option<String>,
    /// "" or null: a new thread on the next run.
    pub thread_id: Option<Option<String>>,
    pub start_at: Option<i64>,
    pub every_min: Option<i64>,
    pub enabled: Option<bool>,
}

pub fn save(c: &Connection, id: Option<&str>, f: Fields) -> Result<Agent, String> {
    let now = now_ms();
    let cur = match id {
        Some(id) => Some(get(c, id).map_err(|e| e.to_string())?.ok_or("not found")?),
        None => None,
    };
    let name = f.name.or(cur.as_ref().map(|a| a.name.clone())).unwrap_or_default().trim().to_string();
    let prompt = f.prompt.or(cur.as_ref().map(|a| a.prompt.clone())).unwrap_or_default().trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err("name must be 1 to 100 characters".into());
    }
    if prompt.is_empty() || prompt.len() > 20_000 {
        return Err("instructions must be 1 to 20000 bytes".into());
    }
    let thread_id = match f.thread_id {
        Some(t) => t.filter(|t| !t.is_empty()),
        None => cur.as_ref().and_then(|a| a.thread_id.clone()),
    };
    if let Some(t) = &thread_id
        && storage::get_thread(c, t).map_err(|e| e.to_string())?.is_none() {
            return Err("thread not found".into());
        }
    let start_at = f.start_at.or(cur.as_ref().map(|a| a.start_at)).unwrap_or(now);
    let every_min = f.every_min.or(cur.as_ref().map(|a| a.every_min)).unwrap_or(0);
    if every_min != 0 && !(MIN_EVERY_MIN..=60 * 24 * 366).contains(&every_min) {
        return Err(format!("repeat every {MIN_EVERY_MIN} minutes to a year, or 0 for once"));
    }
    let enabled = f.enabled.or(cur.as_ref().map(|a| a.enabled)).unwrap_or(true);
    // A changed schedule starts over; otherwise the planned run stays.
    let reschedule = cur.as_ref().is_none_or(|a| a.start_at != start_at || a.every_min != every_min || (!a.enabled && enabled));
    let next_run = if reschedule { Some(next_slot(start_at, every_min, now)) } else { cur.as_ref().and_then(|a| a.next_run) };
    let id = match cur {
        Some(a) => {
            c.execute(
                "UPDATE agents SET name = ?2, prompt = ?3, thread_id = ?4, start_at = ?5, every_min = ?6, enabled = ?7, next_run = ?8 WHERE id = ?1",
                params![a.id, name, prompt, thread_id, start_at, every_min, enabled as i64, next_run],
            )
            .map_err(|e| e.to_string())?;
            a.id
        }
        None => {
            let id = new_id();
            c.execute(
                "INSERT INTO agents (id, name, prompt, thread_id, start_at, every_min, enabled, next_run, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, name, prompt, thread_id, start_at, every_min, enabled as i64, next_run, now],
            )
            .map_err(|e| e.to_string())?;
            id
        }
    };
    get(c, &id).map_err(|e| e.to_string())?.ok_or_else(|| "not found".into())
}

pub fn delete(c: &Connection, id: &str) -> DbResult<bool> {
    Ok(c.execute("DELETE FROM agents WHERE id = ?1", [id])? > 0)
}

/// Run the scheduler for as long as the server runs.
pub fn start(app: &AppRef) {
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            tick(&app, now_ms());
        }
    });
}

/// Start every agent that is due at `now`. Returns how many started.
pub fn tick(app: &AppRef, now: i64) -> usize {
    let due: Vec<Agent> = {
        let c = app.db.lock();
        match list(&c) {
            Ok(all) => all.into_iter().filter(|a| a.enabled && a.next_run.is_some_and(|n| n <= now)).collect(),
            Err(e) => {
                crate::error!("agents: {e}");
                return 0;
            }
        }
    };
    due.iter().filter(|a| run(app, a, now, true).is_ok()).count()
}

/// Post the agent's instruction into its thread and let Alice answer.
/// `advance` moves the schedule on (a manual "run now" leaves it alone).
pub fn run(app: &AppRef, a: &Agent, now: i64, advance: bool) -> Result<String, String> {
    let thread_id = match ensure_thread(app, a) {
        Ok(t) => t,
        Err(e) => {
            record(app, a, now, advance, Some(format!("error: {e}")));
            return Err(e);
        }
    };
    let started = agent::start_turn(
        app,
        TurnInput { thread_id: thread_id.clone(), text: a.prompt.clone(), client: "agent".into(), model: None, attachment_ids: vec![] },
    );
    match started {
        Ok(_) => {
            crate::info!("agent {} started", a.id);
            record(app, a, now, advance, Some("ok".into()));
            app.emit(json!({ "type": "agents.updated" }));
            Ok(thread_id)
        }
        Err(TurnError::Busy) if advance => {
            // Someone is talking in that thread; try again shortly.
            let _ = app.db.lock().execute("UPDATE agents SET next_run = ?2, last_status = 'busy' WHERE id = ?1", params![a.id, now + BUSY_RETRY_MS]);
            Err("busy".into())
        }
        Err(e) => {
            crate::warn!("agent {} failed: {e}", a.id);
            record(app, a, now, advance, Some(format!("error: {e}")));
            Err(e.to_string())
        }
    }
}

fn record(app: &App, a: &Agent, now: i64, advance: bool, status: Option<String>) {
    let c = app.db.lock();
    let next = if a.every_min > 0 { Some(next_slot(a.start_at, a.every_min, now + 1)) } else { None };
    let r = if advance {
        c.execute("UPDATE agents SET last_run = ?2, last_status = ?3, next_run = ?4 WHERE id = ?1", params![a.id, now, status, next])
    } else {
        c.execute("UPDATE agents SET last_run = ?2, last_status = ?3 WHERE id = ?1", params![a.id, now, status])
    };
    if let Err(e) = r {
        crate::error!("agents: {e}");
    }
}

/// The agent's thread, made (and remembered) on first use.
fn ensure_thread(app: &App, a: &Agent) -> Result<String, String> {
    if let Some(t) = &a.thread_id {
        return Ok(t.clone());
    }
    let t = {
        let c = app.db.lock();
        let t = storage::create_thread(&c, &a.name, None).map_err(|e| e.to_string())?;
        c.execute("UPDATE agents SET thread_id = ?2 WHERE id = ?1", params![a.id, t.id]).map_err(|e| e.to_string())?;
        t
    };
    app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
    Ok(t.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        let m = 60_000;
        // Future start: that time.
        assert_eq!(next_slot(10 * m, 60, 5 * m), 10 * m);
        // Past start: the next slot on the grid, never a catch-up.
        assert_eq!(next_slot(0, 60, 61 * m), 120 * m);
        assert_eq!(next_slot(0, 60, 60 * m), 60 * m);
        // A one-off in the past is due now.
        assert_eq!(next_slot(0, 0, 99 * m), 0);
    }

    #[test]
    fn save_validates_and_schedules() {
        let db = crate::storage::Db::open_in_memory().unwrap();
        let c = db.lock();
        assert!(save(&c, None, Fields { name: Some("x".into()), prompt: Some("".into()), ..Default::default() }).is_err());
        assert!(save(&c, None, Fields { name: Some("x".into()), prompt: Some("hi".into()), every_min: Some(1), ..Default::default() }).is_err());
        let start = now_ms() + 3_600_000;
        let a = save(&c, None, Fields { name: Some("Morning".into()), prompt: Some("news".into()), start_at: Some(start), every_min: Some(1440), ..Default::default() }).unwrap();
        assert_eq!(a.next_run, Some(start));
        assert!(a.enabled && a.thread_id.is_none());
        // Renaming keeps the planned run.
        let b = save(&c, Some(&a.id), Fields { name: Some("Morning news".into()), ..Default::default() }).unwrap();
        assert_eq!((b.name.as_str(), b.next_run), ("Morning news", Some(start)));
        assert!(delete(&c, &a.id).unwrap());
        assert!(list(&c).unwrap().is_empty());
    }
}
