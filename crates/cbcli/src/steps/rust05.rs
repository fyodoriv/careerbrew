//! Native CLI handlers owned by `rust-05` (see `steps/mod.rs`): the storage
//! port's settings and approval-receipt commands.
//!
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

fn scalar(value: &Canon) -> Option<String> {
    match value {
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
