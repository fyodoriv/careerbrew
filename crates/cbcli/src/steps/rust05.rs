//! Native CLI handlers owned by `rust-05` (see `steps/mod.rs`): the storage
//! port's settings and approval-receipt commands.
//!
//! - `config show`: `config.effective` through the rust-02 settings rows.
//! - `config set <key> <value>`: `config.set_value` (the rust-02
//!   validators) under the settings lock, and its `KeyError` and
//!   `ValueError` lines.
//! - `config unset <key>`: `config.unset` under the settings lock.
//! - `approvals list|guidance|show`: readers over `Receipts` (no lock).
//! - `approvals decide`: `Receipts::decide` under the receipt lock.
//!
//! Refusals (exit 64, Python runs the command), always before any write:
//! a redaction input, a data root or key path the vault context cannot
//! resolve the way Python does, any read or record error, a printed value
//! that is not a string, and a write that would seal a file (its bytes
//! cannot match: every seal has a new IV) or that needs a YAML form outside
//! the small subset `settings_yaml` writes byte-equal to PyYAML.
use super::Ctx;
use crate::args::Parsed;
use crate::{events, Outcome};
use careerbrew_policy::approvals;
use careerbrew_policy::vectors::Out;
use careerbrew_policy::Rules;
use careerbrew_vault::canon::Canon;
use careerbrew_vault::receipts::{ReceiptError, Receipts, Runtime};
use careerbrew_vault::store::Store;
use careerbrew_vault::transfer::Context;
use std::io::{IsTerminal, Write};
use std::path::Path;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust05.rs`.
pub const COMMANDS: &[&str] = &[
    "config.show",
    "config.set",
    "config.unset",
    "approvals.list",
    "approvals.guidance",
    "approvals.show",
    "approvals.decide",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // Every command prints text through `privacy.redact`.
    if ctx.redact_inputs_present() {
        return None;
    }
    let vault = vault_context()?;
    let store = vault.store();
    match name {
        "config.show" => config_show(ctx, &store),
        "config.set" => config_set(args, ctx, &store),
        "config.unset" => config_unset(args, ctx, &store),
        "approvals.list" => approvals_list(&store),
        "approvals.guidance" => approvals_guidance(&store),
        "approvals.show" => approvals_show(args, ctx, &store),
        "approvals.decide" => approvals_decide(args, ctx, &store),
        _ => None,
    }
}

fn ok(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

/// The vault context from the environment, when it resolves the key path
/// as Python's `keyfile.key_path` does (`Path.home()` and `expanduser`).
fn vault_context() -> Option<Context> {
    if std::env::var("HOME").map_or(true, |home| home.is_empty()) {
        return None;
    }
    if let Ok(key) = std::env::var("CAREERBREW_KEY_FILE") {
        if key.starts_with('~') && key != "~" && !key.starts_with("~/") {
            return None;
        }
    }
    Context::from_env().ok()
}

// ---------- event rows ----------

/// Appends one row to `<data root>/logs/events.jsonl`, as `events.write_row`.
fn append_row(ctx: &Ctx, row: &str) {
    if !ctx.env.caps.events_on {
        return;
    }
    let logs = ctx.root.join("logs");
    if std::fs::create_dir_all(&logs).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("events.jsonl"))
    {
        let _ = file.write_all(row.as_bytes());
    }
}

fn now_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    events::stamp(secs)
}

/// One sorted row of fixed id fields plus `ts` and `v`, as `events.emit`
/// writes it. `fields` are in sorted key order and all sort before `ts`.
fn row(fields: &[(&str, &str)], tail: &[(&str, &str)]) -> String {
    let mut out = String::from("{");
    let ts = now_stamp();
    let all = fields
        .iter()
        .copied()
        .chain([("ts", ts.as_str()), ("v", events::version())])
        .chain(tail.iter().copied());
    for (i, (key, value)) in all.enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        crate::render::py_string(key, &mut out);
        out.push_str(": ");
        crate::render::py_string(value, &mut out);
    }
    out.push_str("}\n");
    out
}

// ---------- config unset ----------

/// What `config.unset` does with the settings it reads.
enum Unset {
    /// Nothing to write: no file, the key absent, or its value None.
    Keep,
    /// Write this text to settings.yaml.
    Write(String),
}

fn unset_plan(store: &Store, key: &str) -> Option<Unset> {
    let data = careerbrew_vault::settings::read_settings(store).ok()?;
    // `dict.pop` on anything else raises in Python.
    let Canon::Map(items) = data else {
        return None;
    };
    let names: Vec<&str> = items
        .iter()
        .map(|(k, _)| match k {
            Canon::Str(k) => Some(k.as_str()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    // Python's loader keeps one entry per key; a repeated key here would be
    // an order question this handler does not answer.
    if names
        .iter()
        .enumerate()
        .any(|(i, n)| names[..i].contains(n))
    {
        return None;
    }
    let Some(at) = names.iter().position(|n| *n == key) else {
        return Some(Unset::Keep);
    };
    if items[at].1 == Canon::Null {
        return Some(Unset::Keep);
    }
    let rest: Vec<(&str, &Canon)> = items
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != at)
        .map(|(i, (_, v))| (names[i], v))
        .collect();
    // A sealed write differs on every run; leave it to Python.
    if store.encryption_enabled() {
        return None;
    }
    settings_yaml(&rest).map(Unset::Write)
}

/// A plain scalar PyYAML writes unquoted and reads back as the same string.
fn plain(text: &str) -> bool {
    const WORDS: &[&str] = &["y", "n", "yes", "no", "true", "false", "on", "off", "null"];
    let mut chars = text.chars();
    text.len() <= 60
        && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !WORDS.contains(&text.to_ascii_lowercase().as_str())
}

fn int_text(value: &Canon) -> Option<String> {
    let Canon::Int(n) = value else {
        return None;
    };
    let text = n.to_string();
    let digits = text.strip_prefix('-').unwrap_or(&text);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(text)
}

/// `HH:MM` in ASCII digits, the form the `_hhmm` validator accepts.
fn hhmm(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 5
        && match b[0] {
            b'0' | b'1' => b[1].is_ascii_digit(),
            b'2' => (b'0'..=b'3').contains(&b[1]),
            _ => false,
        }
        && b[2] == b':'
        && (b'0'..=b'5').contains(&b[3])
        && b[4].is_ascii_digit()
}

fn scalar(value: &Canon) -> Option<String> {
    match value {
        // PyYAML quotes a time YAML 1.1 reads as a base-60 integer
        // (`[1-9][0-9_]*(:[0-5]?[0-9])+`): `'19:00'`, while `08:30` stays
        // plain (checked for all 1440 times).
        Canon::Str(s) if hhmm(s) && s.starts_with(['1', '2']) => Some(format!("'{s}'")),
        Canon::Str(s) if hhmm(s) => Some(s.clone()),
        Canon::Null => Some("null".into()),
        Canon::Bool(b) => Some(if *b { "true" } else { "false" }.into()),
        Canon::Int(_) => int_text(value),
        Canon::Str(s) if s.is_empty() => Some("''".into()),
        Canon::Str(s) if plain(s) => Some(s.clone()),
        _ => None,
    }
}

/// `yaml.safe_dump(data, sort_keys=False, allow_unicode=True)` for a map of
/// identifier keys whose values are null, booleans, integers, empty or
/// plain strings, or lists of those. Anything else returns None.
fn settings_yaml(items: &[(&str, &Canon)]) -> Option<String> {
    if items.is_empty() {
        return Some("{}\n".into());
    }
    let mut out = String::new();
    for (key, value) in items {
        let mut chars = key.chars();
        let key_ok = key.len() <= 40
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && plain(key);
        if !key_ok {
            return None;
        }
        match value {
            Canon::List(list) if list.is_empty() => out.push_str(&format!("{key}: []\n")),
            Canon::List(list) => {
                out.push_str(&format!("{key}:\n"));
                for item in list {
                    if matches!(item, Canon::Str(s) if s.is_empty()) {
                        return None;
                    }
                    out.push_str(&format!("- {}\n", scalar(item)?));
                }
            }
            _ => out.push_str(&format!("{key}: {}\n", scalar(value)?)),
        }
    }
    Some(out)
}

fn config_unset(args: &Parsed, ctx: &Ctx, store: &Store) -> Option<Outcome> {
    let started = std::time::Instant::now();
    let key = args.text("key")?;
    // Plan once before the lock file exists, so a refusal writes nothing.
    unset_plan(store, key)?;
    let lock = ctx.root.join(".settings.yaml.lock");
    careerbrew_vault::lock::with_exact_lock(&lock, || -> Option<()> {
        match unset_plan(store, key)? {
            Unset::Keep => Some(()),
            Unset::Write(text) => store
                .write_text(&ctx.root.join("settings.yaml"), &text)
                .ok(),
        }
    })
    .ok()??;
    // `cli.main` names this run `config` (the subparser's dest is `action`,
    // not `sub`), so the row is written here, not by `main`.
    let ms = started.elapsed().as_millis() as u64;
    events::emit_command(&ctx.env.caps, "config", true, ms);
    Some(Outcome {
        stdout: format!("{key} back to its default\n"),
        stderr: String::new(),
        code: 0,
        emit_event: false,
    })
}

// ---------- config show and set (the rust-02 settings rows) ----------

/// One answer of the rust-02 `settings` vector set (`careerbrew_purevec`)
/// for `op` over the stored settings, in the typed `{t, v}` form. `extra`
/// holds more input fields, already JSON text (`"key": ..., "raw": ...`).
fn settings_op(op: &str, settings: &Canon, extra: &str) -> Option<Canon> {
    let mut text = String::from("{\"vectors\": [{\"id\": \"native\", \"op\": ");
    crate::render::py_string(op, &mut text);
    text.push_str(", \"input\": {\"settings\": ");
    text.push_str(&settings.to_json().to_string());
    text.push_str(extra);
    text.push_str("}}]}");
    let fixture = text.parse().ok()?;
    let answer = careerbrew_purevec::run("settings", &fixture).ok()?;
    let output = &answer["results"][0]["output"]["ok"];
    // The answer is plain JSON, not the typed form: wrap it as a typed
    // value through its JSON text, a map of `{t, v}` leaves.
    plain_to_canon(&output.to_string())
}

/// Reads the JSON text of a plain answer (objects whose leaves are typed
/// `{t, v}` values) as a typed map, so callers use one value type.
fn plain_to_canon(json: &str) -> Option<Canon> {
    let parsed = crate::registry::parse(json).ok()?;
    from_answer(&parsed)
}

fn from_answer(value: &crate::registry::Json) -> Option<Canon> {
    use crate::registry::Json;
    // A typed leaf: an object with a string `t`.
    if let Some(Json::Str(kind)) = value.get("t") {
        let text = value.get("v");
        return Some(match (kind.as_str(), text) {
            ("null", _) => Canon::Null,
            ("bool", Some(Json::Bool(b))) => Canon::Bool(*b),
            ("int", Some(Json::Str(n))) => Canon::Int(n.parse().ok()?),
            ("str", Some(Json::Str(s))) => Canon::Str(s.clone()),
            ("list", Some(Json::Array(items))) => {
                Canon::List(items.iter().map(from_answer).collect::<Option<_>>()?)
            }
            ("map", Some(Json::Array(pairs))) => Canon::Map(
                pairs
                    .iter()
                    .map(|pair| match pair.as_array()? {
                        [k, v] => Some((from_answer(k)?, from_answer(v)?)),
                        _ => None,
                    })
                    .collect::<Option<_>>()?,
            ),
            // Floats, dates and anything else are not served.
            _ => return None,
        });
    }
    match value {
        Json::Array(items) => Some(Canon::List(
            items.iter().map(from_answer).collect::<Option<_>>()?,
        )),
        Json::Object(pairs) => Some(Canon::Map(
            pairs
                .iter()
                .map(|(k, v)| Some((Canon::Str(k.clone()), from_answer(v)?)))
                .collect::<Option<_>>()?,
        )),
        Json::Str(s) => Some(Canon::Str(s.clone())),
        Json::Bool(b) => Some(Canon::Bool(*b)),
        Json::Null => Some(Canon::Null),
        Json::Number(_) => None,
    }
}

fn map_get<'a>(map: &'a Canon, key: &str) -> Option<&'a Canon> {
    match map {
        Canon::Map(items) => items
            .iter()
            .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
            .map(|(_, v)| v),
        _ => None,
    }
}

fn map_text<'a>(map: &'a Canon, key: &str) -> Option<&'a str> {
    match map_get(map, key)? {
        Canon::Str(s) => Some(s),
        _ => None,
    }
}

/// Python's `repr()` of a string, for the plain ASCII strings it quotes
/// with `'` and no escapes; anything else returns None.
fn simple_repr(text: &str) -> Option<String> {
    text.chars()
        .all(|c| (' '..='~').contains(&c) && c != '\'' && c != '\\')
        .then(|| format!("'{text}'"))
}

/// Python's `str()` of a settings value: None, booleans, integers, strings,
/// and lists of those (whose strings print with `repr`). Floats, maps,
/// dates and strings `repr` would escape return None.
fn py_str(value: &Canon) -> Option<String> {
    Some(match value {
        Canon::Null => "None".into(),
        Canon::Bool(b) => (if *b { "True" } else { "False" }).into(),
        Canon::Int(n) => n.to_string(),
        Canon::Str(s) => s.clone(),
        Canon::List(items) => {
            let parts = items
                .iter()
                .map(|item| match item {
                    Canon::Str(s) => simple_repr(s),
                    other => py_str(other),
                })
                .collect::<Option<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        _ => return None,
    })
}

/// The stored settings, when they are a map of distinct string keys (the
/// only shape `config.effective` and `set_value` handle the same way here).
fn stored_settings(store: &Store) -> Option<Canon> {
    let data = careerbrew_vault::settings::read_settings(store).ok()?;
    let Canon::Map(items) = &data else {
        return None;
    };
    let mut names: Vec<&str> = Vec::new();
    for (k, _) in items {
        let Canon::Str(k) = k else {
            return None;
        };
        if names.contains(&k.as_str()) {
            return None;
        }
        names.push(k);
    }
    Some(data)
}

fn config_show(ctx: &Ctx, store: &Store) -> Option<Outcome> {
    let started = std::time::Instant::now();
    let settings = stored_settings(store)?;
    let Canon::List(rows) = settings_op("effective", &settings, "")? else {
        return None;
    };
    let mut out = String::new();
    for row in &rows {
        let key = map_text(row, "key")?;
        let value = py_str(map_get(row, "value")?)?;
        let source = map_text(row, "source")?;
        let help = map_text(row, "help")?;
        out.push_str(&format!("{key:<18} {value:<24} {source:<14} {help}\n"));
    }
    // Python's stdout encoding is the locale's; ASCII prints the same in all.
    if !out.is_ascii() {
        return None;
    }
    // `cli.main` names this run `config` (see `config_unset`).
    let ms = started.elapsed().as_millis() as u64;
    events::emit_command(&ctx.env.caps, "config", true, ms);
    Some(Outcome {
        stdout: out,
        stderr: String::new(),
        code: 0,
        emit_event: false,
    })
}

/// What `config.set_value` does, from the rust-02 `set` answer.
enum SetPlan {
    /// `KeyError` or `ValueError` with its message: no lock, no read.
    Raised(&'static str, String),
    /// The parsed value, and the settings map to write.
    Write(Canon, Canon),
}

fn set_plan(settings: &Canon, key: &str, raw: &str) -> Option<SetPlan> {
    let mut extra = String::from(", \"key\": ");
    crate::render::py_string(key, &mut extra);
    extra.push_str(", \"raw\": ");
    crate::render::py_string(raw, &mut extra);
    let answer = settings_op("set", settings, &extra)?;
    if let Some(class) = map_text(&answer, "raised") {
        let class = match class {
            "KeyError" => "KeyError",
            "ValueError" => "ValueError",
            _ => return None,
        };
        return Some(SetPlan::Raised(
            class,
            map_text(&answer, "message")?.to_owned(),
        ));
    }
    // `refused: delegated` (a validator that calls workday modules or
    // tzdata) and any other shape go to Python.
    let value = map_get(&answer, "value")?.clone();
    let written = map_get(&answer, "written")?.clone();
    Some(SetPlan::Write(value, written))
}

/// The settings.yaml text for a written map, in `settings_yaml`'s subset.
fn written_yaml(written: &Canon) -> Option<String> {
    let Canon::Map(items) = written else {
        return None;
    };
    let pairs: Vec<(&str, &Canon)> = items
        .iter()
        .map(|(k, v)| match k {
            Canon::Str(k) => Some((k.as_str(), v)),
            _ => None,
        })
        .collect::<Option<_>>()?;
    settings_yaml(&pairs)
}

fn config_set(args: &Parsed, ctx: &Ctx, store: &Store) -> Option<Outcome> {
    let started = std::time::Instant::now();
    let key = args.text("key")?;
    let raw = args.text("value")?;
    // A key whose `repr` is plain, and a value of printable ASCII, so every
    // message and printed value reads the same in both languages.
    if !simple_id(key) || !raw.chars().all(|c| (' '..='~').contains(&c)) {
        return None;
    }
    // Python validates before it takes the lock or reads the file, so the
    // first answer uses no stored settings: an error reads nothing.
    let empty = Canon::Map(Vec::new());
    match set_plan(&empty, key, raw)? {
        SetPlan::Raised(class, message) => return config_set_error(ctx, class, &message),
        SetPlan::Write(..) => {}
    }
    // A sealed write differs on every run; leave it to Python.
    if store.encryption_enabled() {
        return None;
    }
    // Plan the write once before the lock file exists, so a refusal
    // writes nothing; then again under the lock.
    let plan = |store: &Store| -> Option<(Canon, String)> {
        let settings = stored_settings(store)?;
        match set_plan(&settings, key, raw)? {
            SetPlan::Write(value, written) => Some((value, written_yaml(&written)?)),
            SetPlan::Raised(..) => None,
        }
    };
    let (value, _) = plan(store)?;
    let shown = py_str(&value)?;
    let line = format!("{key} = {shown} (saved to settings.yaml)\n");
    if !line.is_ascii() {
        return None;
    }
    let lock = ctx.root.join(".settings.yaml.lock");
    careerbrew_vault::lock::with_exact_lock(&lock, || -> Option<()> {
        let (_, text) = plan(store)?;
        store
            .write_text(&ctx.root.join("settings.yaml"), &text)
            .ok()
    })
    .ok()??;
    append_row(ctx, &row(&[("event", "config.set"), ("status", "ok")], &[]));
    let ms = started.elapsed().as_millis() as u64;
    events::emit_command(&ctx.env.caps, "config", true, ms);
    Some(Outcome {
        stdout: line,
        stderr: String::new(),
        code: 0,
        emit_event: false,
    })
}

/// `cli.main` on a `KeyError` or `ValueError`: `error: <str(exc)>`, one
/// `cli.error` row, exit 2, and no `cli.run` row.
fn config_set_error(ctx: &Ctx, class: &str, message: &str) -> Option<Outcome> {
    let (shown, id) = match class {
        // `str(KeyError(m))` is `repr(m)`.
        "KeyError" => (simple_repr(message)?, "keyerror"),
        _ => (message.to_owned(), "valueerror"),
    };
    let stdout = format!("error: {shown}\n");
    if !stdout
        .chars()
        .all(|c| c == '\n' || (' '..='~').contains(&c))
    {
        return None;
    }
    append_row(
        ctx,
        &row(
            &[("command", "config"), ("error", id), ("event", "cli.error")],
            &[("via", ctx.env.caps.via)],
        ),
    );
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 2,
        emit_event: false,
    })
}

// ---------- approvals ----------

/// The runtime of a read: nothing reads a clock, an id or the event log.
struct ReadRuntime;

impl Runtime for ReadRuntime {
    fn now(&self) -> String {
        String::new()
    }
    fn new_id(&self) -> String {
        String::new()
    }
    fn emit(&self, _: &str, _: &str) -> Result<(), ReceiptError> {
        Ok(())
    }
}

/// The validated records, as `approvals._records()`.
fn records(store: &Store) -> Option<Vec<Out>> {
    let loaded = Receipts::new(store, &ReadRuntime).load().ok()?;
    approvals::validate_records(&Rules::python(), &loaded).ok()
}

fn field<'a>(record: &'a Out, key: &str) -> Option<&'a Out> {
    match record {
        Out::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn text<'a>(record: &'a Out, key: &str) -> Option<&'a str> {
    match field(record, key) {
        Some(Out::Str(s)) => Some(s),
        _ => None,
    }
}

fn approvals_list(store: &Store) -> Option<Outcome> {
    let items = records(store)?;
    let mut out = String::new();
    for item in &items[items.len().saturating_sub(20)..] {
        let (id, status, title) = (
            text(item, "id")?,
            text(item, "status")?,
            text(item, "title")?,
        );
        out.push_str(&format!("{id}  {status:<9} {title}\n"));
    }
    ok(out, 0)
}

fn approvals_guidance(store: &Store) -> Option<Outcome> {
    let rows = Receipts::new(store, &ReadRuntime)
        .guidance(approvals::GUIDANCE_MAX)
        .ok()?;
    if rows.is_empty() {
        return ok(
            "no notes yet: a note on an approval decision becomes guidance agents read before they ask again\n"
                .into(),
            0,
        );
    }
    let mut out = format!(
        "{} note(s) from your decisions, newest first; agents see them when they ask:\n",
        rows.len()
    );
    for g in &rows {
        let (status, title, note) = (text(g, "status")?, text(g, "title")?, text(g, "note")?);
        out.push_str(&format!("- {status}: {title} · {note}\n"));
    }
    ok(out, 0)
}

/// An id whose Python `repr` is just the id in single quotes.
fn simple_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn approvals_show(args: &Parsed, ctx: &Ctx, store: &Store) -> Option<Outcome> {
    let id = args.text("id")?;
    let items = records(store)?;
    let Some(item) = items.iter().find(|a| text(a, "id") == Some(id)) else {
        if !simple_id(id) {
            return None;
        }
        // `cli.main`: a KeyError prints `error: <repr>`, logs `cli.error`
        // and exits 2 with no `cli.run` row.
        append_row(
            ctx,
            &row(
                &[
                    ("command", "approvals.show"),
                    ("error", "keyerror"),
                    ("event", "cli.error"),
                ],
                &[("via", ctx.env.caps.via)],
            ),
        );
        return Some(Outcome {
            stdout: format!("error: 'unknown approval {id}'\n"),
            stderr: String::new(),
            code: 2,
            emit_event: false,
        });
    };
    let (status, title) = (text(item, "status")?, text(item, "title")?);
    let mut out = format!("{id} {status}: {title}");
    match field(item, "note") {
        Some(Out::Str(note)) if !note.is_empty() => out.push_str(&format!(" · note: {note}")),
        Some(note) if note.truthy() => return None,
        _ => {}
    }
    out.push('\n');
    ok(out, if status == "approved" { 0 } else { 1 })
}

/// The clock `approvals.decide` stamps with:
/// `calendar.now_utc().isoformat(timespec="seconds")`. Served only for an
/// unset `CAREERBREW_NOW` or a whole-second ISO value, and a `timezone`
/// setting the native zone reader resolves to the same name.
fn decide_stamp(store: &Store, root: &Path) -> Option<String> {
    match std::env::var("CAREERBREW_NOW") {
        Err(std::env::VarError::NotPresent) => {}
        Ok(value) if pinned_form(&value) => {}
        _ => return None,
    }
    let settings = careerbrew_vault::settings::read_settings(store).ok()?;
    let Canon::Map(items) = &settings else {
        return None;
    };
    let configured = match items
        .iter()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == "timezone"))
        .map(|(_, v)| v)
    {
        None | Some(Canon::Null) => None,
        Some(Canon::Str(zone)) if zone.is_empty() => None,
        Some(Canon::Str(zone)) => Some(zone.clone()),
        Some(_) => return None,
    };
    if let Some(zone) = configured {
        if careerbrew_tutor::local_zone(root).name() != zone {
            return None;
        }
    }
    // The teacher clock panics on a pinned wall time that does not exist
    // (a DST gap); that run goes to Python.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let stamp = std::panic::catch_unwind(|| careerbrew_teacher::stamp(root));
    std::panic::set_hook(hook);
    stamp
        .ok()
        .filter(|s| s.len() == 25 && s.ends_with("+00:00"))
}

/// `YYYY-MM-DDTHH:MM:SS`, optionally with `Z` or `+HH:MM` / `-HH:MM`.
fn pinned_form(value: &str) -> bool {
    let b = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    let base = b.len() >= 19
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    base && match &b[19..] {
        [] | [b'Z'] => true,
        [sign, ..] if b.len() == 25 && matches!(sign, b'+' | b'-') => {
            digits(20..22) && b[22] == b':' && digits(23..25)
        }
        _ => false,
    }
}

/// The decide runtime: the stamp read before the lock, and the
/// `approvals.decide` row appended after the lock is released.
struct DecideRuntime<'a> {
    stamp: String,
    ctx: &'a Ctx<'a>,
}

impl Runtime for DecideRuntime<'_> {
    fn now(&self) -> String {
        self.stamp.clone()
    }
    fn new_id(&self) -> String {
        String::new()
    }
    fn emit(&self, event: &str, status: &str) -> Result<(), ReceiptError> {
        append_row(self.ctx, &row(&[("event", event), ("status", status)], &[]));
        Ok(())
    }
}

fn approvals_decide(args: &Parsed, ctx: &Ctx, store: &Store) -> Option<Outcome> {
    // `cli._owner_terminal`; `actions.OWNER_UI` is set only in the web app.
    let owner = std::io::stdin().is_terminal()
        || std::env::var("CAREERBREW_TEST_OWNER").as_deref() == Ok("1");
    if !owner {
        return ok(
            "only you set goals: run this in your own terminal, or use Goals in the web app\n"
                .into(),
            1,
        );
    }
    let id = args.text("id")?;
    let decision = args.text("decision")?;
    let note = match args.get("note") {
        crate::args::Value::Text(note) => note.as_str(),
        _ => return None,
    };
    // Every refusal comes before the lock file and the write: a sealed
    // write, a bad clock, or a request Python would refuse.
    if store.encryption_enabled() {
        return None;
    }
    let stamp = decide_stamp(store, &ctx.root)?;
    let loaded = Receipts::new(store, &ReadRuntime).load().ok()?;
    approvals::decide_items_with(&Rules::python(), &loaded, id, decision, note, || {
        stamp.clone()
    })
    .ok()?;
    let runtime = DecideRuntime { stamp, ctx };
    let decided = Receipts::new(store, &runtime)
        .decide(id, decision, note)
        .ok()?;
    let (id, status) = (text(&decided, "id")?, text(&decided, "status")?);
    ok(format!("{id} {status}\n"), 0)
}
