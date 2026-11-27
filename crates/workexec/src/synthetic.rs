//! Synthetic rust-16 differential workload over native component functions.
//! It mirrors `tests/oracles/rust16.py` step for step. Every clock, runner,
//! plan and live-feed result is supplied; no real command or watcher runs.

use crate::{
    cron_check, day_data, day_relative, end, error, gate, parse_iso, timing, typed, watch,
    write_json, Result, Stamp,
};
use careerbrew_workday::status::{self, Update};
use chrono::{DateTime, FixedOffset, NaiveDate};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "gate-check",
    "timing-history",
    "loop-records",
    "loop-pass",
    "catch-up",
    "wrap-control",
];
const CANARY: &str = "SYNTHCANARY-rust16-private";

fn plant() -> String {
    std::env::var("CAREERBREW_RUST16_PLANT_WRITE").unwrap_or_default()
}

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("CAREERBREW_RUST13_WRITE_TRACE", base.join("writes.log"));
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_TEST_BUSINESS");
        std::env::remove_var("CAREERBREW_GOAL_ACTIVE");
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profile/examples");
        for relative in [
            "day/calendars.yaml",
            "jobs/criteria.yaml",
            "jobs/sources.yaml",
            "mail.yaml",
            "profile.yaml",
            "sensitive-terms.txt",
            "settings.yaml",
            "work/sources.yaml",
        ] {
            let text = std::fs::read_to_string(examples.join(relative))?;
            careerbrew_workday::write_text(&root, relative, &text)?;
        }
        careerbrew_workday::reset_write_count(&base)?;
        Ok(Self { base, root })
    }
}

fn at(value: &str) {
    std::env::set_var("CAREERBREW_NOW", value);
}

fn moment(value: &str) -> DateTime<FixedOffset> {
    match parse_iso(value) {
        Some(Stamp::Aware(at)) => at,
        _ => panic!("fixture moment"),
    }
}

fn day(value: &str) -> NaiveDate {
    crate::parse_date(value).expect("fixture day")
}

fn class_of<T>(result: Result<T>) -> String {
    match result {
        Ok(_) => "accepted".into(),
        Err(error) => error.class.into(),
    }
}

fn names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn update(
    step: &str,
    plan: &[String],
    state: &str,
    stamp: &str,
    on: NaiveDate,
    extra: Update,
) -> Result<Value> {
    at(stamp);
    let root = careerbrew_workday::root()?;
    status::update(
        &root,
        step,
        plan,
        &Update {
            status: Some(state.into()),
            ..extra
        },
        on,
    )
}

const DAY: &str = "2026-11-02";

// ---------- gate ----------

fn gate_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "gate-check")?;
    let root = &ws.root;
    let today = day(DAY);
    let plan = names(&["/careerbrew", "/standup", "/long", "/fresh"]);
    let planned = vec![
        json!({"step": "/careerbrew", "timeout": 30}),
        json!({"step": "/standup", "timeout": 30}),
        json!({"step": "/long", "timeout": 5, "long_running": true}),
        json!({"step": "/fresh", "timeout": 30}),
    ];
    let nexts = [
        ("blocked", ""),
        ("failed", "env"),
        ("failed", "auth"),
        ("failed", "network"),
        ("failed", "conflict"),
        ("failed", "tool"),
        ("failed", "other"),
        ("done", ""),
        ("waiting", ""),
    ]
    .iter()
    .map(|(state, class)| gate::next_action("/careerbrew", state, class))
    .collect::<Vec<_>>();
    let t = moment("2026-11-02T09:00:00+00:00");
    let probes = [
        json!({"status": "done", "started": "2026-11-02T07:00:00+00:00"}),
        json!({"status": "running"}),
        json!({"status": "running", "started": ""}),
        json!({"status": "running", "started": "not a time"}),
        json!({"status": "running", "started": "2026-11-02T08:40:00+00:00"}),
        json!({"status": "running", "started": "2026-11-02T08:30:00+00:00"}),
        json!({"status": "running", "started": "2026-11-02T08:29:59+00:00"}),
        json!({"status": "running", "started": "2026-11-02T03:29:00-05:00"}),
        json!({"status": "running", "started": "2026-11-02T08:29+00:00"}),
    ];
    let overdue = probes
        .iter()
        .map(|row| gate::overdue(row, 30, t))
        .collect::<Vec<_>>();
    update(
        "/standup",
        &plan,
        "running",
        "2026-11-02T08:00:00+00:00",
        today,
        Update::default(),
    )?;
    update(
        "/long",
        &plan,
        "running",
        "2026-11-02T08:00:00+00:00",
        today,
        Update::default(),
    )?;
    update(
        "/careerbrew",
        &plan,
        "running",
        "2026-11-02T08:40:00+00:00",
        today,
        Update::default(),
    )?;
    update(
        "/fresh",
        &plan,
        "running",
        "2026-11-02T08:50:00+00:00",
        today,
        Update {
            summary: Some(CANARY.into()),
            needs_you: Some(format!("Ask {CANARY}")),
            ..Default::default()
        },
    )?;
    at("2026-11-02T09:00:00+00:00");
    let first = gate::check(root, &planned, today)?;
    let second_run = update(
        "/standup",
        &plan,
        "running",
        "2026-11-02T09:05:00+00:00",
        today,
        Update::default(),
    )?;
    at("2026-11-02T09:40:00+00:00");
    let mut second = gate::check(root, &planned, today)?;
    if plant() == "false-success" {
        second["failed"] = json!([]);
    }
    careerbrew_workday::reset_write_count(&ws.base)?;
    at("2026-11-02T09:41:00+00:00");
    let third = gate::check(root, &planned, today)?;
    let idle_writes = careerbrew_workday::write_count(&ws.base);
    let rows = status::rows(root, &plan, today)?;
    let shown = rows
        .iter()
        .map(|r| {
            let mut out = Map::new();
            for key in [
                "step",
                "status",
                "attempt",
                "retry",
                "timed_out",
                "error_class",
                "next",
                "started",
                "ended",
                "seconds",
            ] {
                out.insert(key.into(), r.get(key).cloned().unwrap_or(Value::Null));
            }
            Value::Object(out)
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "next": nexts,
        "overdue": overdue,
        "first": first,
        "second_attempt": second_run["attempt"],
        "second": second,
        "third": third,
        "idle_writes": idle_writes,
        "rows": shown,
        "may_run": [gate::may_run(root, "careerbrew")?, gate::may_run(root, "settings")?, gate::may_run(root, "plugin:acme")?],
        "counts": status::counts(&rows),
    }))
}

// ---------- timing ----------

fn timing_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "timing-history")?;
    let root = &ws.root;
    let today = day(DAY);
    let plan = names(&["/careerbrew", "/standup", "/new"]);
    let ran = |step: &str, on: &str, start: &str, end_: &str, last: &str| -> Result<()> {
        update(
            step,
            &plan,
            "running",
            &format!("{on}T{start}+00:00"),
            day(on),
            Update::default(),
        )?;
        let extra = match last {
            "blocked" => Update {
                needs_you: Some("Owner picks".into()),
                ..Default::default()
            },
            "failed" => Update {
                error_class: Some("network".into()),
                ..Default::default()
            },
            _ => Update::default(),
        };
        update(
            step,
            &plan,
            last,
            &format!("{on}T{end_}+00:00"),
            day(on),
            extra,
        )?;
        Ok(())
    };
    ran("/careerbrew", "2026-10-26", "09:00:00", "09:01:00", "done")?;
    ran("/careerbrew", "2026-10-27", "09:00:00", "09:02:01", "done")?;
    ran("/careerbrew", "2026-10-28", "09:00:00", "09:03:00", "done")?;
    ran(
        "/careerbrew",
        "2026-10-29",
        "09:00:00",
        "09:10:00",
        "failed",
    )?;
    ran("/careerbrew", "2026-10-30", "09:00:00", "09:04:00", "done")?;
    ran("/standup", "2026-10-29", "10:00:00", "10:00:30", "done")?;
    ran("/standup", "2026-10-30", "10:00:00", "10:20:00", "blocked")?;
    ran("/standup", "2026-10-31", "10:00:00", "10:00:40", "done")?;
    ran("/careerbrew", DAY, "08:00:00", "08:06:40", "done")?;
    ran("/standup", DAY, "08:00:00", "08:00:20", "done")?;
    update(
        "/new",
        &plan,
        "running",
        "2026-11-02T08:30:00+00:00",
        today,
        Update::default(),
    )?;
    at("2026-11-02T09:00:00+00:00");
    let rows = status::rows(root, &plan, today)?;
    let seconds = [
        None,
        Some(0),
        Some(45),
        Some(59),
        Some(60),
        Some(89),
        Some(90),
        Some(150),
        Some(210),
        Some(3599),
        Some(3600),
        Some(3629),
        Some(3631),
        Some(3900),
        Some(7170),
    ];
    let declared = vec![
        json!({"step": "/careerbrew", "from": "careerbrew"}),
        json!({"step": "/a", "from": "plugin:acme"}),
        json!({"step": "/b", "from": "plugin:acme"}),
        json!({"step": "/c", "from": "settings"}),
        json!({"step": "/d", "from": "plugin:zeta"}),
    ];
    let days = timing::history(root);
    let mut history_days = days.keys().cloned().collect::<Vec<_>>();
    history_days.sort();
    Ok(json!({
        "row_seconds": rows.iter().map(|r| r.get("seconds").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "medians": [
            timing::median("/careerbrew", today, &days),
            timing::median("/standup", today, &days),
            timing::median("/careerbrew", day("2026-10-30"), &days),
            timing::median("/new", today, &days),
        ],
        "flags": timing::flags(root, &rows, today)?,
        "week": timing::summary(root, today, 7),
        "three": timing::summary(root, today, 3),
        "shown": seconds.iter().map(|s| timing::shown(*s)).collect::<Vec<_>>(),
        "labels": timing::labels(&declared, &names(&["/careerbrew", "/gone", "/a", "/old", "/d"])),
        "history_days": history_days,
    }))
}

// ---------- loop records ----------

fn loop_records(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "loop-records")?;
    let root = ws.root.clone();
    let root = &root;
    at("2026-11-01T22:00:00+00:00");
    let early = vec![
        watch::add(root, "watch", watch::SELF_ID, "", "")?,
        watch::add(root, "cron", "cron-1", "/careerbrew", "")?,
    ];
    at("2026-11-02T09:00:00+00:00");
    let added = vec![
        watch::add(root, "watch", watch::SELF_ID, "", "")?,
        watch::add(root, "monitor", "mon-1", "careerbrew", "")?,
        watch::add(root, "cron", "cron-1", "", "")?,
        watch::add(root, "agent", "agent.one:2", "/standup", "")?,
        watch::add(root, "session", "sess-1", "", "rec-1")?,
        watch::add(root, "session", "sess-1", "", "rec-1")?,
    ];
    let long_id = "x".repeat(65);
    let errors = vec![
        class_of(watch::add(root, "bogus", "x", "", "")),
        class_of(watch::add(root, "cron", "bad id", "", "")),
        class_of(watch::add(root, "cron", "", "", "")),
        class_of(watch::add(root, "cron", &long_id, "", "")),
        class_of(watch::add(root, "cron", "ok-id", "bad step!", "")),
        class_of(watch::add(root, "session", "cron-1", "", "rec-2")),
        class_of(watch::add(root, "session", "sess-1", "", "rec-2")),
        class_of(watch::stop(root, "nope", "", false)),
    ];
    let stopped = vec![
        watch::stop(root, "mon-1", "", false)?,
        watch::stop(root, "nope", "", true)?,
        watch::stop(root, "sess-1", "other", true)?,
    ];
    let before_end = watch::status(root)?;
    let envs: Vec<BTreeMap<String, String>> =
        [None, Some("1"), Some("TRUE"), Some("0"), Some("maybe")]
            .iter()
            .map(|v| {
                v.map(|v| BTreeMap::from([("CAREERBREW_GOAL_ACTIVE".to_owned(), v.to_owned())]))
                    .unwrap_or_default()
            })
            .collect();
    let mut cron = Vec::new();
    for env in &envs {
        cron.push(cron_check::check(root, env)?.remove(0));
    }
    let intervals = [
        vec![],
        vec![10.0],
        vec![100.0, 200.0, 300.0],
        vec![1000.0, 1000.0, 1000.0],
        vec![5000.0],
        vec![49.9],
        vec![100.0, 200.0],
        vec![
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 400.0,
        ],
    ]
    .iter()
    .map(|x| watch::interval(x))
    .collect::<Vec<_>>();
    let sample = ws.base.join("input.txt");
    let missing = watch::fingerprint(root, &names(&["file:/nonexistent/rust16-missing"]))?;
    let feed = watch::fingerprint(root, &names(&["feed:chat"]))?;
    std::fs::write(&sample, "one")?;
    let item = vec![format!("file:{}", sample.display())];
    let fp1 = watch::fingerprint(root, &item)?;
    let fp2 = watch::fingerprint(root, &item)?;
    std::fs::write(&sample, "two two")?;
    let fp3 = watch::fingerprint(root, &item)?;
    let ended = watch::end_loops(root)?;
    let after = watch::status(root)?;
    let cron_after = cron_check::check(root, &BTreeMap::new())?.remove(0);
    if plant() == "extra" {
        careerbrew_workday::write_text(root, "day/rust16-planted.json", "{}")?;
    }
    let bad = Workspace::new(base, "loop-invalid")?;
    let documents = [
        json!([]),
        json!({"passes": {}}),
        json!({"inputs": []}),
        json!({"loops": [{"id": "x", "kind": "weird"}]}),
        json!({"loops": [{"id": "", "kind": "cron"}]}),
        json!({"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00"}]}),
        json!({"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-13-02", "started": "2026-11-02T09:00+00:00", "stopped": ""}]}),
        json!({"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "later", "stopped": ""}]}),
        json!({"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00", "stopped": "soon"}]}),
        json!({"loops": [{"id": "x", "kind": "cron", "step": "", "day": "2026-11-02", "started": "2026-11-02T09:00+00:00", "stopped": ""}]}),
    ];
    let mut invalid = Vec::new();
    for document in documents {
        write_json(&bad.root, "day/loops.json", &document)?;
        invalid.push(class_of(watch::status(&bad.root)));
    }
    Ok(json!({
        "early": early,
        "added": added,
        "errors": errors,
        "stopped": stopped,
        "before_end": before_end,
        "cron": cron,
        "intervals": intervals,
        "fingerprints": [missing, feed, fp1 == fp2, fp1 != fp3, fp1.len()],
        "ended": ended,
        "after": after,
        "cron_after": cron_after,
        "invalid": invalid,
    }))
}

// ---------- one pass ----------

struct FakeClock(Vec<f64>);

impl FakeClock {
    fn new() -> Self {
        Self(vec![
            100.0, 112.5, 200.0, 230.25, 300.0, 300.0, 400.0, 461.0, 500.0, 512.0, 600.0, 612.0,
            700.0, 712.0,
        ])
    }

    fn next(&mut self) -> f64 {
        self.0.remove(0)
    }
}

#[derive(Default)]
struct FakeRunner {
    calls: Vec<Value>,
}

impl FakeRunner {
    fn run(&mut self, command: &str, seconds: f64) -> (bool, String) {
        self.calls.push(json!([command, seconds]));
        if plant() == "duplicate-dispatch" && self.calls.len() == 1 {
            self.calls.push(json!([command, seconds]));
        }
        if command == "false" {
            (false, "exit 1".into())
        } else {
            (true, String::new())
        }
    }
}

fn pass(
    root: &Path,
    steps: &[Value],
    live: &str,
    clock: &mut FakeClock,
    runner: &mut FakeRunner,
    restart: bool,
) -> Result<Value> {
    let inputs = watch::PassInputs {
        steps,
        live: json!({"status": live}),
    };
    watch::run_pass(
        root,
        &inputs,
        &mut || clock.next(),
        &mut |command, seconds| runner.run(command, seconds),
        restart,
    )
}

fn loop_pass(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "loop-pass")?;
    let root = ws.root.clone();
    let root = &root;
    let sample = ws.base.join("input.txt");
    std::fs::write(&sample, "one")?;
    let steps = vec![
        json!({"step": "/careerbrew", "from": "careerbrew", "cycle": "careerbrew work feed cycle", "watch": [], "timeout": 30}),
        json!({"step": "/repo", "from": "settings", "cycle": "sync-repo --quick", "watch": [format!("file:{}", sample.display())], "timeout": 10}),
        json!({"step": "/plugin", "from": "plugin:acme", "cycle": "acme run", "watch": [], "timeout": 30}),
        json!({"step": "/fail", "from": "settings", "cycle": "false", "watch": [], "timeout": 0}),
        json!({"step": "/quiet", "from": "settings", "cycle": "", "watch": [], "timeout": 30}),
    ];
    let mut clock = FakeClock::new();
    let mut runner = FakeRunner::default();
    let mut out = Map::new();
    out.insert(
        "first".into(),
        pass(root, &steps, "ok", &mut clock, &mut runner, true)?,
    );
    at("2026-11-02T09:20:00+00:00");
    out.insert(
        "second".into(),
        pass(root, &steps, "ok", &mut clock, &mut runner, true)?,
    );
    std::fs::write(&sample, "two two")?;
    at("2026-11-02T09:40:30+00:00");
    out.insert(
        "third".into(),
        pass(root, &steps, "failed", &mut clock, &mut runner, true)?,
    );
    {
        let held = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(watch::lock_path(root))?;
        held.try_lock()
            .map_err(|_| error("ValueError", "fixture lock"))?;
        out.insert(
            "skipped".into(),
            pass(root, &steps, "ok", &mut clock, &mut runner, true)?,
        );
        held.unlock()?;
    }
    at("2026-11-02T10:00:00+00:00");
    out.insert("end".into(), watch::end_loops(root)?);
    careerbrew_workday::reset_write_count(&ws.base)?;
    out.insert(
        "ended".into(),
        pass(root, &steps, "ok", &mut clock, &mut runner, false)?,
    );
    out.insert(
        "ended_writes".into(),
        json!(careerbrew_workday::write_count(&ws.base)),
    );
    at("2026-11-02T10:05:00+00:00");
    out.insert(
        "restarted".into(),
        pass(root, &steps, "ok", &mut clock, &mut runner, true)?,
    );
    out.insert("calls".into(), json!(runner.calls));
    out.insert("status".into(), watch::status(root)?);
    Ok(Value::Object(out))
}

// ---------- catch-up ----------

fn catch_up(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "catch-up")?;
    let root = ws.root.clone();
    let root = &root;
    let steps = vec![
        json!({"step": "/careerbrew", "from": "careerbrew", "cycle": "careerbrew work feed cycle", "watch": [], "timeout": 30}),
    ];
    let mut clock = FakeClock::new();
    let mut runner = FakeRunner::default();
    let inputs = watch::PassInputs {
        steps: &steps,
        live: json!({"status": "ok"}),
    };
    let catch = |clock: &mut FakeClock, runner: &mut FakeRunner| {
        watch::catch_up(
            root,
            &inputs,
            &mut || clock.next(),
            &mut |command, seconds| runner.run(command, seconds),
        )
    };
    let mut out = Map::new();
    out.insert("none".into(), catch(&mut clock, &mut runner)?);
    out.insert(
        "first_pass".into(),
        pass(root, &steps, "ok", &mut clock, &mut runner, true)?,
    );
    at("2026-11-02T09:03:00+00:00");
    out.insert("early".into(), catch(&mut clock, &mut runner)?);
    at("2026-11-02T09:10:00+00:00");
    out.insert("late".into(), catch(&mut clock, &mut runner)?);
    out.insert("again".into(), catch(&mut clock, &mut runner)?);
    at("2026-11-02T09:30:00+00:00");
    let due = watch::catchup_due(
        root,
        &watch::load(root)?,
        moment("2026-11-02T09:30:00+00:00"),
    )?;
    watch::changing(root, |state| {
        state["catchup"] = json!({"due": due, "pid": std::process::id(), "at": "2026-11-02T09:29:00.000000+00:00", "status": "running"});
        Ok(())
    })?;
    out.insert("busy".into(), catch(&mut clock, &mut runner)?);
    watch::changing(root, |state| {
        state["catchup"] = json!({"due": due, "pid": 0, "at": "2026-11-02T09:29:00.000000+00:00", "status": "running"});
        Ok(())
    })?;
    out.insert("crashed".into(), catch(&mut clock, &mut runner)?);
    watch::changing(root, |state| {
        state["catchup"] =
            json!({"due": "loop:2026-11-02T09:31:00+00:00", "pid": 0, "at": "x", "status": "done"});
        state["next_at"] = json!("2026-11-02T09:31");
        Ok(())
    })?;
    at("2026-11-02T09:40:00+00:00");
    out.insert("done_claim".into(), catch(&mut clock, &mut runner)?);
    let late = moment("2026-11-02T17:10:00+00:00");
    let st = watch::load(root)?;
    let with = |extra: Value| {
        let mut state = st.clone();
        for (k, v) in extra.as_object().expect("extra") {
            state[k] = v.clone();
        }
        state
    };
    let mut probes = Vec::new();
    for next_at in [
        "2026-11-02T17:00+00:00",
        "2026-11-01T17:00+00:00",
        "2026-11-02T17:05+00:00",
        "garbage",
        "",
        "2026-11-02T12:04:00-05:00",
    ] {
        probes.push(watch::catchup_due(
            root,
            &with(json!({"next_at": next_at})),
            late,
        )?);
    }
    probes.push(watch::catchup_due(
        root,
        &with(json!({"next_at": "", "ended": "2026-11-02T16:00+00:00"})),
        late,
    )?);
    probes.push(watch::catchup_due(
        root,
        &with(json!({"next_at": "", "ended": "2026-11-01T16:00+00:00"})),
        late,
    )?);
    probes.push(watch::catchup_due(
        root,
        &with(json!({"next_at": ""})),
        moment("2026-11-02T17:05:00+00:00"),
    )?);
    at("2026-11-02T17:10:00+00:00");
    end::control(root, "16:00", false)?;
    probes.push(watch::catchup_due(
        root,
        &with(json!({"next_at": ""})),
        late,
    )?);
    end::control(root, "", true)?;
    probes.push(watch::catchup_due(
        root,
        &with(json!({"next_at": ""})),
        late,
    )?);
    out.insert("probes".into(), json!(probes));
    let mut shown = watch::status(root)?;
    shown.as_object_mut().expect("status").remove("running");
    out.insert("status".into(), shown);
    Ok(Value::Object(out))
}

// ---------- wrap-up control ----------

fn wrap_control(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "wrap-control")?;
    let root = &ws.root;
    let today = day(DAY);
    let now = crate::now;
    let mut out = Map::new();
    out.insert("default".into(), json!(end::times_text(root, now())?));
    out.insert("view".into(), end::view(root, now())?);
    out.insert(
        "errors".into(),
        json!(["25:00", "9:00", "23:31", "12:60", "noon", "16:00:00"]
            .iter()
            .map(|a| class_of(end::control(root, a, false)))
            .collect::<Vec<_>>()),
    );
    out.insert("noop".into(), end::control(root, "", false)?);
    out.insert("moved".into(), end::control(root, "16:00", false)?);
    out.insert("moved_times".into(), json!(end::times_text(root, now())?));
    let mut phases = Vec::new();
    for s in [
        "2026-11-02T15:59:00+00:00",
        "2026-11-02T16:00:00+00:00",
        "2026-11-02T16:10:00+00:00",
    ] {
        phases.push(end::phase(root, moment(s))?);
    }
    out.insert("phases".into(), json!(phases));
    out.insert(
        "tick_before".into(),
        end::tick(root, moment("2026-11-02T15:59:00+00:00"), true)?,
    );
    out.insert("late_move".into(), end::control(root, "23:29", false)?);
    if plant() == "lost-wrap" {
        out.insert("held".into(), end::view(root, now())?);
    } else {
        out.insert("held".into(), end::control(root, "", true)?);
    }
    out.insert(
        "held_tick".into(),
        end::tick(root, moment("2026-11-02T23:50:00+00:00"), true)?,
    );
    out.insert(
        "held_phase".into(),
        json!(end::phase(root, moment("2026-11-02T23:50:00+00:00"))?),
    );
    out.insert("unheld".into(), end::control(root, "10:00", false)?);
    let data = day_data(root, today)?;
    let mut keys = data
        .as_object()
        .map(|o| o.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    out.insert("day_file_keys".into(), json!(keys));
    out.insert("wrap_control".into(), data["wrap_control"].clone());
    let mut data = data;
    data["end"] = json!({"at": "2026-11-02T12:00+00:00", "by": "owner"});
    write_json(root, &day_relative(today), &data)?;
    out.insert(
        "after_end".into(),
        json!(class_of(end::control(root, "11:00", false))),
    );
    out.insert("ended_phase".into(), json!(end::phase(root, now())?));
    out.insert("ended_tick".into(), end::tick(root, now(), true)?);
    out.insert(
        "outcomes".into(),
        end::step_outcomes(&[
            json!({"status": "done"}),
            json!({"status": "blocked", "needs_you": "x"}),
            json!({"status": "failed"}),
            json!({"status": "waiting"}),
            json!({"status": "odd"}),
        ]),
    );
    Ok(Value::Object(out))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "gate-check" => gate_case(base),
        "timing-history" => timing_case(base),
        "loop-records" => loop_records(base),
        "loop-pass" => loop_pass(base),
        "catch-up" => catch_up(base),
        "wrap-control" => wrap_control(base),
        _ => Err(error("ValueError", "unknown rust-16 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = careerbrew_workday::root()?.join("rust16-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            let result = match run(case, &base) {
                Ok(result) => result,
                Err(error) => json!({"error": error.class, "message": error.message}),
            };
            cases.push(json!({"id": case, "result": result}));
        }
    }
    Ok(careerbrew_workday::compact(&typed(&json!({"cases": cases})))? + "\n")
}
