//! The one work-day watch loop (`workday/watch.py`): loop records, the
//! measured interval, input fingerprints, one locked pass and turn-start
//! catch-up.
//!
//! The pass lock `day/.loop.lock` is held for the whole pass and is never
//! waited on: a second pass returns `skipped` at once. Every change of
//! `day/loops.json` reads, changes and writes it under `day/.loops.json.lock`.
//! The plan's steps, the live-feed cycle result, the pass clock and the
//! cycle runner are supplied by the caller (`PassInputs`).

use crate::{
    digest16, emit, end, error, field, gate, iso_day, iso_micros, iso_minutes, iso_seconds, now,
    parse_date, parse_iso, read_json, text, today, write_json, Result,
};
use chrono::{DateTime, Duration, FixedOffset, Timelike};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

pub const FACTOR: f64 = 1.2;
pub const WINDOW: usize = 10;
pub const DEFAULT_SECONDS: i64 = 300;
pub const MIN_SECONDS: i64 = 60;
pub const MAX_SECONDS: i64 = 3600;
pub const OWNED: &[&str] = &["watch", "live-feed", "day-watcher"];
pub const KINDS: &[&str] = &[
    "watch",
    "live-feed",
    "day-watcher",
    "cron",
    "monitor",
    "agent",
    "session",
];
pub const SELF_ID: &str = "day-loop";
pub const LIVE_ID: &str = "live-feed";
pub const WATCHER_ID: &str = "day-watcher";
pub const SKIP_ENV: &str = "CAREERBREW_WATCH_PHASE";
const STATE: &str = "day/loops.json";
const INVALID: &str = "saved loop records are invalid; repair the loop store before continuing";

fn watch_error(message: impl Into<String>) -> crate::ProjectError {
    error("WatchError", message)
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join("day/.loop.lock")
}

fn stamp() -> String {
    iso_minutes(&now())
}

fn today_text() -> String {
    iso_day(today())
}

/// Seconds until the next pass: `FACTOR` times the median measured pass.
pub fn interval(seconds: &[f64]) -> i64 {
    if seconds.is_empty() {
        return DEFAULT_SECONDS;
    }
    let window = &seconds[seconds.len().saturating_sub(WINDOW)..];
    let mut sorted = window.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    ((FACTOR * median).ceil() as i64).clamp(MIN_SECONDS, MAX_SECONDS)
}

fn passes_of(state: &Value) -> Vec<f64> {
    state["passes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_f64)
        .collect()
}

fn ensure_day_dir(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("day"))?;
    Ok(())
}

/// The loop store, validated like Python's `_load`.
pub fn load(root: &Path) -> Result<Value> {
    ensure_day_dir(root)?;
    let mut state = read_json(root, STATE, json!({}))?;
    let Some(object) = state.as_object() else {
        return Err(watch_error(INVALID));
    };
    if !object.get("passes").is_none_or(Value::is_array)
        || !object.get("inputs").is_none_or(Value::is_object)
        || !object.get("loops").is_none_or(Value::is_array)
    {
        return Err(watch_error(INVALID));
    }
    let loops = object
        .get("loops")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if loops.iter().any(|lp| {
        !lp.is_object()
            || !field(lp, "id").is_some_and(truthy)
            || !field(lp, "kind")
                .and_then(Value::as_str)
                .is_some_and(|k| KINDS.contains(&k))
    }) {
        return Err(watch_error(INVALID));
    }
    for lp in &loops {
        if ["id", "step", "started", "day", "stopped"]
            .iter()
            .any(|k| !field(lp, k).is_some_and(Value::is_string))
        {
            return Err(watch_error(INVALID));
        }
        let started = parse_iso(&text(field(lp, "started")));
        let day = parse_date(&text(field(lp, "day")));
        let stopped = text(field(lp, "stopped"));
        if started.is_none()
            || day.is_none()
            || (!stopped.is_empty() && parse_iso(&stopped).is_none())
        {
            return Err(watch_error(INVALID));
        }
    }
    let object = state.as_object_mut().expect("object");
    object.entry("passes").or_insert_with(|| json!([]));
    object.entry("inputs").or_insert_with(|| json!({}));
    object.entry("loops").or_insert_with(|| json!([]));
    Ok(state)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
    }
}

/// Read, change and write the loop store under its write lock (Python's `_changing`).
/// The store is saved even when `f` returns early with a value, like the context manager.
pub fn changing<T>(root: &Path, f: impl FnOnce(&mut Value) -> Result<T>) -> Result<T> {
    ensure_day_dir(root)?;
    careerbrew_vault::lock::with_lock(&root.join(STATE), || {
        let mut state = load(root)?;
        let out = f(&mut state)?;
        write_json(root, STATE, &state)?;
        Ok(out)
    })?
}

fn loops_mut(state: &mut Value) -> &mut Vec<Value> {
    state["loops"].as_array_mut().expect("validated loops")
}

fn running(state: &Value) -> Vec<Value> {
    state["loops"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lp| !field(lp, "stopped").is_some_and(truthy))
        .cloned()
        .collect()
}

fn record(state: &mut Value, kind: &str, loop_id: &str, step: &str) {
    let today = today_text();
    for lp in loops_mut(state).iter_mut() {
        let day = field(lp, "day").map_or_else(|| today.clone(), |d| text(Some(d)));
        if text(field(lp, "id")) == loop_id
            && !field(lp, "stopped").is_some_and(truthy)
            && OWNED.contains(&kind)
            && day < today
        {
            lp["stopped"] = json!(stamp());
        }
    }
    if running(state)
        .iter()
        .any(|lp| text(field(lp, "id")) == loop_id)
    {
        return;
    }
    loops_mut(state).push(json!({
        "kind": kind, "id": loop_id, "step": step, "day": today,
        "started": stamp(), "stopped": "",
    }));
}

fn stop_id(state: &mut Value, loop_id: &str) -> i64 {
    let mut n = 0;
    for lp in loops_mut(state).iter_mut() {
        if text(field(lp, "id")) == loop_id && !field(lp, "stopped").is_some_and(truthy) {
            lp["stopped"] = json!(stamp());
            n += 1;
        }
    }
    n
}

fn ensure_session(state: &mut Value, loop_id: &str, session_record: &str) -> Result<()> {
    let same = running(state)
        .into_iter()
        .filter(|lp| text(field(lp, "id")) == loop_id)
        .collect::<Vec<_>>();
    if same.iter().any(|lp| {
        text(field(lp, "kind")) != "session" || text(field(lp, "session_record")) != session_record
    }) {
        return Err(watch_error("session loop id conflicts with another running loop; resolve that loop before continuing"));
    }
    if same.is_empty() {
        loops_mut(state).push(json!({
            "kind": "session", "id": loop_id, "step": "", "day": today_text(),
            "started": stamp(), "stopped": "", "session_record": session_record,
        }));
    }
    Ok(())
}

fn loop_id_ok(value: &str) -> bool {
    Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")
        .expect("loop id")
        .is_match(value)
}

fn py_repr_prefix(value: &str, limit: usize) -> String {
    format!("'{}'", value.chars().take(limit).collect::<String>())
}

/// Record a loop the agent started, so end of day can check it stopped.
pub fn add(
    root: &Path,
    kind: &str,
    loop_id: &str,
    step: &str,
    session_record: &str,
) -> Result<Value> {
    if !KINDS.contains(&kind) {
        return Err(watch_error(format!(
            "loop kind {}: use one of {}",
            py_repr_prefix(kind, 20),
            KINDS.join(", ")
        )));
    }
    if !loop_id_ok(loop_id) {
        return Err(watch_error(format!(
            "loop id {}: use the tool's own id, 1 to 64 letters, digits, . _ : - (no spaces)",
            py_repr_prefix(loop_id, 40)
        )));
    }
    let step = if step.is_empty() {
        String::new()
    } else {
        careerbrew_workday::steps::normalize(step)?
    };
    changing(root, |state| {
        if session_record.is_empty() {
            record(state, kind, loop_id, &step);
            Ok(())
        } else {
            ensure_session(state, loop_id, session_record)
        }
    })?;
    emit(
        root,
        "workday.loop",
        json!({"status": "added", "kind_owned": OWNED.contains(&kind)}),
    )?;
    Ok(json!({"kind": kind, "id": loop_id, "step": step}))
}

/// Mark a loop stopped after the agent stopped it with its own tool.
pub fn stop(root: &Path, loop_id: &str, session_record: &str, missing_ok: bool) -> Result<Value> {
    let n = changing(root, |state| {
        if session_record.is_empty() {
            return Ok(stop_id(state, loop_id));
        }
        let at = stamp();
        let mut n = 0;
        for lp in loops_mut(state).iter_mut() {
            if !field(lp, "stopped").is_some_and(truthy)
                && text(field(lp, "id")) == loop_id
                && text(field(lp, "kind")) == "session"
                && text(field(lp, "session_record")) == session_record
            {
                lp["stopped"] = json!(at);
                n += 1;
            }
        }
        Ok(n)
    })?;
    if n == 0 && !missing_ok {
        return Err(watch_error(format!(
            "no running loop has id {} (careerbrew day loop status lists them)",
            py_repr_prefix(loop_id, 40)
        )));
    }
    Ok(json!({"id": loop_id, "stopped": n}))
}

/// End of day: stop every loop careerbrew owns, and list the ones only the agent can stop.
pub fn end_loops(root: &Path) -> Result<Value> {
    let (owned, left) = careerbrew_sessions::sessions::changing(root, |sessions| {
        changing(root, |state| {
            let owned = running(state)
                .into_iter()
                .filter(|lp| OWNED.contains(&text(field(lp, "kind")).as_str()))
                .collect::<Vec<_>>();
            state["ended"] = json!(stamp());
            for lp in &owned {
                stop_id(state, &text(field(lp, "id")));
            }
            let mut left = running(state);
            for session in sessions["sessions"].as_array().into_iter().flatten() {
                let record = text(field(session, "record_id"));
                let id = text(field(session, "id"));
                let listed = left.iter().any(|lp| {
                    text(field(lp, "kind")) == "session"
                        && text(field(lp, "id")) == id
                        && text(field(lp, "session_record")) == record
                        && !record.is_empty()
                });
                if !field(session, "ended").is_some_and(truthy) && !listed {
                    left.push(json!({
                        "kind": "session", "id": id, "step": "",
                        "started": text(field(session, "started")), "day": text(field(session, "day")),
                    }));
                }
            }
            // The stopped records as Python returns them: the stored records after the stop.
            let owned_after = owned
                .iter()
                .map(|lp| {
                    let mut lp = lp.clone();
                    lp["stopped"] = json!(stamp());
                    lp
                })
                .collect::<Vec<_>>();
            Ok((owned_after, left))
        })
    })?;
    let kinds = owned
        .iter()
        .map(|lp| text(field(lp, "kind")))
        .collect::<Vec<_>>();
    if kinds.iter().any(|k| k == "live-feed" || k == "day-watcher") {
        return Err(error(
            "NotPorted",
            "stopping the live feed or the day watcher is rust-14/rust-17 work",
        ));
    }
    emit(
        root,
        "workday.loop",
        json!({"status": "ended", "stopped": owned.len(), "left": left.len()}),
    )?;
    Ok(json!({"stopped": owned, "running": left}))
}

// ---------- inputs ----------

fn expand_user(target: &str) -> PathBuf {
    if target == "~" || target.starts_with("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(format!("{home}{}", &target[1..]));
        }
    }
    PathBuf::from(target)
}

fn live_state(root: &Path) -> Result<Value> {
    std::fs::create_dir_all(root.join("work/live"))?;
    let mut state = read_json(root, "work/live/state.json", json!({}))?;
    if !state.is_object() {
        state = json!({});
    }
    let object = state.as_object_mut().expect("object");
    object.entry("running").or_insert(json!(false));
    object.entry("cursors").or_insert(json!({}));
    object.entry("seen").or_insert(json!({}));
    Ok(state)
}

fn python_str(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(Value::Null) => "None".into(),
        Some(other) => other.to_string(),
    }
}

fn git_output(repo: &Path, args: &[&str]) -> Option<(bool, String)> {
    use std::process::{Command, Stdio};
    use wait_timeout::ChildExt;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    match child
        .wait_timeout(std::time::Duration::from_secs(30))
        .ok()?
    {
        Some(status) => Some((status.success(), reader.join().ok()?)),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

fn input_digest(root: &Path, item: &str) -> Result<String> {
    let (kind, target) = item.split_once(':').unwrap_or((item, ""));
    if kind == "file" {
        return Ok(match std::fs::metadata(expand_user(target)) {
            Ok(meta) => {
                use std::os::unix::fs::MetadataExt;
                let ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
                format!("{}:{ns}", meta.len())
            }
            Err(_) => "missing".into(),
        });
    }
    if kind == "repo" {
        let repo = expand_user(target);
        let refs = git_output(
            &repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/remotes",
            ],
        );
        let head = git_output(&repo, &["rev-parse", "HEAD"]);
        return Ok(match (refs, head) {
            (Some((true, refs)), Some((_, head))) => digest16(&(refs + &head)),
            (Some(_), Some(_)) => "unreadable".into(),
            _ => "unreadable".into(),
        });
    }
    let live = live_state(root)?;
    Ok(format!(
        "{}|{}",
        python_str(live["cursors"].get(target)),
        python_str(live["seen"].get(target))
    ))
}

/// One digest of a step's watch inputs (ids and sizes only, never content).
pub fn fingerprint(root: &Path, inputs: &[String]) -> Result<String> {
    let mut lines = Vec::new();
    for item in inputs {
        lines.push(format!("{item}={}", input_digest(root, item)?));
    }
    Ok(digest16(&lines.join("\n")))
}

// ---------- the pass lock ----------

/// The pass lock: `Some(handle)` when this pass holds it, `None` at once when another pass runs.
pub fn try_pass_lock(root: &Path) -> Result<Option<File>> {
    ensure_day_dir(root)?;
    let handle = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(lock_path(root))?;
    match handle.try_lock() {
        Ok(()) => Ok(Some(handle)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e.into()),
    }
}

/// True when the background day watcher holds its lock (a shared-lock probe, like Python).
pub fn day_watcher_running(root: &Path) -> Result<bool> {
    ensure_day_dir(root)?;
    let handle = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(root.join("day/watch.lock"))?;
    Ok(match handle.try_lock_shared() {
        Ok(()) => false,
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(e)) => return Err(e.into()),
    })
}

// ---------- one pass ----------

/// The supplied facts of one pass: the plan's steps and the live-feed cycle's result.
pub struct PassInputs<'a> {
    pub steps: &'a [Value],
    pub live: Value,
}

pub type Clock<'a> = &'a mut dyn FnMut() -> f64;
pub type Runner<'a> = &'a mut dyn FnMut(&str, f64) -> (bool, String);

fn rearm_at(seconds: i64) -> String {
    let mut at = now() + Duration::seconds(seconds);
    if at.second() != 0 || at.nanosecond() != 0 {
        at = at
            .with_second(0)
            .and_then(|a| a.with_nanosecond(0))
            .expect("minute")
            + Duration::minutes(1);
    }
    iso_minutes(&at)
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn stale(state: &Value) -> Vec<Value> {
    let today = today_text();
    running(state)
        .into_iter()
        .filter(|lp| field(lp, "day").map_or_else(|| today.clone(), |d| text(Some(d))) < today)
        .collect()
}

/// One pass: lock, the live cycle, each watch-capable step's cycle, then measure and re-arm.
pub fn run_pass(
    root: &Path,
    inputs: &PassInputs,
    clock: Clock,
    runner: Runner,
    restart: bool,
) -> Result<Value> {
    let Some(held) = try_pass_lock(root)? else {
        emit(root, "workday.loop", json!({"status": "skipped"}))?;
        return Ok(json!({"status": "skipped", "reason": "previous cycle running"}));
    };
    let start = clock();
    let today = today_text();
    let began = changing(root, |state| {
        if python_str(state.get("ended")).starts_with(&today) && !restart {
            return Ok(None);
        }
        if let Some(object) = state.as_object_mut() {
            object.remove("ended");
        }
        record(state, "watch", SELF_ID, "");
        if day_watcher_running(root)? {
            record(state, "day-watcher", WATCHER_ID, "");
        }
        Ok(Some(state["inputs"].clone()))
    })?;
    let Some(digests) = began else {
        return Ok(json!({"status": "ended"}));
    };
    let result_live = inputs.live.clone();
    if live_state(root)?["running"].as_bool().unwrap_or(false) {
        changing(root, |state| {
            record(state, "live-feed", LIVE_ID, "");
            Ok(())
        })?;
    }
    let mut results: Vec<Value> = Vec::new();
    let mut fresh = Map::new();
    for s in inputs.steps {
        let cycle = text(field(s, "cycle"));
        let review = text(field(s, "review_ask_cycle"));
        if cycle.is_empty() && review.is_empty() {
            continue;
        }
        let step = text(field(s, "step"));
        let origin = text(field(s, "from"));
        let watched = field(s, "watch")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(|i| text(Some(i))).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut row = json!({"step": step, "result": "", "why": ""});
        let fp = if watched.is_empty() {
            String::new()
        } else {
            fingerprint(root, &watched)?
        };
        if !gate::may_run(root, &origin)? {
            row["result"] = json!("blocked");
            row["why"] = json!(format!("the cycle of {step} comes from {origin}, which may not run commands: careerbrew plugin add <src> --allow-exec"));
        } else {
            let timeout = field(s, "timeout").and_then(Value::as_i64).unwrap_or(0);
            let minutes = if timeout == 0 { 30 } else { timeout };
            if !cycle.is_empty() {
                if !fp.is_empty() && digests.get(&step) == Some(&json!(fp)) {
                    row["result"] = json!("unchanged");
                } else {
                    let (ok, why) = runner(&cycle, (minutes * 60) as f64);
                    row["result"] = json!(if ok { "ok" } else { "failed" });
                    row["why"] = json!(why);
                    if ok && !fp.is_empty() {
                        fresh.insert(step.clone(), json!(fp));
                    }
                }
            }
            if !review.is_empty() {
                return Err(error(
                    "NotPorted",
                    "the review-ask cycle is not part of the rust-16 component",
                ));
            }
        }
        results.push(row);
    }
    let seconds = (clock() - start).max(0.0);
    let (every, next_at, stale_now) = changing(root, |state| {
        if let Some(stored) = state["inputs"].as_object_mut() {
            for (k, v) in &fresh {
                stored.insert(k.clone(), v.clone());
            }
        }
        let mut passes = state["passes"].as_array().cloned().unwrap_or_default();
        passes.push(json!(round3(seconds)));
        let passes = passes[passes.len().saturating_sub(WINDOW)..].to_vec();
        state["passes"] = Value::Array(passes);
        let every = interval(&passes_of(state));
        state["interval"] = json!(every);
        state["next_at"] = json!(rearm_at(every));
        let mut last_results = Map::new();
        for r in &results {
            last_results.insert(text(field(r, "step")), r["result"].clone());
        }
        state["last"] = json!({
            "at": stamp(), "seconds": round3(seconds),
            "live": result_live["status"], "results": last_results,
        });
        Ok((every, text(field(state, "next_at")), stale(state)))
    })?;
    let clock_check = end::tick(root, now(), true)?;
    drop(held);
    let failed = results.iter().filter(|r| r["result"] == "failed").count()
        + usize::from(result_live["status"] == "failed");
    emit(
        root,
        "workday.loop",
        json!({
            "status": if failed > 0 { "failed" } else { "ok" }, "steps": results.len(),
            "failed": failed, "seconds": round1(seconds), "interval": every,
        }),
    )?;
    Ok(json!({
        "status": if failed > 0 { "failed" } else { "ok" }, "seconds": round3(seconds),
        "interval": every, "next_at": next_at, "live": result_live, "steps": results,
        "stale": stale_now, "phase": clock_check["phase"], "wrap_up": clock_check,
    }))
}

/// The loop's status: measured passes, interval, next pass, running and stale loops.
pub fn status(root: &Path) -> Result<Value> {
    let state = load(root)?;
    Ok(json!({
        "passes": state["passes"], "interval": interval(&passes_of(&state)),
        "next_at": state.get("next_at").cloned().unwrap_or(json!("")),
        "last": state.get("last").filter(|v| truthy(v)).cloned().unwrap_or(json!({})),
        "ended": state.get("ended").cloned().unwrap_or(json!("")),
        "running": running(&state), "stale": stale(&state),
    }))
}

// ---------- turn-start catch-up ----------

/// The late due events at `at`: a missed re-arm and a missed wrap-up, each more than 5 minutes late.
pub fn catchup_due(root: &Path, state: &Value, at: DateTime<FixedOffset>) -> Result<String> {
    let mut due = Vec::new();
    let next = text(field(state, "next_at"));
    if let Some(next_at) = parse_iso(&next).and_then(|s| s.with_zone(*at.offset())) {
        if next_at.date_naive() == at.date_naive() && at > next_at + Duration::minutes(5) {
            due.push(format!("loop:{}", crate::iso_auto(&next_at)));
        }
    }
    let (wrap_at, _) = end::times(root, at)?;
    let phase = end::phase(root, at)?;
    let wrapped = field(&crate::day_data(root, at.date_naive())?, "wrapup").is_some_and(truthy);
    if phase != "ended" && phase != "held" && !wrapped && at > wrap_at + Duration::minutes(5) {
        due.push(format!("wrap:{}", crate::iso_auto(&wrap_at)));
    }
    let ended_today = python_str(state.get("ended")).starts_with(&iso_day(at.date_naive()));
    Ok(if ended_today {
        String::new()
    } else {
        due.join("|")
    })
}

fn process_alive(pid: &Value) -> bool {
    let Some(pid) = pid.as_i64().filter(|_| pid.is_i64()) else {
        return false;
    };
    if pid <= 0 || pid > i64::from(i32::MAX) {
        return false;
    }
    // SAFETY: kill with signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// At turn start, claim a more-than-5-minute-late due event once, then run the normal locked pass.
pub fn catch_up(root: &Path, inputs: &PassInputs, clock: Clock, runner: Runner) -> Result<Value> {
    let at = now();
    if catchup_due(root, &load(root)?, at)?.is_empty() {
        return Ok(json!({"status": "current"}));
    }
    let owned = changing(root, |state| {
        let due = catchup_due(root, state, at)?;
        let claim = state
            .get("catchup")
            .filter(|v| truthy(v))
            .cloned()
            .unwrap_or(json!({}));
        let busy =
            claim["status"] == "running" && process_alive(claim.get("pid").unwrap_or(&json!(0)));
        if due.is_empty() || busy || (claim["due"] == json!(due) && claim["status"] == "done") {
            return Ok(None);
        }
        let owned = json!({"due": due, "pid": std::process::id(), "at": iso_micros(&at)});
        let mut claimed = owned.clone();
        claimed["status"] = json!("running");
        state["catchup"] = claimed;
        Ok(Some(owned))
    })?;
    let Some(owned) = owned else {
        return Ok(json!({"status": "current"}));
    };
    let matches = |state: &Value| {
        owned
            .as_object()
            .expect("owned")
            .iter()
            .all(|(k, v)| state.get("catchup").and_then(|c| c.get(k)) == Some(v))
    };
    let result = match run_pass(root, inputs, clock, runner, false) {
        Ok(result) => result,
        Err(failure) => {
            changing(root, |state| {
                if matches(state) {
                    state["catchup"]["status"] = json!("failed");
                }
                Ok(())
            })?;
            return Err(failure);
        }
    };
    changing(root, |state| {
        if matches(state) {
            state["catchup"]["status"] = json!(if result["status"] == "skipped" {
                "retry"
            } else {
                "done"
            });
            state["catchup"]["finished_at"] = json!(iso_seconds(&now()));
        }
        Ok(())
    })?;
    let shown = if result["status"] == "ok" {
        json!("ran")
    } else {
        result["status"].clone()
    };
    Ok(json!({"status": shown, "due": owned["due"], "pass": result}))
}
