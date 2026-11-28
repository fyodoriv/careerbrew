//! Native CLI handlers owned by `unowned` (see `steps/mod.rs`): the run
//! page readers (`report latest`, `report list`) and the data folder
//! commands (`data encrypt`, `data decrypt`, `data show`).
//!
//! Each handler refuses (exit 64, Python runs it) before any write when a
//! case could differ from Python:
//! - a redaction input exists (`Ctx::redact_inputs_present`): `say` would
//!   then rewrite printed text;
//! - a run page summary Python would read, sort or print differently (an
//!   unsure JSON form, a duplicate key, a number Python re-spells, a tie in
//!   the sort, a missing field Python would crash on);
//! - a path Python's `pathlib` would print differently, a symlink in the
//!   data folder, a replay checkout, or a backup key this module cannot
//!   prove usable;
//! - non-ASCII output when stdout may not be UTF-8 in Python.
use super::Ctx;
use crate::args::Parsed;
use crate::registry::Json;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_unowned.rs`.
pub const COMMANDS: &[&str] = &[
    "report.latest",
    "report.list",
    "data.encrypt",
    "data.decrypt",
    "data.show",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "report.latest" => report_latest(args, ctx),
        "report.list" => report_list(args, ctx),
        "data.encrypt" => data_encrypt(ctx),
        "data.decrypt" => data_decrypt(args, ctx),
        "data.show" => data_show(args, ctx),
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

/// True when `pathlib.Path` prints `path` unchanged.
fn plain_path(path: &str) -> bool {
    !(path.is_empty()
        || path.contains("//")
        || path.contains("/./")
        || path.starts_with("./")
        || path.ends_with("/.")
        || (path.len() > 1 && path.ends_with('/'))
        || path == ".")
}

/// True when Python's stdout encodes UTF-8: no `PYTHONIOENCODING`, and a
/// UTF-8 or C/POSIX (coerced to UTF-8) locale, or UTF-8 mode on.
fn utf8_stdout() -> bool {
    let get = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    if get("PYTHONIOENCODING").is_some() {
        return false;
    }
    match get("PYTHONUTF8").as_deref() {
        Some("1") => return true,
        Some(_) => return false,
        None => {}
    }
    let locale = get("LC_ALL")
        .or_else(|| get("LC_CTYPE"))
        .or_else(|| get("LANG"));
    match locale {
        None => true,
        Some(value) => {
            let lower = value.to_ascii_lowercase();
            value == "C" || value == "POSIX" || lower.contains("utf-8") || lower.contains("utf8")
        }
    }
}

/// The printed text, or None when Python could print it differently.
fn printable(text: String) -> Option<String> {
    (text.is_ascii() || utf8_stdout()).then_some(text)
}

struct NowClock;

impl careerbrew_vault::events::Clock for NowClock {
    fn stamp(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        crate::events::stamp(secs)
    }

    fn version(&self) -> String {
        crate::events::version().to_owned()
    }
}

/// `events.emit(event, status=..., items=...)`; a failed log write is
/// ignored, as Python's command output is already decided.
fn emit(root: &Path, event: &str, status: &str, items: Option<usize>) {
    let mut fields = vec![(Canon::Str("status".into()), Canon::Str(status.into()))];
    if let Some(items) = items {
        fields.push((
            Canon::Str("items".into()),
            Canon::Int((items as u64).into()),
        ));
    }
    let _ = careerbrew_vault::events::emit(
        root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        event,
        &Canon::Map(fields),
        &NowClock,
    );
}

// ---------- run pages (`report.py`) ----------

const NO_PAGES: &str = "no run pages yet: run scripts/checkup.sh, `careerbrew`, or `careerbrew review` (the run_pages setting must be on)";

fn digits(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

/// `^[a-z][a-z0-9-]{0,30}$` on an ASCII string.
fn run_word(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=31).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `report.ID_RE`: `^[a-z][a-z0-9-]{0,30}-\d{8}-\d{6}(?:-\d{1,9})?$`, for
/// an ASCII stem with no control character (so `$` cannot match before a
/// trailing newline).
fn page_id(stem: &str) -> bool {
    let base = |text: &str| -> bool {
        // <word>-<8 digits>-<6 digits>
        let Some((head, six)) = text.rsplit_once('-') else {
            return false;
        };
        let Some((word, eight)) = head.rsplit_once('-') else {
            return false;
        };
        digits(six, 6, 6) && digits(eight, 8, 8) && run_word(word)
    };
    if base(stem) {
        return true;
    }
    match stem.rsplit_once('-') {
        Some((head, tail)) => digits(tail, 1, 9) && base(head),
        None => false,
    }
}

/// True when no string in `text` holds a raw control character, which
/// Python's strict `json.loads` rejects and the registry reader accepts.
fn strict_strings(text: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for byte in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            } else if byte < 0x20 {
                return false;
            }
        } else if byte == b'"' {
            in_string = true;
        }
    }
    !in_string
}

/// True when Python's `json.dumps(json.loads(text))` spells the number as
/// `text`: a plain int, or a float in the range `repr` writes without an
/// exponent and in its shortest form.
fn python_number(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let int_part = body.split('.').next().unwrap_or("");
    let plain_int = int_part == "0"
        || (!int_part.is_empty() && !int_part.starts_with('0') && digits(int_part, 1, 300));
    if !plain_int {
        return false;
    }
    match body.split_once('.') {
        None => text != "-0",
        Some((_, frac)) => {
            if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
            let Ok(value) = text.parse::<f64>() else {
                return false;
            };
            let size = value.abs();
            if !value.is_finite() || size >= 1e16 || (size != 0.0 && size < 1e-4) {
                return false;
            }
            let mut repr = format!("{value}");
            if !repr.contains('.') {
                repr.push_str(".0");
            }
            repr == text
        }
    }
}

/// True for a value Python reads and writes back the same way: canonical
/// numbers and no duplicate object keys.
fn round_trips(value: &Json) -> bool {
    match value {
        Json::Number(text) => python_number(text),
        Json::Array(items) => items.iter().all(round_trips),
        Json::Object(pairs) => pairs.iter().enumerate().all(|(i, (key, item))| {
            !pairs[..i].iter().any(|(other, _)| other == key) && round_trips(item)
        }),
        _ => true,
    }
}

struct Summary {
    json: Json,
    seq: i64,
    at: String,
}

impl Summary {
    fn get(&self, key: &str) -> Option<&Json> {
        self.json.get(key)
    }
}

/// `report.all_summaries()`: every kept summary, newest first. None when
/// Python could read, keep or order one differently.
fn summaries(folder: &Path) -> Option<Vec<Summary>> {
    let mut rows = Vec::new();
    let entries = match std::fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(rows),
        Err(_) => return None,
    };
    for entry in entries {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".json") {
            continue;
        }
        if !name.is_ascii() || !name.ends_with(".json") || name.bytes().any(|b| b < 0x20) {
            return None;
        }
        let stem = &name[..name.len() - ".json".len()];
        if stem.is_empty() || !page_id(stem) {
            continue;
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() {
            return None;
        }
        let text = String::from_utf8(std::fs::read(&path).ok()?).ok()?;
        if !strict_strings(&text) {
            return None;
        }
        let Json::Object(mut pairs) = crate::registry::parse(&text).ok()? else {
            return None;
        };
        if !round_trips(&Json::Object(pairs.clone())) {
            return None;
        }
        // `{**json, "id": stem}`: a read id keeps its place.
        match pairs.iter_mut().find(|(key, _)| key == "id") {
            Some((_, value)) => *value = Json::Str(stem.to_owned()),
            None => pairs.push(("id".to_owned(), Json::Str(stem.to_owned()))),
        }
        let json = Json::Object(pairs);
        let seq = match json.get("seq") {
            None => 0,
            Some(Json::Number(text)) if !text.contains('.') => text.parse().ok()?,
            Some(_) => return None,
        };
        let at = match json.get("at") {
            None => String::new(),
            Some(Json::Str(text)) => text.clone(),
            Some(_) => return None,
        };
        rows.push(Summary { json, seq, at });
    }
    rows.sort_by(|a, b| (b.seq, &b.at).cmp(&(a.seq, &a.at)));
    // Python's sort is stable over the directory order; a tie would make
    // the order depend on it.
    if rows
        .windows(2)
        .any(|pair| pair[0].seq == pair[1].seq && pair[0].at == pair[1].at)
    {
        return None;
    }
    Some(rows)
}

/// The data root as Python prints it, or None when `pathlib` would fold it.
fn shown_root(ctx: &Ctx) -> Option<String> {
    let text = ctx.root.to_str()?.to_owned();
    plain_path(&text).then_some(text)
}

/// `report.folder()`, created as Python creates it.
fn reports_folder(ctx: &Ctx) -> PathBuf {
    ctx.root.join("out").join("reports")
}

fn text_of(value: Option<&Json>) -> Option<&str> {
    value.and_then(Json::as_str)
}

/// `x.get(key)` used as a truth value then printed: None for a falsy value,
/// the text for a non-empty string; any other type refuses (outer None).
fn optional_text(value: Option<&Json>) -> Option<Option<&str>> {
    match value {
        None | Some(Json::Null) => Some(None),
        Some(Json::Str(text)) if text.is_empty() => Some(None),
        Some(Json::Str(text)) => Some(Some(text)),
        Some(_) => None,
    }
}

fn items(value: Option<&Json>) -> Option<&[Json]> {
    value.and_then(Json::as_array)
}

/// `report.text_lines(s)`.
fn text_lines(s: &Summary) -> Option<Vec<String>> {
    let head = match optional_text(s.get("headline"))? {
        Some(text) => format!("{text}; "),
        None => String::new(),
    };
    let done = items(s.get("done"))?;
    let blockers = items(s.get("blockers"))?;
    let next = items(s.get("next"))?;
    let mut out = vec![format!(
        "{}: {} at {}: {head}{} done, {} blocker(s), {} next action(s)",
        text_of(s.get("run"))?,
        text_of(s.get("status"))?,
        text_of(s.get("at"))?,
        done.len(),
        blockers.len(),
        next.len()
    )];
    for d in done {
        let mut line = format!("done     {}", text_of(d.get("label"))?);
        if let Some(evidence) = optional_text(d.get("evidence"))? {
            line.push_str(&format!(": {evidence}"));
        }
        out.push(line);
    }
    for b in blockers {
        let mut line = format!(
            "blocker  {}: {}",
            text_of(b.get("label"))?,
            text_of(b.get("why"))?
        );
        if let Some(command) = optional_text(b.get("command"))? {
            line.push_str(&format!(" -> fix: {command}"));
        }
        out.push(line);
    }
    for (n, x) in next.iter().enumerate() {
        let mut line = format!("next {}. {}", n + 1, text_of(x.get("label"))?);
        if let Some(command) = optional_text(x.get("command"))? {
            line.push_str(&format!(" -> {command}"));
        }
        out.push(line);
    }
    Some(out)
}

fn report_list(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let top = args.int("top")?;
    let json = args.flag("json");
    let root = shown_root(ctx)?;
    let folder = reports_folder(ctx);
    let rows = summaries(&folder)?;
    // `rows[:top]`, a negative `top` counting from the end.
    let len = rows.len() as i64;
    let keep = if top >= 0 {
        top.min(len)
    } else {
        (len + top).max(0)
    } as usize;
    let rows = &rows[..keep];
    let mut out = String::new();
    if json {
        let array = Json::Array(rows.iter().map(|s| s.json.clone()).collect());
        out.push_str(&crate::render::py_dumps(&array));
        out.push('\n');
    } else {
        for s in rows {
            out.push_str(&format!(
                "{:<36} {:<8} {} done, {} blocker(s), {} next\n",
                text_of(s.get("id"))?,
                text_of(s.get("status"))?,
                items(s.get("done"))?.len(),
                items(s.get("blockers"))?.len(),
                items(s.get("next"))?.len()
            ));
        }
        if rows.is_empty() {
            out.push_str(NO_PAGES);
        } else {
            out.push_str(&format!(
                "{} run page(s) in {root}/out/reports; open one: careerbrew open --report <id>",
                rows.len()
            ));
        }
        out.push('\n');
    }
    let out = printable(out)?;
    std::fs::create_dir_all(&folder).ok()?;
    ok(out, 0)
}

fn report_latest(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let run = args.text("run");
    let json = args.flag("json");
    let root = shown_root(ctx)?;
    let folder = reports_folder(ctx);
    let rows = summaries(&folder)?;
    let found = rows.iter().find(|s| match run {
        None => true,
        Some(run) => text_of(s.get("run")) == Some(run),
    });
    let (out, code) = match found {
        None => {
            let mut have = Vec::new();
            for s in &rows {
                have.push(text_of(s.get("run"))?.to_owned());
            }
            have.sort();
            have.dedup();
            let line = match run {
                Some(run) if !run.is_empty() && !have.is_empty() => {
                    format!(
                        "no run page for {run} (runs with pages: {})",
                        have.join(", ")
                    )
                }
                _ => NO_PAGES.to_owned(),
            };
            (format!("{line}\n"), 1)
        }
        Some(s) if json => {
            let mut out = crate::render::py_dumps(&s.json);
            out.push('\n');
            (out, 0)
        }
        Some(s) => {
            let mut out = format!("page: {root}/out/reports/{}.html\n", text_of(s.get("id"))?);
            for line in text_lines(s)? {
                out.push_str(&line);
                out.push('\n');
            }
            (out, 0)
        }
    };
    let out = printable(out)?;
    std::fs::create_dir_all(&folder).ok()?;
    ok(out, code)
}

// ---------- the data folder (`vault.py`) ----------

/// The store context, when the key path prints and resolves as in Python
/// and the vault environment is the default.
fn vault_context(ctx: &Ctx) -> Option<careerbrew_vault::transfer::Context> {
    if std::env::var_os("CAREERBREW_ENCRYPT").is_some() {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    if !home.starts_with('/') || !plain_path(&home) {
        return None;
    }
    if let Some(value) = std::env::var_os("CAREERBREW_KEY_FILE") {
        let value = value.into_string().ok()?;
        if !value.starts_with('/') || !plain_path(&value) {
            return None;
        }
    }
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    // The key path is printed: it must print as Python's `str(Path)`.
    let key_file = context.key_file.to_str()?;
    if context.root != ctx.root || !(key_file.is_ascii() || utf8_stdout()) {
        return None;
    }
    Some(context)
}

/// The backup key text when the key file holds exactly one Fernet key that
/// lies outside the data root, as Python's `backup.load_key` reads it.
fn usable_key(context: &careerbrew_vault::transfer::Context) -> Option<String> {
    let meta = std::fs::symlink_metadata(&context.key_file).ok()?;
    if !meta.is_file() {
        return None;
    }
    let key_dir = context.key_file.parent()?.canonicalize().ok()?;
    let root = context.root.canonicalize().ok()?;
    if key_dir.starts_with(&root) {
        return None;
    }
    let raw = std::fs::read(&context.key_file).ok()?;
    let raw = raw.strip_suffix(b"\n").unwrap_or(&raw);
    let key = std::str::from_utf8(raw).ok()?;
    let shaped = key.len() == 44
        && key.ends_with('=')
        && key[..43]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !shaped || careerbrew_vault::seal(key, b"").is_err() {
        return None;
    }
    Some(key.to_owned())
}

/// True when the data root holds no symlink and no replay checkout folder
/// (`learn/`), so the native file list is Python's.
fn plain_tree(root: &Path) -> bool {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                return false;
            };
            if meta.file_type().is_symlink() || (meta.is_dir() && !walk(&entry.path())) {
                return false;
            }
        }
        true
    }
    !root.join("learn").exists() && walk(root)
}

fn encrypt_line(context: &careerbrew_vault::transfer::Context, verb: &str, n: usize) -> String {
    if verb == "encrypted" {
        format!(
            "encrypted {n} file(s); encryption at rest is on. Keep the key ({}) in your password manager: without it the data cannot be opened\n",
            context.key_file.display()
        )
    } else {
        format!(
            "decrypted {n} file(s); encryption at rest is off (`careerbrew data encrypt` turns it back on)\n"
        )
    }
}

fn data_encrypt(ctx: &Ctx) -> Option<Outcome> {
    let context = vault_context(ctx)?;
    usable_key(&context)?;
    if !plain_tree(&ctx.root) {
        return None;
    }
    let store = context.store();
    let targets = careerbrew_vault::bulk::files(&store).ok()?;
    for path in &targets {
        let bundle = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .is_some_and(|s| s == "cbk" || s == "cbsync");
        // A bundle is Python's `VaultError`; a file that starts like a
        // Fernet token is decided by Python's own base64 reader.
        let raw = std::fs::read(path).ok()?;
        if bundle || raw.first() == Some(&b'g') {
            return None;
        }
    }
    let n = careerbrew_vault::bulk::encrypt_all(&store).ok()?;
    emit(&ctx.root, "data.encrypt", "on", Some(n));
    ok(encrypt_line(&context, "encrypted", n), 0)
}

fn data_decrypt(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if !args.flag("yes") {
        return ok(
            "this writes every data file as plaintext and turns encryption off; rerun with --yes to confirm\n".to_owned(),
            1,
        );
    }
    let context = vault_context(ctx)?;
    let key = usable_key(&context)?;
    if !plain_tree(&ctx.root) {
        return None;
    }
    let store = context.store();
    // Every sealed file must open before the first one is rewritten.
    for path in careerbrew_vault::bulk::files(&store).ok()? {
        let raw = std::fs::read(&path).ok()?;
        if raw.starts_with(careerbrew_vault::MAGIC)
            && careerbrew_vault::open(Some(&key), &raw).is_err()
        {
            return None;
        }
    }
    let n = careerbrew_vault::bulk::decrypt_all(&store).ok()?;
    emit(&ctx.root, "data.encrypt", "off", Some(n));
    ok(encrypt_line(&context, "decrypted", n), 0)
}

/// The data file `name` names: plain lowercase ASCII words with no symlink
/// on the way, so Python's `resolve()` and the native one agree and the
/// relative path prints as typed.
fn plain_name(root: &Path, name: &str) -> bool {
    if name.is_empty()
        || !name.is_ascii()
        || name
            .bytes()
            .any(|b| b.is_ascii_uppercase() || b < 0x20 || b == 0x7f)
    {
        return false;
    }
    let mut path = root.to_path_buf();
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    true
}

fn data_show(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `--keys` parses YAML; that outline stays Python. A terminal may show
    // business text (`may_show_business`).
    if args.flag("keys") || ctx.env.stdout_is_terminal || !utf8_stdout() {
        return None;
    }
    let file = args.text("file")?;
    let name = file.strip_prefix("private/").unwrap_or(file);
    if !plain_name(&ctx.root, name) {
        return None;
    }
    let context = vault_context(ctx)?;
    let first = name.split('/').next().unwrap_or("");
    let personal = !name.contains('/') || matches!(first, "jobs" | "tutor" | "prep" | "linkedin");
    let target = ctx.root.join(name);
    if personal && target.is_file() {
        let raw = std::fs::read(&target).ok()?;
        if raw.starts_with(careerbrew_vault::MAGIC) {
            let key = usable_key(&context)?;
            careerbrew_vault::open(Some(&key), &raw).ok()?;
        }
    }
    let store = context.store();
    let outcome = careerbrew_vault::cli::cmd_data_show(
        &store,
        file,
        false,
        false,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        &NowClock,
    )
    .ok()?;
    ok(outcome.stdout, i32::from(outcome.code))
}
