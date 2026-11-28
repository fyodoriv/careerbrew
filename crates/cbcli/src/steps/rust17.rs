//! Native CLI handlers owned by `rust-17` (see `steps/mod.rs`): the
//! results-of-the-day count (`day results`) and the promise list (`work
//! promise list`).
//!
//! Wave 8 adds the paths that read no sent-day text: `day pm-sent` with no
//! draft shown, `work promise outcomes` and `stats` when no record holds a
//! sent-day observation or decision to check, and `work promise accept`
//! without `--yes`.
//!
//! All only read. They refuse (exit 64, Python runs it) when Python could
//! show business text or redact (see `rust16::business_hidden`), when the
//! clock is a form the two sides read differently (`rust16::pin_clock`),
//! and when a stored value has a type or shape Python would print, sort or
//! fail on differently. The promise writers (`add`, `done`, `dropped`,
//! `step`) stay in Python: their YAML rewrite is not byte-identical yet.
use super::rust16::{business_hidden, day_form, logged, out, pin_clock, Canon};
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust17.rs`.
pub const COMMANDS: &[&str] = &[
    "day.results",
    "work.promise.list",
    "day.pm-sent",
    "work.promise.outcomes",
    "work.promise.stats",
    "work.promise.accept",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = std::time::Instant::now();
    match name {
        "day.results" => results(args, ctx),
        "work.promise.list" => logged("work.promise", promise_list(args, ctx)?, ctx, started),
        "day.pm-sent" => pm_sent(args, ctx),
        "work.promise.outcomes" => logged("work.promise", outcomes_cmd(args, ctx)?, ctx, started),
        "work.promise.stats" => logged("work.promise", stats_cmd(args, ctx)?, ctx, started),
        "work.promise.accept" => logged("work.promise", accept_cmd(args, ctx)?, ctx, started),
        _ => None,
    }
}

fn store_of() -> Option<careerbrew_vault::transfer::Context> {
    careerbrew_vault::transfer::Context::from_env().ok()
}

/// A map's value for `key`, or Err when the map holds that key twice or a
/// key that is not a string (PyYAML and `json.loads` keep the last one, or
/// a non-string key Python compares differently).
fn lookup<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Result<Option<&'a Canon>, ()> {
    let mut found = None;
    for (k, v) in pairs {
        let Canon::Str(name) = k else {
            return Err(());
        };
        if name == key {
            if found.is_some() {
                return Err(());
            }
            found = Some(v);
        }
    }
    Ok(found)
}

/// Unique string keys only.
fn plain_keys(pairs: &[(Canon, Canon)]) -> Option<()> {
    let mut seen: Vec<&str> = Vec::new();
    for (k, _) in pairs {
        let Canon::Str(name) = k else {
            return None;
        };
        if seen.contains(&name.as_str()) {
            return None;
        }
        seen.push(name);
    }
    Some(())
}

/// Python truthiness of a stored value; None for a float (refused).
fn truthy(value: Option<&Canon>) -> Option<bool> {
    Some(match value {
        None | Some(Canon::Null) => false,
        Some(Canon::Bool(b)) => *b,
        Some(Canon::Int(n)) => n.to_string() != "0",
        Some(Canon::Float(_)) => return None,
        Some(Canon::Str(s) | Canon::Date(s) | Canon::DateTime(s)) => !s.is_empty(),
        Some(Canon::List(items)) => !items.is_empty(),
        Some(Canon::Map(pairs)) => !pairs.is_empty(),
    })
}

// ---------- day results ----------

/// Line breaks `str.splitlines` splits on and `str::lines` does not.
fn odd_breaks(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c,
            '\r' | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    })
}

fn results(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !business_hidden(ctx) {
        return None;
    }
    let root = ctx.root.as_path();
    pin_clock(root)?;
    let day = match args.text("date") {
        // `date.fromisoformat` takes other forms too (`20261120`, a week
        // date); only the plain form is served. Year 0 too: chrono reads
        // it, `date.fromisoformat` raises.
        Some(text) => {
            if !day_form(text) || text.starts_with("0000") {
                return None;
            }
            careerbrew_projects::date(text).ok()?.to_string()
        }
        None => careerbrew_workday::today().to_string(),
    };
    let context = store_of()?;
    let store = context.store();
    let path = root.join(format!("day/{day}.jsonl"));
    let text = store
        .read_text(&path, Some(""), true)
        .ok()?
        .unwrap_or_default();
    if odd_breaks(&text) {
        return None;
    }
    let rows = store.read_jsonl(&path, true).ok()?;
    let mut kinds: Vec<(String, usize)> = Vec::new();
    for row in &rows {
        let Canon::Map(pairs) = row else {
            return None;
        };
        let kind = match lookup(pairs, "kind").ok()? {
            None => "other".to_owned(),
            Some(Canon::Str(kind)) => kind.clone(),
            Some(_) => return None,
        };
        match kinds.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => kinds.push((kind, 1)),
        }
    }
    let summary = kinds
        .iter()
        .map(|(k, n)| format!("{k}={n}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stdout = format!(
        "results of {day}: {} item(s): {}\n",
        rows.len(),
        if summary.is_empty() { "none" } else { &summary }
    );
    if args.flag("show") {
        stdout.push_str("titles hidden: stdout is not a terminal and no plugin policy allows agents to read business data\n");
    }
    Some(out(stdout, 0))
}

// ---------- work promise list ----------

const STEPS: [&str; 3] = ["research", "draft", "status"];

const STEP_TEXT: &str = "{\"research\": \"research what the promise needs (the thread, the links, the code)\", \"draft\": \"draft the follow-up message (a draft only; the owner sends it)\", \"status\": \"report status to the owner\"}";

/// One row of `promises.view(..., business=False)`.
struct Row {
    name: String,
    due: String,
    overdue: bool,
    made_at: String,
    event: String,
    url: String,
    parent: String,
    /// `steps` already as Python's `json.dumps` prints it.
    steps: String,
    next: &'static str,
    closed: String,
}

/// A stored string field: absent gives `default`; anything but a string is
/// refused.
fn string_of(value: Option<&Canon>) -> Option<String> {
    match value {
        None => Some(String::new()),
        Some(Canon::Str(text)) => Some(text.clone()),
        Some(_) => None,
    }
}

/// `json.dumps` of a stored value; None for a form whose spelling could
/// differ (a float, a date, a non-string or repeated key).
fn canon_json(value: &Canon, out: &mut String) -> Option<()> {
    match value {
        Canon::Null => out.push_str("null"),
        Canon::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Canon::Int(n) => out.push_str(&n.to_string()),
        Canon::Str(text) => crate::render::py_string(text, out),
        Canon::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                canon_json(item, out)?;
            }
            out.push(']');
        }
        Canon::Map(pairs) => {
            plain_keys(pairs)?;
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                canon_json(key, out)?;
                out.push_str(": ");
                canon_json(item, out)?;
            }
            out.push('}');
        }
        Canon::Float(_) | Canon::Date(_) | Canon::DateTime(_) => return None,
    }
    Some(())
}

fn row_of(name: &str, project: &Canon, today: &str) -> Option<Option<Row>> {
    let Canon::Map(p) = project else {
        return None;
    };
    plain_keys(p)?;
    let get = |key: &str| lookup(p, key).ok().flatten();
    if !matches!(get("kind"), Some(Canon::Str(kind)) if kind == "promise") {
        // Python skips it without reading anything else.
        return Some(None);
    }
    let archived = truthy(get("archived_at"))?;
    let promise: &[(Canon, Canon)] = match get("promise") {
        Some(Canon::Map(pairs)) => pairs,
        None => &[],
        Some(v) if !truthy(Some(v))? => &[],
        Some(_) => return None,
    };
    plain_keys(promise)?;
    let pget = |key: &str| lookup(promise, key).ok().flatten();
    let due = match pget("due") {
        Some(Canon::Str(text)) => text.clone(),
        None => String::new(),
        Some(v) if !truthy(Some(v))? => String::new(),
        Some(_) => return None,
    };
    let mut url = String::new();
    match get("sources") {
        Some(Canon::List(sources)) => {
            for source in sources {
                let Canon::Map(s) = source else {
                    return None;
                };
                plain_keys(s)?;
                let kind = lookup(s, "kind").ok()?;
                if matches!(kind, Some(Canon::Str(k)) if k == "chat" || k == "url") {
                    url = string_of(Some(lookup(s, "ref").ok()??))?;
                    break;
                }
                if !matches!(kind, None | Some(Canon::Str(_) | Canon::Null)) {
                    return None;
                }
            }
        }
        None => {}
        Some(v) if !truthy(Some(v))? => {}
        Some(_) => return None,
    }
    let (steps, next) = match pget("steps") {
        Some(Canon::Map(pairs)) => {
            let mut text = String::new();
            canon_json(&Canon::Map(pairs.clone()), &mut text)?;
            let done = |step: &str| {
                pairs
                    .iter()
                    .any(|(k, _)| matches!(k, Canon::Str(s) if s == step))
            };
            (
                text,
                STEPS.into_iter().find(|s| !done(s)).unwrap_or("status"),
            )
        }
        None => ("{}".to_owned(), "research"),
        Some(v) if !truthy(Some(v))? => ("{}".to_owned(), "research"),
        Some(_) => return None,
    };
    let closed = if archived {
        string_of(pget("closed"))?
    } else {
        String::new()
    };
    Some(Some(Row {
        name: name.to_owned(),
        overdue: !due.is_empty() && due.as_str() < today,
        due,
        made_at: string_of(pget("made_at"))?,
        event: string_of(pget("event"))?,
        url,
        parent: string_of(get("parent"))?,
        steps,
        next,
        closed,
    }))
}

fn promise_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !business_hidden(ctx) {
        return None;
    }
    let root = ctx.root.as_path();
    pin_clock(root)?;
    let today = careerbrew_workday::today().to_string();
    let include_closed = args.flag("all");
    let context = store_of()?;
    let data = context
        .store()
        .read_yaml(&root.join("work/projects.yaml"), Canon::Null, true)
        .ok()?;
    let projects: &[(Canon, Canon)] = match &data {
        Canon::Map(top) => {
            plain_keys(top)?;
            match lookup(top, "projects").ok()? {
                Some(Canon::Map(pairs)) => pairs,
                None => &[],
                Some(v) if !truthy(Some(v))? => &[],
                Some(_) => return None,
            }
        }
        other if !truthy(Some(other))? => &[],
        _ => return None,
    };
    plain_keys(projects)?;
    let mut rows = Vec::new();
    for (name, project) in projects {
        let Canon::Str(name) = name else {
            return None;
        };
        if let Canon::Map(p) = project {
            if !include_closed {
                let archived = truthy(lookup(p, "archived_at").ok()?)?;
                let promise =
                    matches!(lookup(p, "kind").ok()?, Some(Canon::Str(k)) if k == "promise");
                if promise && archived {
                    continue;
                }
            }
        }
        if let Some(row) = row_of(name, project, &today)? {
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| {
        let key = |r: &Row| {
            (
                !r.closed.is_empty(),
                r.due.is_empty(),
                r.due.clone(),
                r.made_at.clone(),
                r.name.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
    let mut stdout = String::new();
    if args.flag("json") {
        stdout.push_str("{\"promises\": [");
        for (i, r) in rows.iter().enumerate() {
            if i > 0 {
                stdout.push_str(", ");
            }
            let s = &mut stdout;
            s.push_str("{\"name\": ");
            crate::render::py_string(&r.name, s);
            s.push_str(", \"due\": ");
            crate::render::py_string(&r.due, s);
            s.push_str(if r.overdue {
                ", \"overdue\": true, \"made_at\": "
            } else {
                ", \"overdue\": false, \"made_at\": "
            });
            crate::render::py_string(&r.made_at, s);
            s.push_str(", \"event\": ");
            crate::render::py_string(&r.event, s);
            s.push_str(", \"url\": ");
            crate::render::py_string(&r.url, s);
            s.push_str(", \"parent\": ");
            crate::render::py_string(&r.parent, s);
            s.push_str(", \"steps\": ");
            s.push_str(&r.steps);
            s.push_str(", \"next\": ");
            crate::render::py_string(r.next, s);
            s.push_str(", \"closed\": ");
            crate::render::py_string(&r.closed, s);
            s.push('}');
        }
        stdout.push_str("], \"steps\": ");
        stdout.push_str(STEP_TEXT);
        stdout.push_str("}\n");
        return Some(out(stdout, 0));
    }
    if rows.is_empty() {
        return Some(out("promises: none open\n".to_owned(), 0));
    }
    let open = rows.iter().filter(|r| r.closed.is_empty()).count();
    let overdue = rows.iter().filter(|r| r.overdue).count();
    stdout.push_str(&format!("promises: {open} open"));
    if overdue > 0 {
        stdout.push_str(&format!(", {overdue} overdue"));
    }
    stdout.push('\n');
    for r in &rows {
        let when = if r.due.is_empty() {
            String::new()
        } else {
            format!(" due {}{}", r.due, if r.overdue { " OVERDUE" } else { "" })
        };
        let state = if r.closed.is_empty() {
            format!(" next: {}", r.next)
        } else {
            format!(" [{}]", r.closed)
        };
        let url = if r.url.is_empty() {
            String::new()
        } else {
            format!(" {}", r.url)
        };
        stdout.push_str(&format!("- {}{when}: (text hidden){state}{url}\n", r.name));
    }
    Some(out(stdout, 0))
}

// ---------- day pm-sent (no draft shown) ----------

/// `cmd_day_pm_sent` when no draft was shown for the day: it reads the
/// file, finds no shown draft, prints one line and writes nothing. A shown
/// draft (the learning path writes history, promises and receipts)
/// refuses before any write.
fn pm_sent(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let root = ctx.root.as_path();
    pin_clock(root)?;
    let day = match args.text("day") {
        // Python takes any text; only the plain form is served (it is
        // part of a file name).
        Some(text) if day_form(text) => text.to_owned(),
        Some(_) => return None,
        None => careerbrew_workday::today().to_string(),
    };
    let file = args.text("file")?;
    if file.is_empty() || file.starts_with('~') {
        return None;
    }
    // `Path.read_text(encoding="utf-8")`: a read or decode error is
    // Python's to report.
    let bytes = std::fs::read(file).ok()?;
    String::from_utf8(bytes).ok()?;
    let context = store_of()?;
    let shown = context
        .store()
        .read_json(
            &root.join(format!("day/pm-shown-{day}.json")),
            Canon::Map(Vec::new()),
            true,
        )
        .ok()?;
    if truthy(Some(&shown))? {
        return None;
    }
    // `paths.sub` makes the day folder (no file) before the read.
    std::fs::create_dir_all(root.join("day")).ok()?;
    Some(out(
        format!("no draft was shown for {day}: show the PM update first (Day log in the web app, or `careerbrew day send` without --yes)\n"),
        0,
    ))
}

// ---------- work promise outcomes / stats / accept ----------

/// `projects.records()` as (name, record) pairs, or None for a shape
/// Python would read differently or fail on.
fn records(root: &std::path::Path) -> Option<Vec<(String, Canon)>> {
    let context = store_of()?;
    let data = context
        .store()
        .read_yaml(&root.join("work/projects.yaml"), Canon::Null, true)
        .ok()?;
    let projects: Vec<(Canon, Canon)> = match &data {
        Canon::Map(top) => {
            plain_keys(top)?;
            match lookup(top, "projects").ok()? {
                Some(Canon::Map(pairs)) => pairs.clone(),
                None => Vec::new(),
                Some(v) if !truthy(Some(v))? => Vec::new(),
                Some(_) => return None,
            }
        }
        other if !truthy(Some(other))? => Vec::new(),
        _ => return None,
    };
    plain_keys(&projects)?;
    projects
        .into_iter()
        .map(|(k, v)| match k {
            Canon::Str(name) => Some((name, v)),
            _ => None,
        })
        .collect()
}

/// `value or {}` that Python then calls `.get` on: a map, or empty for a
/// falsy value; None (refuse) for a truthy non-map or repeated keys.
fn map_or_empty(value: Option<&Canon>) -> Option<&[(Canon, Canon)]> {
    match value {
        Some(Canon::Map(pairs)) => {
            plain_keys(pairs)?;
            Some(pairs)
        }
        v if !truthy(v)? => Some(&[]),
        _ => None,
    }
}

fn is_str(value: Option<&Canon>, text: &str) -> bool {
    matches!(value, Some(Canon::Str(s)) if s == text)
}

/// `_safe_day(value)`: the value when it is a string that is exactly a
/// valid ISO day, else "". None refuses (year 0: chrono reads it, Python
/// does not).
fn safe_day(value: Option<&Canon>) -> Option<String> {
    let Some(Canon::Str(text)) = value else {
        return Some(String::new());
    };
    if !day_form(text) {
        return Some(String::new());
    }
    if text.starts_with("0000") {
        return None;
    }
    Some(match careerbrew_projects::date(text) {
        Ok(day) if day.to_string() == *text => text.clone(),
        _ => String::new(),
    })
}

/// One `decision-needs-review` row whose decision day is not a safe day,
/// so Python reads no sent text and offers no resolutions.
struct Review {
    promise: String,
    prior_state: &'static str,
    current_state: String,
    prior_due: String,
    current_due: String,
}

/// `pm_promises.outcomes()` when it reads no sent-day text: every truthy
/// decision has no safe day, and every observation either has no safe day
/// or sits on an archived record. Anything else refuses: Python would take
/// the sent-day and feed locks (creating lock files) and compare receipt
/// revisions.
fn outcomes_of(every: &[(String, Canon)]) -> Option<Vec<Review>> {
    let mut rows = Vec::new();
    for (name, row) in every {
        let Canon::Map(row) = row else {
            return None;
        };
        plain_keys(row)?;
        let get = |key: &str| lookup(row, key).ok().flatten();
        let promise = map_or_empty(get("promise"))?;
        let pget = |key: &str| lookup(promise, key).ok().flatten();
        let observed = map_or_empty(pget("pm_observation"))?;
        let decision = map_or_empty(pget("pm_decision"))?;
        let dget = |key: &str| lookup(decision, key).ok().flatten();
        if !decision.is_empty() {
            if !safe_day(dget("day"))?.is_empty() {
                return None;
            }
            let state = dget("state");
            let prior_state = if is_str(state, "done") {
                "done"
            } else if is_str(state, "moved") {
                "moved"
            } else {
                "unknown"
            };
            let closed = pget("closed");
            let current_state = if is_str(closed, "done") || is_str(closed, "dropped") {
                string_of(closed)?
            } else if truthy(get("archived_at"))? {
                "archived".to_owned()
            } else {
                "open".to_owned()
            };
            rows.push(Review {
                promise: name.clone(),
                prior_state,
                current_state,
                prior_due: if prior_state == "moved" {
                    safe_day(dget("due"))?
                } else {
                    String::new()
                },
                current_due: safe_day(pget("due"))?,
            });
            continue;
        }
        let oget = |key: &str| lookup(observed, key).ok().flatten();
        if safe_day(oget("day"))?.is_empty() || truthy(get("archived_at"))? {
            continue;
        }
        return None;
    }
    rows.sort_by(|a, b| a.promise.cmp(&b.promise));
    Some(rows)
}

fn outcomes_cmd(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let every = records(ctx.root.as_path())?;
    let rows = outcomes_of(&every)?;
    let mut s = String::new();
    if args.flag("json") {
        s.push_str("{\"outcomes\": [");
        for (i, r) in rows.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str("{\"id\": \"\", \"promise\": ");
            crate::render::py_string(&r.promise, &mut s);
            s.push_str(", \"state\": \"decision-needs-review\", \"due\": \"\", \"prior_state\": ");
            crate::render::py_string(r.prior_state, &mut s);
            s.push_str(", \"current_state\": ");
            crate::render::py_string(&r.current_state, &mut s);
            s.push_str(", \"prior_due\": ");
            crate::render::py_string(&r.prior_due, &mut s);
            s.push_str(", \"current_due\": ");
            crate::render::py_string(&r.current_due, &mut s);
            s.push_str(", \"resolutions\": []}");
        }
        s.push_str("]}\n");
        return Some(out(s, 0));
    }
    s.push_str(&format!("PM promise outcomes: {}\n", rows.len()));
    for r in &rows {
        s.push_str(&format!("  {} decision-needs-review\n", r.promise));
        let due = |d: &str| {
            if d.is_empty() {
                String::new()
            } else {
                format!(" due {d}")
            }
        };
        s.push_str(&format!(
            "    prior: {}{}; current: {}{}\n",
            r.prior_state,
            due(&r.prior_due),
            r.current_state,
            due(&r.current_due)
        ));
    }
    Some(out(s, 0))
}

fn two(b: &[u8]) -> Option<u32> {
    if b.len() == 2 && b.iter().all(u8::is_ascii_digit) {
        Some(u32::from(b[0] - b'0') * 10 + u32::from(b[1] - b'0'))
    } else {
        None
    }
}

/// A day as its number of days since 1970-01-01.
fn ordinal_of(text: &str) -> Option<i64> {
    let epoch = careerbrew_projects::date("1970-01-01").ok()?;
    let day = careerbrew_projects::date(text).ok()?;
    Some((day - epoch).num_days())
}

/// `date.fromisoformat(text)` as a day number. Some(Err) is Python's
/// ValueError (the plain form but no real day, or ""); None refuses (a
/// form Python may read another way).
fn plain_day(text: &str) -> Option<Result<i64, ()>> {
    if text.is_empty() {
        return Some(Err(()));
    }
    if !day_form(text) || text.starts_with("0000") {
        return None;
    }
    Some(ordinal_of(text).ok_or(()))
}

/// `datetime.fromisoformat(text)` turned into a local day number (an aware
/// stamp through `astimezone(local_tz())`, a naive one as written).
/// Served forms: "", `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM:SS`, and that with
/// `Z` or `+HH:MM`. Everything else refuses.
fn stamp_day(text: &str, root: &std::path::Path) -> Option<Result<i64, ()>> {
    if text.len() <= 10 {
        return plain_day(text);
    }
    let b = text.as_bytes();
    if b.len() < 19 || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, m, sec) = (two(&b[11..13])?, two(&b[14..16])?, two(&b[17..19])?);
    if h > 23 || m > 59 || sec > 59 {
        return None;
    }
    let day = plain_day(&text[..10])?.ok()?;
    let rest = &b[19..];
    if rest.is_empty() {
        return Some(Ok(day));
    }
    let offset_ok = match rest {
        b"Z" => true,
        [sign, h1, h2, b':', m1, m2] if *sign == b'+' || *sign == b'-' => matches!(
            (two(&[*h1, *h2]), two(&[*m1, *m2])),
            (Some(oh), Some(om)) if oh < 24 && om < 60
        ),
        _ => false,
    };
    if !offset_ok {
        return None;
    }
    let at = careerbrew_tutor::parse_at(text).ok()?;
    let zone = careerbrew_tutor::local_zone(root);
    let epoch = careerbrew_projects::date("1970-01-01").ok()?;
    Some(Ok((at.with_timezone(&zone).date_naive() - epoch).num_days()))
}

/// `str(value)` fed to a date parser: absent or null spells "" or "None"
/// (a ValueError either way, so ""); only strings are read further.
fn stored_text(value: Option<&Canon>) -> Option<&str> {
    match value {
        None | Some(Canon::Null) => Some(""),
        Some(Canon::Str(text)) => Some(text),
        Some(_) => None,
    }
}

/// `promises.stats(days)` when `outcomes_of` serves.
fn stats_cmd(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let root = ctx.root.as_path();
    let days = args.int("days")?;
    if !(1..=3660).contains(&days) {
        return Some(out(
            "error: promise stats needs --days between 1 and 3660 (breaks US-99)\n".to_owned(),
            2,
        ));
    }
    pin_clock(root)?;
    let today = ordinal_of(&careerbrew_workday::today().to_string())?;
    let start = today - (days - 1);
    let every = records(root)?;
    let invalid: Vec<String> = outcomes_of(&every)?
        .into_iter()
        .map(|r| r.promise)
        .collect();
    let (mut made, mut on_time, mut late, mut moved, mut dropped) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut durations: Vec<i64> = Vec::new();
    for (name, row) in &every {
        // `outcomes_of` checked each record is a map with unique keys.
        let Canon::Map(row) = row else {
            return None;
        };
        let get = |key: &str| lookup(row, key).ok().flatten();
        if !is_str(get("kind"), "promise") {
            continue;
        }
        let promise = map_or_empty(get("promise"))?;
        let pget = |key: &str| lookup(promise, key).ok().flatten();
        let Ok(made_day) = stamp_day(stored_text(pget("made_at"))?, root)? else {
            continue;
        };
        if !(start <= made_day && made_day <= today) || truthy(pget("pm_removed"))? {
            continue;
        }
        let is_invalid = invalid.contains(name);
        made += 1;
        if truthy(pget("pm_moved"))? && !is_invalid {
            moved += 1;
        }
        let archived = truthy(get("archived_at"))?;
        if is_str(pget("closed"), "dropped") && archived {
            dropped += 1;
        }
        if is_str(pget("closed"), "done") && archived && !is_invalid {
            // A truthy decision makes the record invalid (`outcomes_of`),
            // so the decision branch never runs here.
            let kept = if truthy(pget("manual_kept_day"))? {
                plain_day(stored_text(pget("manual_kept_day"))?)?
            } else {
                stamp_day(stored_text(get("archived_at"))?, root)?
            };
            let Ok(kept) = kept else {
                continue;
            };
            if !(made_day <= kept && kept <= today) {
                continue;
            }
            durations.push(kept - made_day);
            let due = match lookup(promise, "original_due").ok()? {
                Some(value) => stored_text(Some(value))?,
                None => stored_text(pget("due"))?,
            };
            let Ok(due) = plain_day(due)? else {
                continue;
            };
            if kept <= due {
                on_time += 1;
            } else {
                late += 1;
            }
        }
    }
    durations.sort_unstable();
    let median = match durations.len() {
        0 => None,
        n if n % 2 == 1 => Some(durations[n / 2].to_string()),
        n => {
            let sum = durations[n / 2 - 1] + durations[n / 2];
            Some(format!("{}.{}", sum / 2, if sum % 2 == 0 { 0 } else { 5 }))
        }
    };
    let counts = [
        ("made", made),
        ("kept_on_time", on_time),
        ("kept_late", late),
        ("moved", moved),
        ("dropped", dropped),
        ("not_mentioned", 0),
    ];
    let mut s = String::new();
    if args.flag("json") {
        s.push('{');
        for (key, value) in counts {
            s.push_str(&format!("\"{key}\": {value}, "));
        }
        s.push_str(&format!(
            "\"median_days_to_keep\": {}}}\n",
            median.as_deref().unwrap_or("null")
        ));
    } else {
        for (key, value) in counts {
            s.push_str(&format!("{key}: {value}\n"));
        }
        s.push_str(&format!(
            "median_days_to_keep: {}\n",
            median.as_deref().unwrap_or("unknown")
        ));
    }
    Some(out(s, 0))
}

/// `work promise accept` without `--yes`: Python refuses before it reads
/// anything. With `--yes` it checks that stdin and stdout are terminals,
/// which this child cannot see under the console script, so it refuses.
fn accept_cmd(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() || args.flag("yes") {
        return None;
    }
    Some(out(
        "PM promise acceptance needs owner confirmation\n".to_owned(),
        2,
    ))
}
