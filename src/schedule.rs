//! Scheduled agents: instructions Alice runs on her own, either every N
//! minutes or at set times of day, replying in a chat thread.
//!
//! The instruction is posted as a hidden user message (client "agent"), so the
//! reply reads as Alice speaking up on her own. When the instruction says to
//! stay quiet if there is nothing to report and Alice answers [`SILENT`], the
//! run leaves no trace in the chat. Missed runs are not replayed: after
//! downtime the next run is the next slot from now.

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
/// Alice's whole reply when there is nothing to report.
pub const SILENT: &str = "NO_UPDATE";
const TICK: Duration = Duration::from_secs(20);
/// A thread that is busy is tried again this much later.
const BUSY_RETRY_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub start_at: i64,
    /// Repeat interval; 0 with no `times` = once.
    pub every_min: i64,
    /// Daily times "HH:MM" (server local time); when set, `every_min` is unused.
    pub times: Vec<String>,
    pub enabled: bool,
    pub next_run: Option<i64>,
    pub last_run: Option<i64>,
    pub last_status: Option<String>,
    pub created_at: i64,
}

const COLS: &str = "a.id, a.name, a.prompt, a.thread_id, t.title, a.start_at, a.every_min, a.times, a.enabled, a.next_run, a.last_run, a.last_status, a.created_at";

fn row(r: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: r.get(0)?,
        name: r.get(1)?,
        prompt: r.get(2)?,
        thread_id: r.get(3)?,
        thread_title: r.get(4)?,
        start_at: r.get(5)?,
        every_min: r.get(6)?,
        times: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
        enabled: r.get::<_, i64>(8)? != 0,
        next_run: r.get(9)?,
        last_run: r.get(10)?,
        last_status: r.get(11)?,
        created_at: r.get(12)?,
    })
}

pub fn list(c: &Connection) -> DbResult<Vec<Agent>> {
    let mut st = c.prepare_cached(&format!("SELECT {COLS} FROM agents a LEFT JOIN threads t ON t.id = a.thread_id ORDER BY a.created_at"))?;
    st.query_map([], row)?.collect()
}

pub fn get(c: &Connection, id: &str) -> DbResult<Option<Agent>> {
    c.query_row(&format!("SELECT {COLS} FROM agents a LEFT JOIN threads t ON t.id = a.thread_id WHERE a.id = ?1"), [id], row).optional()
}

/// The first interval slot at or after `now`: `start_at`, then every `every_min` minutes.
/// A one-off whose time has passed is due now (it has not run yet).
pub fn next_slot(start_at: i64, every_min: i64, now: i64) -> i64 {
    if start_at >= now || every_min <= 0 {
        return start_at;
    }
    let step = every_min * 60_000;
    start_at + (now - start_at + step - 1) / step * step
}

/// Minutes after midnight for "HH:MM".
fn minutes(t: &str) -> Option<i64> {
    let (h, m) = t.split_once(':')?;
    let (h, m): (i64, i64) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 60 + m)
}

/// The server's UTC offset (ms) at `at`, daylight saving included (SQLite asks the OS).
pub fn local_offset(c: &Connection, at: i64) -> i64 {
    c.query_row(
        "SELECT CAST(ROUND((julianday(?1 / 1000.0, 'unixepoch', 'localtime') - julianday(?1 / 1000.0, 'unixepoch')) * 86400) AS INTEGER)",
        [at],
        |r| r.get::<_, i64>(0),
    )
    .map(|s| s * 1000)
    .unwrap_or(0)
}

/// The first of the daily `times` (local) at or after `now`.
pub fn next_daily(times: &[String], now: i64, offset: impl Fn(i64) -> i64) -> Option<i64> {
    let mins: Vec<i64> = times.iter().filter_map(|t| minutes(t)).collect();
    let local_now = now + offset(now);
    let day = local_now.div_euclid(DAY_MS) * DAY_MS;
    (0..3)
        .flat_map(|d| mins.iter().map(move |m| day + d * DAY_MS + m * 60_000))
        .map(|local| local - offset(local - offset(now)))
        .filter(|&at| at >= now)
        .min()
}

fn next_run(c: &Connection, a: &Agent, now: i64) -> Option<i64> {
    if !a.times.is_empty() {
        return next_daily(&a.times, now, |at| local_offset(c, at));
    }
    Some(next_slot(a.start_at, a.every_min, now))
}

/// What a client sends to create or change an agent.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Fields {
    pub name: Option<String>,
    pub prompt: Option<String>,
    /// Left out: unchanged. "" or null: a new thread on the next run.
    #[serde(default, deserialize_with = "present")]
    pub thread_id: Option<Option<String>>,
    /// Interval mode: the first run is as soon as possible, then every N minutes.
    pub every_min: Option<i64>,
    /// Times mode: every day at these local times.
    pub times: Option<Vec<String>>,
    pub enabled: Option<bool>,
    /// Older clients: when the interval starts (defaults to now).
    pub start_at: Option<i64>,
}

/// Tells a field that is present (even as null) from one left out:
/// serde would read both as None.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(d).map(Some)
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
    let mut times = f.times.or(cur.as_ref().map(|a| a.times.clone())).unwrap_or_default();
    if times.len() > 24 || times.iter().any(|t| minutes(t).is_none()) {
        return Err("times must be up to 24 of HH:MM".into());
    }
    times = times.iter().filter_map(|t| minutes(t)).map(|m| format!("{:02}:{:02}", m / 60, m % 60)).collect();
    times.sort();
    times.dedup();
    let every_min = if times.is_empty() { f.every_min.or(cur.as_ref().map(|a| a.every_min)).unwrap_or(0) } else { 0 };
    if every_min != 0 && !(MIN_EVERY_MIN..=60 * 24 * 366).contains(&every_min) {
        return Err(format!("repeat every {MIN_EVERY_MIN} minutes to a year"));
    }
    let enabled = f.enabled.or(cur.as_ref().map(|a| a.enabled)).unwrap_or(true);
    // A changed schedule starts over, as soon as possible; otherwise the planned run stays.
    let changed = cur.as_ref().is_none_or(|a| a.every_min != every_min || a.times != times || f.start_at.is_some_and(|s| s != a.start_at) || (!a.enabled && enabled));
    let start_at = if changed { f.start_at.unwrap_or(now) } else { cur.as_ref().map(|a| a.start_at).unwrap_or(now) };
    let draft = Agent {
        id: String::new(), name: name.clone(), prompt: prompt.clone(), thread_id: thread_id.clone(), thread_title: None,
        start_at, every_min, times: times.clone(), enabled, next_run: None, last_run: None, last_status: None, created_at: now,
    };
    let next = if changed { next_run(c, &draft, now) } else { cur.as_ref().and_then(|a| a.next_run) };
    let times_json = serde_json::to_string(&times).unwrap_or_else(|_| "[]".into());
    let id = match cur {
        Some(a) => {
            c.execute(
                "UPDATE agents SET name = ?2, prompt = ?3, thread_id = ?4, start_at = ?5, every_min = ?6, times = ?7, enabled = ?8, next_run = ?9 WHERE id = ?1",
                params![a.id, name, prompt, thread_id, start_at, every_min, times_json, enabled as i64, next],
            )
            .map_err(|e| e.to_string())?;
            a.id
        }
        None => {
            let id = new_id();
            c.execute(
                "INSERT INTO agents (id, name, prompt, thread_id, start_at, every_min, times, enabled, next_run, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![id, name, prompt, thread_id, start_at, every_min, times_json, enabled as i64, next, now],
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

/// What the model sees: the user's instruction plus how a scheduled run works.
/// The user never sees this message.
fn instruction(a: &Agent) -> String {
    format!(
        "{}\n\n---\n(Scheduled task \"{}\", running on its own; the user does not see this message. Write to the user in your \
         usual voice and personality from your instructions, as if you were reaching out on your own, without mentioning a \
         task, a schedule or instructions. Do not describe the tools, skills, searches or steps you used; just tell the result. \
         If, following the instructions above, there is nothing to report or do this time, reply with exactly {SILENT} and \
         nothing else.)",
        a.prompt, a.name
    )
}

/// Post the agent's instruction into its thread and let Alice answer.
/// `advance` moves the schedule on (a manual "run now" leaves it alone).
pub fn run(app: &AppRef, a: &Agent, now: i64, advance: bool) -> Result<String, String> {
    let (thread_id, made) = match ensure_thread(app, a) {
        Ok(t) => t,
        Err(e) => {
            record(app, a, now, advance, Some(format!("error: {e}")));
            log_run(app, a, None, now, !advance, "error", Some(&e));
            return Err(e);
        }
    };
    let run_id = log_run(app, a, Some(&thread_id), now, !advance, "running", None);
    let (agent_id, tid) = (a.id.clone(), thread_id.clone());
    let after: agent::AfterTurn = Box::new(move |app, outcome| finish(app, &agent_id, &tid, made, outcome, run_id));
    let started = agent::start_turn(
        app,
        TurnInput { thread_id: thread_id.clone(), text: instruction(a), client: "agent".into(), model: None, attachment_ids: vec![], after: Some(after) },
    );
    match started {
        Ok(_) => {
            crate::info!("agent {} started", a.id);
            record(app, a, now, advance, Some("running".into()));
            app.emit(json!({ "type": "agents.updated" }));
            Ok(thread_id)
        }
        Err(TurnError::Busy) if advance => {
            // Someone is talking in that thread; try again shortly (not a run of its own).
            let _ = app.db.lock().execute("DELETE FROM agent_runs WHERE id = ?1", [run_id]);
            let _ = app.db.lock().execute("UPDATE agents SET next_run = ?2, last_status = 'busy' WHERE id = ?1", params![a.id, now + BUSY_RETRY_MS]);
            Err("busy".into())
        }
        Err(e) => {
            crate::warn!("agent {} failed: {e}", a.id);
            record(app, a, now, advance, Some(format!("error: {e}")));
            end_run(app, run_id, "error", Some(&e.to_string()), Some(&thread_id));
            Err(e.to_string())
        }
    }
}

/// How many runs are kept per agent.
pub const RUNS_KEPT: i64 = 10;

/// One run of an agent, for the run history.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: i64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// running / waiting (for a tool approval) / ok / quiet / error / stopped
    pub status: String,
    pub error: Option<String>,
    pub thread_id: Option<String>,
    pub thread_title: Option<String>,
    pub manual: bool,
}

fn log_run(app: &App, a: &Agent, thread_id: Option<&str>, now: i64, manual: bool, status: &str, error: Option<&str>) -> i64 {
    let c = app.db.lock();
    let finished = (status != "running").then_some(now);
    let id = c
        .execute(
            "INSERT INTO agent_runs (agent_id, started_at, finished_at, status, error, thread_id, manual) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![a.id, now, finished, status, error, thread_id, manual as i64],
        )
        .map(|_| c.last_insert_rowid())
        .unwrap_or(0);
    let _ = c.execute(
        "DELETE FROM agent_runs WHERE agent_id = ?1 AND id NOT IN (SELECT id FROM agent_runs WHERE agent_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![a.id, RUNS_KEPT],
    );
    id
}

fn end_run(app: &App, run_id: i64, status: &str, error: Option<&str>, thread_id: Option<&str>) {
    let _ = app.db.lock().execute(
        "UPDATE agent_runs SET status = ?2, error = ?3, thread_id = ?4, finished_at = ?5 WHERE id = ?1",
        params![run_id, status, error, thread_id, now_ms()],
    );
}

/// The last runs of an agent, newest first. A run waiting for a tool approval
/// says so (`waiting`).
pub fn runs(app: &App, agent_id: &str) -> DbResult<Vec<Run>> {
    let waiting: std::collections::HashSet<String> =
        app.approvals.lock().unwrap_or_else(|e| e.into_inner()).values().map(|p| p.thread_id.clone()).collect();
    let c = app.db.lock();
    let mut st = c.prepare_cached(
        "SELECT r.id, r.started_at, r.finished_at, r.status, r.error, r.thread_id, t.title, r.manual
         FROM agent_runs r LEFT JOIN threads t ON t.id = r.thread_id WHERE r.agent_id = ?1 ORDER BY r.id DESC LIMIT ?2",
    )?;
    st.query_map(params![agent_id, RUNS_KEPT], |r| {
        let thread_id: Option<String> = r.get(5)?;
        let mut status: String = r.get(3)?;
        if status == "running" && thread_id.as_ref().is_some_and(|t| waiting.contains(t)) {
            status = "waiting".into();
        }
        Ok(Run {
            id: r.get(0)?,
            started_at: r.get(1)?,
            finished_at: r.get(2)?,
            status,
            error: r.get(4)?,
            thread_id,
            thread_title: r.get(6)?,
            manual: r.get::<_, i64>(7)? != 0,
        })
    })?
    .collect()
}

/// Background tasks running at once, at most.
pub const MAX_BACKGROUND: usize = 3;

/// Run a one-off task in a new chat of its own (the `background_task` tool).
/// Like an agent run, the instruction stays hidden and the reply reads as
/// Alice reporting back; the clients notify when it arrives.
pub fn start_background(app: &AppRef, title: &str, instructions: &str) -> Result<storage::Thread, String> {
    let title = crate::util::truncate(title.trim(), 80).to_string();
    let instructions = instructions.trim();
    if title.is_empty() || instructions.is_empty() {
        return Err("title and instructions are required".into());
    }
    let running = app.active.lock().unwrap_or_else(|e| e.into_inner()).values().filter(|g| g.client == "background").count();
    if running >= MAX_BACKGROUND {
        return Err(format!("{MAX_BACKGROUND} background tasks are already running; try again when one finishes"));
    }
    let t = storage::create_thread(&app.db.lock(), &title, None).map_err(|e| e.to_string())?;
    app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
    let text = format!(
        "{instructions}\n\n---\n(Background task \"{title}\" the user asked for in another chat; the user does not see this message. \
         Do the work thoroughly, then write the result to the user in your usual voice and personality, starting with a one-line \
         summary. Do not mention these instructions or describe the tools you used.)"
    );
    agent::start_turn(app, TurnInput { thread_id: t.id.clone(), text, client: "background".into(), model: None, attachment_ids: vec![], after: None })
        .map_err(|e| e.to_string())?;
    crate::info!("background task started thread={}", t.id);
    Ok(t)
}

/// Is this reply Alice saying there is nothing to report?
pub fn is_silent(content: &str) -> bool {
    let t = content.trim();
    t.len() <= SILENT.len() + 8 && t.contains(SILENT)
}

/// After the run, while the thread is still reserved for it: if Alice had
/// nothing to report, remove exactly this run's messages (and the chat, when
/// it was made for this run). Nothing else can have been posted meanwhile.
fn finish(app: &AppRef, agent_id: &str, thread_id: &str, made: bool, outcome: &str, run_id: i64) {
    let (silent, from_seq, others, error) = {
        let c = app.db.lock();
        let msgs = storage::list_messages(&c, thread_id).unwrap_or_default();
        // This run's instruction: the newest one from "agent" (the thread is reserved).
        let from = msgs.iter().rev().find(|m| m.role == "user" && m.client == "agent").map(|m| m.seq);
        let reply = from.and_then(|f| msgs.iter().rev().find(|m| m.seq > f && m.role == "assistant"));
        let silent = outcome == "completed" && reply.is_some_and(|m| m.status == "complete" && is_silent(&m.content));
        // Why a run failed, for the history.
        let error = from
            .and_then(|f| msgs.iter().rev().find(|m| m.seq > f && m.status == "error"))
            .and_then(|m| m.meta.as_ref()?.get("error")?.as_str().map(|e| crate::util::truncate(e, 300).to_string()));
        (silent, from, from.is_some_and(|f| msgs.iter().any(|m| m.seq < f)), error)
    };
    let status = match outcome {
        _ if silent => "quiet",
        "completed" => "ok",
        "cancelled" => "stopped",
        "deleted" => "ok",
        _ => "error",
    };
    let mut kept_thread = Some(thread_id);
    if let (true, Some(from)) = (silent, from_seq) {
        if made && !others {
            let _ = crate::server::delete_thread_now(app, thread_id);
            kept_thread = None;
        } else {
            let _ = app.db.lock().execute("DELETE FROM messages WHERE thread_id = ?1 AND seq >= ?2", params![thread_id, from]);
            app.emit(json!({ "type": "resync", "thread_id": thread_id }));
        }
    }
    let _ = app.db.lock().execute("UPDATE agents SET last_status = ?2 WHERE id = ?1", params![agent_id, status]);
    let error = error.or_else(|| (status == "error").then(|| format!("the turn ended: {outcome}")));
    end_run(app, run_id, status, error.as_deref(), kept_thread);
    app.emit(json!({ "type": "agents.updated" }));
}

fn record(app: &App, a: &Agent, now: i64, advance: bool, status: Option<String>) {
    let c = app.db.lock();
    let next = if a.every_min > 0 || !a.times.is_empty() { next_run(&c, a, now + 1) } else { None };
    let r = if advance {
        c.execute("UPDATE agents SET last_run = ?2, last_status = ?3, next_run = ?4 WHERE id = ?1", params![a.id, now, status, next])
    } else {
        c.execute("UPDATE agents SET last_run = ?2, last_status = ?3 WHERE id = ?1", params![a.id, now, status])
    };
    if let Err(e) = r {
        crate::error!("agents: {e}");
    }
}

/// The agent's thread, made (and remembered) on first use. True when made now.
fn ensure_thread(app: &App, a: &Agent) -> Result<(String, bool), String> {
    if let Some(t) = &a.thread_id {
        return Ok((t.clone(), false));
    }
    let t = {
        let c = app.db.lock();
        let t = storage::create_thread(&c, &a.name, None).map_err(|e| e.to_string())?;
        c.execute("UPDATE agents SET thread_id = ?2 WHERE id = ?1", params![a.id, t.id]).map_err(|e| e.to_string())?;
        t
    };
    app.emit(json!({ "type": "thread.created", "thread_id": t.id, "thread": t }));
    Ok((t.id, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        let m = 60_000;
        assert_eq!(next_slot(10 * m, 60, 5 * m), 10 * m);
        assert_eq!(next_slot(0, 60, 61 * m), 120 * m);
        assert_eq!(next_slot(0, 60, 60 * m), 60 * m);
        assert_eq!(next_slot(0, 0, 99 * m), 0);
    }

    #[test]
    fn daily_times() {
        let h = 3_600_000;
        let jst = |_: i64| 9 * h; // UTC+9
        let times = vec!["12:00".to_string(), "18:00".to_string()];
        // 2026-01-01 10:00 JST = 01:00 UTC.
        let day_utc = 1_767_225_600_000; // 2026-01-01 00:00 UTC
        let now = day_utc + h;
        assert_eq!(next_daily(&times, now, jst), Some(day_utc + 3 * h)); // 12:00 JST
        assert_eq!(next_daily(&times, day_utc + 3 * h + 1, jst), Some(day_utc + 9 * h)); // then 18:00 JST
        // After 18:00 JST: tomorrow 12:00.
        assert_eq!(next_daily(&times, day_utc + 10 * h, jst), Some(day_utc + 27 * h));
        assert!(next_daily(&[], now, jst).is_none());
        assert!(is_silent(" NO_UPDATE ") && is_silent("NO_UPDATE.") && !is_silent("Here is NO_UPDATE news about many things"));
    }

    #[test]
    fn save_validates_and_schedules() {
        let db = crate::storage::Db::open_in_memory().unwrap();
        let c = db.lock();
        assert!(save(&c, None, Fields { name: Some("x".into()), prompt: Some("".into()), ..Default::default() }).is_err());
        assert!(save(&c, None, Fields { name: Some("x".into()), prompt: Some("hi".into()), every_min: Some(1), ..Default::default() }).is_err());
        assert!(save(&c, None, Fields { name: Some("x".into()), prompt: Some("hi".into()), times: Some(vec!["25:00".into()]), ..Default::default() }).is_err());
        // Interval: the first run is right away.
        let before = now_ms();
        let a = save(&c, None, Fields { name: Some("Water".into()), prompt: Some("drink".into()), every_min: Some(180), ..Default::default() }).unwrap();
        assert!(a.next_run.unwrap() >= before && a.next_run.unwrap() <= now_ms());
        // Times: sorted, normalised, next one in the future.
        let b = save(&c, None, Fields { name: Some("Daily".into()), prompt: Some("news".into()), times: Some(vec!["18:00".into(), "9:5".into()]), ..Default::default() }).unwrap();
        assert_eq!(b.times, vec!["09:05", "18:00"]);
        assert_eq!(b.every_min, 0);
        assert!(b.next_run.unwrap() >= before);
        // Renaming keeps the planned run.
        let b2 = save(&c, Some(&b.id), Fields { name: Some("Daily news".into()), ..Default::default() }).unwrap();
        assert_eq!(b2.next_run, b.next_run);
        assert!(delete(&c, &a.id).unwrap() && delete(&c, &b.id).unwrap());
    }
}
