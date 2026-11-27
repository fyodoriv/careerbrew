//! Native CLI handlers owned by `rust-17` (see `steps/mod.rs`): the
//! results-of-the-day count (`day results`) and the promise list (`work
//! promise list`).
//!
//! Both only read. They refuse (exit 64, Python runs it) when Python could
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
pub const COMMANDS: &[&str] = &["day.results", "work.promise.list"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let started = std::time::Instant::now();
    match name {
        "day.results" => results(args, ctx),
        "work.promise.list" => logged("work.promise", promise_list(args, ctx)?, ctx, started),
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
        // date); only the plain form is served.
        Some(text) => {
            if !day_form(text) {
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
