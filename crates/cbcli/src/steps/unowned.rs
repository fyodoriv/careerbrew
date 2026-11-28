//! Native CLI handlers owned by `unowned` (see `steps/mod.rs`): the run
//! page readers (`report latest`, `report list`) and the data folder
//! commands (`data encrypt`, `data decrypt`, `data show`), and the outcome
//! review (`review`) in a data folder that holds no outcome data yet.
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
    "report.public",
    "data.set",
    "review",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // `data set` prints only fixed text with `print`, never `say`; every
    // other command prints through `privacy.redact`.
    if name != "data.set" && ctx.redact_inputs_present() {
        return None;
    }
    match name {
        "report.public" => report_public(args, ctx),
        "data.set" => data_set(args, ctx),
        "report.latest" => report_latest(args, ctx),
        "report.list" => report_list(args, ctx),
        "data.encrypt" => data_encrypt(ctx),
        "data.decrypt" => data_decrypt(args, ctx),
        "data.show" => data_show(args, ctx),
        "review" => review(args, ctx),
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

// ---------- the public repo health export (`repo_health.py`) ----------

/// Python's `paths.REPO_ROOT`: the checkout the console script names, else
/// the one this binary was built from.
fn repo_root() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(std::env::VarError::NotPresent) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .ok(),
        Err(_) => None,
    }
}

/// True when no path component is `.` or `..`, so `Path.resolve()` and the
/// native `resolve` walk the same names.
fn no_dot_parts(path: &Path) -> bool {
    path.components().all(|part| {
        !matches!(
            part,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    })
}

/// `Path(text).resolve()` when Python and the native reader agree on it.
fn py_resolve(text: &str) -> Option<PathBuf> {
    let path = Path::new(text);
    (plain_path(text) && no_dot_parts(path)).then(|| careerbrew_reports::resolve(path))
}

/// True when a `.metrics` file reads the same in Python's `json.loads`
/// (with `str.splitlines` for the event log) and in the native reader: UTF-8,
/// no NaN or Infinity, no lone surrogate escape, no digit run Python's int
/// limit could reject, and no line break other than `\n` and `\r\n`.
fn metrics_text_ok(path: &Path) -> bool {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        // Both sides read a missing or unreadable file as empty.
        Err(_) => return true,
    };
    let Ok(text) = std::str::from_utf8(&raw) else {
        return false;
    };
    if text.contains("NaN") || text.contains("Infinity") || text.starts_with('\u{feff}') {
        return false;
    }
    let lower = text.to_ascii_lowercase();
    if [
        "\\ud8", "\\ud9", "\\uda", "\\udb", "\\udc", "\\udd", "\\ude", "\\udf",
    ]
    .iter()
    .any(|escape| lower.contains(escape))
    {
        return false;
    }
    let mut run = 0;
    for byte in text.bytes() {
        run = if byte.is_ascii_digit() { run + 1 } else { 0 };
        if run > 300 {
            return false;
        }
    }
    let bytes = text.as_bytes();
    for (i, ch) in text.char_indices() {
        let breaks = matches!(
            ch,
            '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if breaks || (ch == '\r' && bytes.get(i + 1) != Some(&b'\n')) {
            return false;
        }
    }
    true
}

/// `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,6})?(Z|[+-]\d{2}:\d{2})?$`
/// in ASCII: a stamp Python's `fromisoformat` and the native one read alike.
fn plain_stamp(text: &str) -> bool {
    let b = text.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    if b.len() < 19
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|i| digit(*i))
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return false;
    }
    let mut rest = &text[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=6).contains(&n) {
            return false;
        }
        rest = &frac[n..];
    }
    match rest {
        "" | "Z" => true,
        offset => {
            let o = offset.as_bytes();
            o.len() == 6
                && (o[0] == b'+' || o[0] == b'-')
                && o[1].is_ascii_digit()
                && o[2].is_ascii_digit()
                && o[3] == b':'
                && o[4].is_ascii_digit()
                && o[5].is_ascii_digit()
        }
    }
}

/// True when no object holds a duplicate key (Python keeps the last one;
/// the registry reader may keep another).
fn distinct_keys(value: &Json) -> bool {
    match value {
        Json::Array(items) => items.iter().all(distinct_keys),
        Json::Object(pairs) => pairs.iter().enumerate().all(|(i, (key, item))| {
            !pairs[..i].iter().any(|(other, _)| other == key) && distinct_keys(item)
        }),
        _ => true,
    }
}

/// True when every gate row in the event log is one the native export
/// counts as Python does: a line that parses (a torn line goes to Python),
/// a plain stamp, and no negative zero seconds.
fn gate_rows_ok(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return true;
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if !strict_strings(line) {
            return false;
        }
        let Ok(row) = crate::registry::parse(line) else {
            return false;
        };
        if !distinct_keys(&row) {
            return false;
        }
        if row.get("event").and_then(Json::as_str) != Some("check.step") {
            continue;
        }
        match row.get("ts") {
            Some(Json::Str(ts)) if plain_stamp(ts) => {}
            _ => return false,
        }
        if let Some(Json::Number(secs)) = row.get("seconds") {
            if secs.starts_with('-') && secs.parse::<f64>().is_ok_and(|x| x == 0.0) {
                return false;
            }
        }
    }
    true
}

/// Python's exception class name for an `OSError` of this kind.
fn os_error_name(error: &std::io::Error) -> &'static str {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::PermissionDenied => "PermissionError",
        ErrorKind::NotFound => "FileNotFoundError",
        ErrorKind::AlreadyExists => "FileExistsError",
        ErrorKind::IsADirectory => "IsADirectoryError",
        ErrorKind::NotADirectory => "NotADirectoryError",
        _ => "OSError",
    }
}

/// Where `--out` writes, decided before the export runs (it logs an event).
enum OutPlan {
    /// `write_rows` refuses: the target is inside this private folder.
    Inside(String),
    /// The resolved folder, which the run can create and write.
    Write(PathBuf),
}

/// The `--out` plan, or None when Python could resolve, refuse or fail it
/// differently.
fn out_plan(out: &str, ctx: &Ctx, repo_root: &Path, repo: &Path) -> Option<OutPlan> {
    use std::os::unix::fs::MetadataExt;
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| home.starts_with('/'));
    let expanded = crate::events::expand_user(out, home)?;
    let target = py_resolve(&expanded)?;
    target.to_str()?;
    let repo_private = repo.join("private");
    if !no_dot_parts(&repo_private) {
        return None;
    }
    for private in [
        careerbrew_reports::resolve(&ctx.root),
        careerbrew_reports::resolve(&repo_root.join("private")),
        careerbrew_reports::resolve(&repo_private),
    ] {
        if target.starts_with(&private) {
            return Some(OutPlan::Inside(private.to_str()?.to_owned()));
        }
    }
    // The folder (or its nearest existing parent) must be a real folder this
    // user owns and may write, so `mkdir`, `unlink` and `write_text` succeed.
    let mut base = target.clone();
    while std::fs::symlink_metadata(&base).is_err() {
        base = base.parent()?.to_path_buf();
    }
    let meta = std::fs::symlink_metadata(&base).ok()?;
    let owner = std::fs::metadata(&ctx.root).ok()?.uid();
    if !meta.is_dir() || meta.uid() != owner || meta.mode() & 0o300 != 0o300 {
        return None;
    }
    if base == target {
        for name in ["summary", "steps", "daily"] {
            match std::fs::symlink_metadata(target.join(format!("{name}.json"))) {
                Ok(meta) if meta.is_dir() => return None,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return None,
            }
        }
    }
    Some(OutPlan::Write(target))
}

/// `json.dumps(value, indent=1)` nested one level deeper, as a value inside
/// a one-key object or a one-item list.
fn nested(text: &str) -> String {
    text.replace('\n', "\n ")
}

fn public_refused(ctx: &Ctx, message: &str) -> Option<Outcome> {
    let line = printable(format!("public export refused: {message}\n"))?;
    emit(&ctx.root, "report.public", "refused", None);
    ok(line, 1)
}

fn report_public(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let days = args.int("days")?;
    let json = args.flag("json");
    let out = args.text("out").filter(|text| !text.is_empty());
    if !(1..=366).contains(&days) {
        return public_refused(ctx, "--days must be 1 to 366");
    }
    let repo_root = repo_root()?;
    // `_repo()`: `CAREERBREW_REPO_DIR or REPO_ROOT`. The domain crate reads
    // the variable, so an unset one is set to Python's checkout.
    let repo = match std::env::var("CAREERBREW_REPO_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        Ok(_) | Err(std::env::VarError::NotPresent) => {
            std::env::set_var("CAREERBREW_REPO_DIR", &repo_root);
            repo_root.clone()
        }
        Err(_) => return None,
    };
    let metrics = repo.join(".metrics");
    if !metrics_text_ok(&metrics.join("events.jsonl"))
        || !metrics_text_ok(&metrics.join("e2e-results.json"))
        || !gate_rows_ok(&metrics.join("events.jsonl"))
    {
        return None;
    }
    // The newest repo checkup page, read as `report.latest` reads it; its
    // status must be hashable for `in STATUSES`.
    let pages = summaries(&reports_folder(ctx))?;
    if let Some(page) = pages
        .iter()
        .find(|s| s.get("run").and_then(Json::as_str) == Some("repo-checkup"))
    {
        if matches!(page.get("status"), Some(Json::Array(_) | Json::Object(_))) {
            return None;
        }
    }
    let plan = match out {
        None => None,
        Some(out) => Some(out_plan(out, ctx, &repo_root, &repo)?),
    };
    let payload = match careerbrew_reports::repo_health::collect(days) {
        Ok(payload) => payload,
        // A crate-side failure Python has no message for goes to Python; it
        // fails before the export logs anything.
        Err(error) if error.0.starts_with("internal: ") || error.0.starts_with("cannot ") => {
            return None
        }
        Err(error) => return public_refused(ctx, &error.0),
    };
    let mut written = Vec::new();
    match plan {
        None => {}
        Some(OutPlan::Inside(private)) => {
            return public_refused(
                ctx,
                &format!("--out must be outside {private}: these files are meant to be published"),
            )
        }
        Some(OutPlan::Write(target)) => {
            if let Err(error) = std::fs::create_dir_all(&target) {
                return public_refused(
                    ctx,
                    &format!(
                        "--out {} is not a folder careerbrew can write ({})",
                        out.unwrap_or_default(),
                        os_error_name(&error)
                    ),
                );
            }
            let mut summary = payload["summary"].clone();
            let row = summary.as_object_mut()?;
            row.insert("generated_at".to_owned(), payload["generated_at"].clone());
            row.insert("window_days".to_owned(), payload["window_days"].clone());
            let summary_rows = format!("[\n {}\n]", nested(&careerbrew_reports::pretty(&summary)));
            let files = [
                ("summary", summary_rows),
                ("steps", careerbrew_reports::pretty(&payload["steps"])),
                ("daily", careerbrew_reports::pretty(&payload["daily"])),
            ];
            for (name, rows) in files {
                let path = target.join(format!("{name}.json"));
                let text = format!("{{\n \"rows\": {}\n}}\n", nested(&rows));
                let result = match std::fs::remove_file(&path) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                    _ => std::fs::write(&path, text),
                };
                if let Err(error) = result {
                    return public_refused(
                        ctx,
                        &format!(
                            "could not write {name}.json in --out ({})",
                            os_error_name(&error)
                        ),
                    );
                }
                written.push(path);
            }
        }
    }
    let mut text = String::new();
    if json {
        text.push_str(&careerbrew_reports::pretty(&payload));
        text.push('\n');
    } else {
        let s = &payload["summary"];
        text.push_str(&format!(
            "repo health, last {} days: {} gate run(s), {}/{} step run(s) failed, e2e {}/{}, checkup {}, {} open PR(s)\n",
            payload["window_days"],
            s["gate_runs"],
            s["failed_steps"],
            s["step_runs"],
            s["e2e_passed"],
            s["e2e_total"],
            s["checkup_status"].as_str()?,
            s["open_prs"]
        ));
        for row in payload["steps"].as_array()?.iter().take(5) {
            text.push_str(&format!(
                "  {:<14} {}/{} failed, {} s on average\n",
                row["step"].as_str()?,
                row["failed"],
                row["runs"],
                row["seconds_avg"]
            ));
        }
    }
    for path in written {
        text.push_str(&format!("wrote {}\n", path.to_str()?));
    }
    ok(printable(text)?, 0)
}

// ---------- `data set` (`machine.set_profile_fields`) ----------

const PROFILE_FAIL: &str =
    "profile update failed; check the profile and encryption key, then use the editor if needed\n";

fn data_set_fail(ctx: &Ctx) -> Option<Outcome> {
    emit(&ctx.root, "data.set", "fail", None);
    ok(PROFILE_FAIL.to_owned(), 1)
}

/// How PyYAML resolves a scalar this reader accepts.
#[derive(PartialEq)]
enum Tag {
    Str,
    Null,
    Other,
}

/// One top-level profile entry: its key and, for a one-line scalar, the
/// scalar's span (start, end) and tag; None for a block sequence.
struct ProfileEntry {
    key: String,
    scalar: Option<(usize, usize, Tag)>,
}

const YAML_BOOLS: &[&str] = &[
    "yes", "Yes", "YES", "no", "No", "NO", "true", "True", "TRUE", "false", "False", "FALSE", "on",
    "On", "ON", "off", "Off", "OFF",
];

/// The length and tag of the scalar at the start of `v`, when the rest of
/// the line after it is only spaces and a comment; None for any form this
/// reader does not prove PyYAML reads the same way.
fn yaml_scalar(v: &str) -> Option<(usize, Tag)> {
    let (len, tag) = match v.as_bytes().first()? {
        b'"' => {
            let close = v[1..].find('"')? + 1;
            if v[1..close].contains('\\') {
                return None;
            }
            (close + 1, Tag::Str)
        }
        b'\'' => {
            let close = v[1..].find('\'')? + 1;
            if v.as_bytes().get(close + 1) == Some(&b'\'') {
                return None;
            }
            (close + 1, Tag::Str)
        }
        first if first.is_ascii_alphanumeric() => {
            let run = v
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || b" ._/@+(),'-".contains(b))
                .count();
            let value = v[..run].trim_end_matches(' ');
            let tag = if first.is_ascii_digit() {
                // A digit-led plain scalar may resolve to an int, a float or
                // a timestamp; only a plain decimal int is certain.
                if !value.bytes().all(|b| b.is_ascii_digit())
                    || (value.len() > 1 && value.starts_with('0'))
                {
                    return None;
                }
                Tag::Other
            } else if YAML_BOOLS.contains(&value) {
                Tag::Other
            } else if ["null", "Null", "NULL"].contains(&value) {
                Tag::Null
            } else {
                Tag::Str
            };
            (value.len(), tag)
        }
        _ => return None,
    };
    let rest = &v[len..];
    let after = rest.trim_start_matches(' ');
    let comment_ok = after.is_empty() || (after.starts_with('#') && after.len() < rest.len());
    comment_ok.then_some((len, tag))
}

/// The top-level entries of a profile in the subset of YAML this reader
/// knows PyYAML composes the same way: printable ASCII lines, `key: scalar`
/// entries at column 0 with lowercase keys, one-line scalars, comments, and
/// block sequences of scalars under an empty value. None for anything else.
fn profile_entries(text: &str) -> Option<Vec<ProfileEntry>> {
    if !text
        .bytes()
        .all(|b| b == b'\n' || (0x20..0x7f).contains(&b))
    {
        return None;
    }
    let mut entries: Vec<ProfileEntry> = Vec::new();
    // True while the last entry's value is empty, so a sequence may follow;
    // the sequence's indent once one item is seen.
    let mut open_empty = false;
    let mut seq_indent: Option<usize> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let body = line.strip_suffix('\n').unwrap_or(line);
        let rest = body.trim_start_matches(' ');
        let indent = body.len() - rest.len();
        if rest.is_empty() || rest.starts_with('#') {
            continue;
        }
        if let Some(item) = rest.strip_prefix("- ") {
            if !open_empty || seq_indent.is_some_and(|at| at != indent) {
                return None;
            }
            yaml_scalar(item.trim_start_matches(' '))?;
            seq_indent = Some(indent);
            entries.last_mut()?.scalar = None;
            continue;
        }
        if indent > 0 {
            return None;
        }
        let key_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
            .count();
        let key = &rest[..key_len];
        if key.is_empty()
            || key.as_bytes()[0].is_ascii_digit()
            || YAML_BOOLS.contains(&key)
            || key == "null"
            || entries.iter().any(|entry| entry.key == key)
        {
            return None;
        }
        let after = rest[key_len..].strip_prefix(':')?;
        let colon_end = start + key_len + 1;
        let value = after.trim_start_matches(' ');
        open_empty = false;
        seq_indent = None;
        let scalar = if value.is_empty() || (value.starts_with('#') && value.len() < after.len()) {
            open_empty = true;
            (colon_end, colon_end, Tag::Null)
        } else if after.starts_with(' ') {
            let at = colon_end + (after.len() - value.len());
            let (len, tag) = yaml_scalar(value)?;
            (at, at + len, tag)
        } else {
            return None;
        };
        entries.push(ProfileEntry {
            key: key.to_owned(),
            scalar: Some(scalar),
        });
    }
    (!entries.is_empty()).then_some(entries)
}

/// `json.dumps(value, ensure_ascii=False)` for printable ASCII.
fn json_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// The profile text `set_profile_fields` writes, or None when this reader
/// cannot prove Python would write it (or would fail).
fn profile_update(raw: &[u8], fields: &[(&str, &str)]) -> Option<String> {
    if raw.starts_with(careerbrew_vault::MAGIC) {
        return None;
    }
    let mut text = std::str::from_utf8(raw).ok()?.to_owned();
    let entries = profile_entries(&text)?;
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for (name, value) in fields {
        match entries.iter().find(|entry| entry.key == *name) {
            None => missing.push((*name, *value)),
            Some(entry) => {
                let (start, end, tag) = entry.scalar.as_ref()?;
                if *tag == Tag::Other {
                    return None;
                }
                found.push((*start, *end, *value));
            }
        }
    }
    found.sort_by_key(|row| std::cmp::Reverse(row.0));
    for (start, end, value) in found {
        let separator = if start == end { " " } else { "" };
        text = format!(
            "{}{separator}{}{}",
            &text[..start],
            json_quote(value),
            &text[end..]
        );
    }
    if !missing.is_empty() {
        let mut grown = text.trim_end_matches('\n').to_owned();
        grown.push('\n');
        for (name, value) in missing {
            grown.push_str(&format!("{name}: {}\n", json_quote(value)));
        }
        text = grown;
    }
    Some(text)
}

fn data_set(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    use std::os::unix::fs::OpenOptionsExt;
    if args.text("file")? != "profile.yaml" {
        emit(&ctx.root, "data.set", "fail", None);
        return ok("data set supports only profile.yaml\n".to_owned(), 1);
    }
    // `sorted(fields)`: employer before name.
    let fields: Vec<(&str, &str)> = ["employer", "name"]
        .iter()
        .filter_map(|name| args.text(name).map(|value| (*name, value)))
        .collect();
    if fields.is_empty() {
        return data_set_fail(ctx);
    }
    for (_, value) in &fields {
        if !value.is_ascii() {
            return None;
        }
        // `not value.strip()`, the length and the control-character checks:
        // in ASCII, a value of only spaces and controls is empty once stripped.
        if value.bytes().all(|b| b <= b' ')
            || value.len() > 256
            || value.bytes().any(|b| b < 0x20 || b == 0x7f)
        {
            return data_set_fail(ctx);
        }
    }
    let context = vault_context(ctx)?;
    if context.store().encryption_enabled() {
        return None;
    }
    let root = ctx.root.canonicalize().ok()?;
    let lock_path = root.join(".profile-set.lock");
    match std::fs::symlink_metadata(&lock_path) {
        Ok(meta) if !meta.is_file() => return None,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let target = root.join("profile.yaml");
    let read = || -> Option<Option<Vec<u8>>> {
        match std::fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(None),
            Err(_) => None,
            Ok(meta) if !meta.is_file() => None,
            Ok(_) => std::fs::read(&target).ok().map(Some),
        }
    };
    if let Some(raw) = read()? {
        profile_update(&raw, &fields)?;
    }
    // Every refusal of a known profile is above; from here the run makes
    // Python's writes.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .open(&lock_path)
        .ok()?;
    lock.lock().ok()?;
    // Read again under the lock, as Python reads. A profile changed in
    // between is decided again; a form this reader cannot prove then goes
    // to Python, which finds the same empty lock file it would create.
    let Some(raw) = read()? else {
        return data_set_fail(ctx);
    };
    let text = profile_update(&raw, &fields)?;
    if context.store().write_text(&target, &text).is_err() {
        return data_set_fail(ctx);
    }
    drop(lock);
    emit(&ctx.root, "data.set", "ok", Some(fields.len()));
    let names: Vec<&str> = fields.iter().map(|(name, _)| *name).collect();
    ok(
        format!("saved profile.yaml fields: {}\n", names.join(", ")),
        0,
    )
}

// ---------- the outcome review (`review.py`, `runs.review`) ----------
//
// Served only in a data folder that holds no outcome data: then every
// metric `review.measure` computes is None, so every goal is `no-data`, and
// the run is a fixed function of `config/goals.yaml`, the clock and the
// `run_pages` setting. Anything else (any other file, `--html`, a goal
// value this module cannot print as Python's `str`, run pages already
// written) goes to Python.

/// The keys of `review.measure()`, which `review.jsonl` stores (sorted).
const MEASURED: &[&str] = &[
    "want_agreement",
    "readiness",
    "retention",
    "practice_pass_rate",
    "promo_coverage",
    "gap_closure",
    "estimate_ratio",
    "evidence_freshness_days",
    "loops_sourced",
    "prep_score",
    "eligible_postings",
    "interview_followup",
    "plugin_setup_minutes",
    "watcher_cpu_ms",
    "suggestion_follow_rate",
    "plan_done_rate",
];

/// The folders `review.measure` creates through `paths.sub`.
const MEASURE_DIRS: &[&str] = &["jobs", "tutor", "work", "prep", "day"];

/// The only files a served review may find: none holds outcome data.
const REVIEW_FILES: &[&str] = &[
    ".plaintext",
    "settings.yaml",
    "review.jsonl",
    ".review.jsonl.lock",
    "logs/events.jsonl",
];

/// The only folders a served review may find (each may be empty).
const REVIEW_DIRS: &[&str] = &[
    "logs",
    "jobs",
    "tutor",
    "work",
    "prep",
    "day",
    "out",
    "out/reports",
];

/// True when the data folder holds only `REVIEW_FILES` and `REVIEW_DIRS`
/// (no symlink, no other entry), and the `.plaintext` marker.
fn fresh_review_home(root: &Path) -> bool {
    fn walk(root: &Path, dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return false;
            };
            let path = entry.path();
            let Some(rel) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
                return false;
            };
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return false;
            };
            if meta.is_dir() {
                if !REVIEW_DIRS.contains(&rel) || !walk(root, &path) {
                    return false;
                }
            } else if !(meta.is_file() && REVIEW_FILES.contains(&rel)) {
                return false;
            }
        }
        true
    }
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return false;
    };
    meta.is_dir()
        && std::fs::symlink_metadata(root.join(".plaintext")).is_ok_and(|m| m.is_file())
        && walk(root, root)
}

fn canon_get<'a>(map: &'a Canon, key: &str) -> Option<&'a Canon> {
    let Canon::Map(pairs) = map else {
        return None;
    };
    pairs
        .iter()
        .find(|(k, _)| matches!(k, Canon::Str(name) if name == key))
        .map(|(_, v)| v)
}

/// `settings.yaml` as `config_io.read_settings` reads it. Served only with
/// the two settings a fresh review reads (`run_pages`, `timezone`); returns
/// the `run_pages` value (default on).
fn review_settings(store: &careerbrew_vault::store::Store, root: &Path) -> Option<bool> {
    let settings = match store
        .read_yaml(&root.join("settings.yaml"), Canon::Map(Vec::new()), false)
        .ok()?
    {
        Canon::Null => Canon::Map(Vec::new()),
        value @ Canon::Map(_) => value,
        _ => return None,
    };
    let Canon::Map(pairs) = &settings else {
        return None;
    };
    let mut pages = true;
    for (key, value) in pairs {
        match (key, value) {
            (Canon::Str(k), Canon::Bool(on)) if k == "run_pages" => pages = *on,
            (Canon::Str(k), Canon::Null) if k == "timezone" => {}
            (Canon::Str(k), Canon::Str(name)) if k == "timezone" => {
                // Python raises on a zone it cannot load; the crates fall back to UTC.
                if !name.is_empty() && careerbrew_feed::local_zone().name() != name {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(pages)
}

/// True for a month day that exists.
fn real_day(year: u32, month: u32, day: u32) -> bool {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=last).contains(&day)
}

/// `CAREERBREW_NOW` unset or empty (the wall clock), or
/// `YYYY-MM-DDTHH:MM:SS` with no zone, `Z`, or `+HH:MM`/`-HH:MM`: forms
/// Python's `fromisoformat` and `careerbrew_feed::now` read as one instant.
fn review_clock_ok() -> bool {
    let value = match std::env::var("CAREERBREW_NOW") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => return true,
        Err(_) => return false,
    };
    if value.is_empty() {
        return true;
    }
    let b = value.as_bytes();
    if b.len() < 19 || !b.is_ascii() {
        return false;
    }
    let num = |from: usize, to: usize| -> Option<u32> {
        let part = &value[from..to];
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let shape = b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':';
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    ) else {
        return false;
    };
    let zone = &value[19..];
    let zone_ok = match zone.len() {
        0 => true,
        1 => zone == "Z",
        6 => {
            let z = zone.as_bytes();
            (z[0] == b'+' || z[0] == b'-')
                && z[3] == b':'
                && num(20, 22).is_some_and(|h| h < 24)
                && num(23, 25).is_some_and(|m| m < 60)
        }
        _ => false,
    };
    shape
        && zone_ok
        && year >= 1
        && real_day(year, month, day)
        && hour < 24
        && minute < 60
        && second < 60
}

/// Python's `repr` of a float, or None where Rust and Python spell it
/// differently (exponent forms, inf, nan).
fn py_float(value: f64) -> Option<String> {
    if !value.is_finite() || (value != 0.0 && !(1e-4..1e16).contains(&value.abs())) {
        return None;
    }
    let text = format!("{value:?}");
    (!text.contains('e')).then_some(text)
}

/// Python's `str` of a goal bound (`success`, `pivot`).
fn bound_text(value: Option<&Canon>) -> Option<String> {
    match value {
        None | Some(Canon::Null) => Some("None".into()),
        Some(Canon::Int(number)) => {
            let text = number.to_string();
            (text.len() < 4000).then_some(text)
        }
        Some(Canon::Float(number)) => py_float(*number),
        _ => None,
    }
}

struct Goal {
    name: String,
    success: String,
    pivot: String,
    higher_is_better: bool,
}

/// `review.goals()` with no `<data root>/goals.yaml`: the metrics of
/// `config/goals.yaml` in file order.
fn review_goals(repo: &Path) -> Option<Vec<Goal>> {
    let text = std::fs::read_to_string(repo.join("config").join("goals.yaml")).ok()?;
    let doc = careerbrew_vault::json_yaml::yaml_read(&text).ok()?;
    let metrics = match &doc {
        Canon::Null => return Some(Vec::new()),
        Canon::Map(_) => match canon_get(&doc, "metrics") {
            None => return Some(Vec::new()),
            Some(value @ Canon::Map(_)) => value,
            Some(_) => return None,
        },
        _ => return None,
    };
    let Canon::Map(pairs) = metrics else {
        return None;
    };
    let mut out = Vec::new();
    for (key, goal) in pairs {
        let Canon::Str(name) = key else {
            return None;
        };
        let simple = !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_');
        if !simple || !matches!(goal, Canon::Map(_)) {
            return None;
        }
        let higher_is_better = match canon_get(goal, "higher_is_better") {
            None => true,
            Some(Canon::Bool(on)) => *on,
            Some(_) => return None,
        };
        out.push(Goal {
            name: name.clone(),
            success: bound_text(canon_get(goal, "success"))?,
            pivot: bound_text(canon_get(goal, "pivot"))?,
            higher_is_better,
        });
    }
    Some(out)
}

/// `html.escape(text)` (quote=True).
fn html_esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(c),
        }
    }
    out
}

/// One `report.Item` (a fresh review has no `detail` and no `why`).
struct PageItem {
    id: String,
    label: String,
    evidence: String,
    command: String,
}

fn page_item(id: &str, label: String, evidence: String, command: &str) -> PageItem {
    PageItem {
        id: id.into(),
        label,
        evidence,
        command: command.into(),
    }
}

fn item_html(item: &PageItem, kind: &str) -> String {
    let mut out = String::from("<li>");
    out.push_str(&html_esc(&item.label));
    if !item.evidence.is_empty() {
        out.push_str(&format!(
            "<div class=\"muted small\">{}</div>",
            html_esc(&item.evidence)
        ));
    }
    if !item.command.is_empty() {
        let label = if kind == "next" { "Run" } else { "Ran" };
        out.push_str(&format!(
            "<div class=\"fix\"><b>{label}:</b> <code>{}</code></div>",
            html_esc(&item.command)
        ));
    }
    out.push_str("</li>");
    out
}

fn section_html(title: &str, kind: &str, items: &[PageItem], empty: &str) -> String {
    let tag = if kind == "next" { "ol" } else { "ul" };
    let body = if items.is_empty() {
        format!("<p class=\"empty\">{}</p>", html_esc(empty))
    } else {
        let inner: String = items.iter().map(|it| item_html(it, kind)).collect();
        format!("<{tag} class=\"items\">{inner}</{tag}>")
    };
    format!(
        "<section class=\"{kind}\" aria-labelledby=\"h-{kind}\"><h2 id=\"h-{kind}\">{} <span class=\"count\">({})</span></h2>{body}</section>",
        html_esc(title),
        items.len()
    )
}

const REVIEW_TITLE: &str = "Outcome review: KEEP, WATCH, PIVOT";

/// The parts of a fresh review run that its page and summary show.
struct ReviewRun<'a> {
    at: &'a str,
    id: &'a str,
    headline: &'a str,
    counts: &'a [(&'a str, usize)],
    done: &'a [PageItem],
    next: &'a [PageItem],
}

/// `report.render` for a fresh review run: no blockers, no logs.
fn review_page(run: &ReviewRun, when: &str) -> String {
    let kpis: String = run
        .counts
        .iter()
        .map(|(k, v)| format!("<li><b>{v}</b>{}</li>", html_esc(&k.replace('_', " "))))
        .collect();
    let mut out = String::new();
    out.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    out.push_str(&format!(
        "<title>{}</title><style>{PAGE_CSS}</style></head><body><main>",
        html_esc(REVIEW_TITLE)
    ));
    out.push_str(&format!(
        "<h1>{}</h1><p class=\"muted small\">{} \u{b7} run <code>{}</code></p>",
        html_esc(REVIEW_TITLE),
        html_esc(when),
        html_esc(run.id)
    ));
    out.push_str("<p class=\"status status-ok\" role=\"status\"><strong>Verdict:</strong> OK: nothing blocks</p>");
    if !run.headline.is_empty() {
        out.push_str(&format!(
            "<p class=\"headline\">{}</p>",
            html_esc(run.headline)
        ));
    }
    out.push_str(&format!(
        "<p class=\"small jump\"><a href=\"#h-blockers\">Blockers (0)</a> \u{b7} <a href=\"#h-next\">Next ({})</a> \u{b7} <a href=\"#h-done\">Done ({})</a></p>",
        run.next.len(),
        run.done.len()
    ));
    if !kpis.is_empty() {
        out.push_str(&format!("<ul class=\"kpis\">{kpis}</ul>"));
    }
    out.push_str(&section_html("Blockers", "blockers", &[], "No blockers."));
    out.push_str(&section_html(
        "Next",
        "next",
        run.next,
        "Nothing to do next.",
    ));
    out.push_str(&section_html(
        "Done",
        "done",
        run.done,
        "Nothing finished in this run.",
    ));
    out.push_str("<section class=\"commands\" aria-labelledby=\"h-commands\"><h2 id=\"h-commands\">Commands run</h2><ul class=\"items\"><li><code>careerbrew review</code></li></ul></section>");
    out.push_str(&format!(
        "<footer><p>Holds your own data. It stays on this machine: open it with <code>careerbrew open --report latest</code>; do not publish it.</p><p>Agents: <code>careerbrew report latest --json</code> gives the counts, ids, statuses, and commands, with no page text. careerbrew {}.</p></footer>",
        html_esc(crate::events::version())
    ));
    out.push_str("</main></body></html>");
    out
}

/// `report._clean` for careerbrew's own ASCII words (no redaction input).
fn clean_text(text: &str) -> Json {
    let joined = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    Json::Str(joined.chars().take(300).collect())
}

fn json_int(value: usize) -> Json {
    Json::Number(value.to_string())
}

/// `{f: _clean(item.f) for f in fields}` with the keys sorted.
fn item_json(item: &PageItem, kind: &str) -> Json {
    let mut pairs = vec![("command".to_owned(), clean_text(&item.command))];
    if kind == "done" {
        pairs.push(("evidence".to_owned(), clean_text(&item.evidence)));
    }
    pairs.push(("id".to_owned(), clean_text(&item.id)));
    pairs.push(("label".to_owned(), clean_text(&item.label)));
    Json::Object(pairs)
}

/// `json.dumps({**summary(r, page), "seq": 1}, indent=1, sort_keys=True)`.
fn review_summary(run: &ReviewRun, page: &str) -> String {
    let mut counts: Vec<(String, Json)> = run
        .counts
        .iter()
        .map(|(k, v)| ((*k).to_owned(), json_int(*v)))
        .collect();
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    let summary = Json::Object(vec![
        ("at".into(), Json::Str(run.at.into())),
        ("blockers".into(), Json::Array(Vec::new())),
        (
            "commands".into(),
            Json::Array(vec![clean_text("careerbrew review")]),
        ),
        ("counts".into(), Json::Object(counts)),
        (
            "done".into(),
            Json::Array(run.done.iter().map(|it| item_json(it, "done")).collect()),
        ),
        ("headline".into(), clean_text(run.headline)),
        ("id".into(), Json::Str(run.id.into())),
        (
            "next".into(),
            Json::Array(run.next.iter().map(|it| item_json(it, "next")).collect()),
        ),
        ("page".into(), Json::Str(page.into())),
        ("personal".into(), Json::Bool(true)),
        ("run".into(), Json::Str("review".into())),
        ("seq".into(), json_int(1)),
        ("status".into(), Json::Str("ok".into())),
        ("title".into(), clean_text(REVIEW_TITLE)),
    ]);
    crate::render::py_dumps(&summary)
}

/// `events.emit(event, **fields)`; a failed log write is ignored.
fn emit_fields(root: &Path, event: &str, fields: &[(&str, Canon)]) {
    let map = Canon::Map(
        fields
            .iter()
            .map(|(k, v)| (Canon::Str((*k).into()), v.clone()))
            .collect(),
    );
    let _ = careerbrew_vault::events::emit(
        root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        event,
        &map,
        &NowClock,
    );
}

fn canon_int(value: usize) -> Canon {
    Canon::Int((value as u64).into())
}

/// The `review.jsonl` row `review.run` appends (`sort_keys=True`).
fn review_row(at: &str, goals: &[Goal]) -> String {
    let mut row = String::from("{\"at\": ");
    crate::render::py_string(at, &mut row);
    row.push_str(", \"status\": {");
    let mut names: Vec<&str> = goals.iter().map(|g| g.name.as_str()).collect();
    names.sort_unstable();
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            row.push_str(", ");
        }
        crate::render::py_string(name, &mut row);
        row.push_str(": \"no-data\"");
    }
    row.push_str("}, \"values\": {");
    let mut measured = MEASURED.to_vec();
    measured.sort_unstable();
    for (i, name) in measured.iter().enumerate() {
        if i > 0 {
            row.push_str(", ");
        }
        crate::render::py_string(name, &mut row);
        row.push_str(": null");
    }
    row.push_str("}}\n");
    row
}

/// `store.append_jsonl` on a plaintext file: append under the file's lock.
fn append_locked(root: &Path, name: &str, row: &str) -> Option<()> {
    use std::io::Write;
    let lock = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join(format!(".{name}.lock")))
        .ok()?;
    lock.lock().ok()?;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join(name))
        .ok()?;
    file.write_all(row.as_bytes()).ok()
}

/// `careerbrew review` (`cli.cmd_review`) in a data folder with no outcome
/// data: every metric is `no-data`.
fn review(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if args.flag("html") || !fresh_review_home(&ctx.root) || !review_clock_ok() {
        return None;
    }
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    let store = context.store();
    if store.encryption_enabled() {
        return None;
    }
    let pages = review_settings(&store, &ctx.root)?;
    let goals = review_goals(&repo_root()?)?;
    let root_text = shown_root(ctx)?;
    let reports = reports_folder(ctx);
    if pages && std::fs::read_dir(&reports).is_ok_and(|mut d| d.next().is_some()) {
        return None; // earlier pages: their seq, ids and pruning stay with Python
    }

    let now = careerbrew_feed::now();
    let at = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
    let n = goals.len();
    let headline = format!("{n} metric(s): 0 KEEP, 0 WATCH, 0 PIVOT, {n} no data");
    let mut done = vec![page_item(
        "measure",
        format!("Measured {n} outcome metric(s)"),
        format!("0 KEEP, 0 WATCH, 0 PIVOT, {n} no data"),
        "careerbrew review",
    )];
    for goal in &goals {
        let (keep, pivot) = if goal.higher_is_better {
            (">=", "<")
        } else {
            ("<=", ">")
        };
        done.push(page_item(
            &goal.name,
            format!("{}: no data yet", goal.name),
            format!("keep {keep} {}, pivot {pivot} {}", goal.success, goal.pivot),
            "",
        ));
    }
    let next = vec![page_item(
        "again",
        "Measure again next week".into(),
        String::new(),
        "careerbrew review",
    )];
    let counts: [(&str, usize); 5] = [
        ("metrics", n),
        ("keep", 0),
        ("watch", 0),
        ("pivot", 0),
        ("no_data", n),
    ];
    let digits: String = at[..19].chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 14 {
        return None;
    }
    let id = format!("review-{}-{}", &digits[..8], &digits[8..14]);
    let page_path = format!("{root_text}/out/reports/{id}.html");
    let stdout = if pages {
        format!(
            "page: {page_path}\nreview: ok: {headline}; {} done, 0 blocker(s), 1 next action(s); next: careerbrew review (agents: careerbrew report latest --json)\n",
            done.len()
        )
    } else {
        goals
            .iter()
            .map(|g| format!("{:<8} {:<24} {:<8} \n", "no-data", g.name, "None"))
            .collect()
    };
    let stdout = printable(stdout)?;
    let run = ReviewRun {
        at: &at,
        id: &id,
        headline: &headline,
        counts: &counts,
        done: &done,
        next: &next,
    };

    // Every refusal is above; from here the run makes Python's writes.
    let _ = std::fs::set_permissions(
        &ctx.root,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    );
    for dir in MEASURE_DIRS {
        std::fs::create_dir_all(ctx.root.join(dir)).ok()?;
    }
    append_locked(&ctx.root, "review.jsonl", &review_row(&at, &goals))?;
    emit_fields(
        &ctx.root,
        "review.run",
        &[
            ("status", Canon::Str("ok".into())),
            ("pivots", canon_int(0)),
        ],
    );
    if pages {
        std::fs::create_dir_all(&reports).ok()?;
        let when = format!(
            "{} {}, {}",
            now.format("%a %b"),
            now.format("%-d"),
            now.format("%H:%M")
        );
        let html = review_page(&run, &when);
        let data = review_summary(&run, &page_path);
        std::fs::write(reports.join(format!("{id}.html")), &html).ok()?;
        std::fs::write(reports.join(format!("{id}.json")), &data).ok()?;
        std::fs::write(reports.join("latest.html"), &html).ok()?;
        std::fs::write(reports.join("latest.json"), &data).ok()?;
        emit_fields(
            &ctx.root,
            "report.write",
            &[
                ("command", Canon::Str("review".into())),
                ("status", Canon::Str("ok".into())),
                ("done", canon_int(done.len())),
                ("blockers", canon_int(0)),
                ("next", canon_int(1)),
            ],
        );
    }
    ok(stdout, 0)
}

/// `report.CSS`, byte for byte.
const PAGE_CSS: &str = r##"
:root{color-scheme:light;--bg:#fcfcfb;--text:#0b0b0b;--muted:#52514e;--line:#e4e3df;--code-bg:#f2f1ee;
--good:#0a7a0a;--bad:#c62f2e;--accent:#2a78d6;--good-bg:#e9f5e9;--bad-bg:#fbeaea}
@media (prefers-color-scheme: dark){:root:not([data-theme="light"]){color-scheme:dark;--bg:#1a1a19;--text:#f0efec;
--muted:#c3c2b7;--line:#3a3936;--code-bg:#2c2b29;--good:#5cc05c;--bad:#ee7b7a;--accent:#5a9fec;--good-bg:#1f2e1f;--bad-bg:#3a2424}}
:root[data-theme="dark"]{color-scheme:dark;--bg:#1a1a19;--text:#f0efec;--muted:#c3c2b7;--line:#3a3936;--code-bg:#2c2b29;
--good:#5cc05c;--bad:#ee7b7a;--accent:#5a9fec;--good-bg:#1f2e1f;--bad-bg:#3a2424}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:15px/1.5 -apple-system,system-ui,"Segoe UI",sans-serif}
main{max-width:860px;margin:0 auto;padding:24px 16px 48px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:28px 0 8px}
.muted{color:var(--muted)}.small{font-size:13px}
.status{display:inline-block;font-weight:600;padding:2px 10px;border-radius:999px;margin:8px 0}
.status-ok{background:var(--good-bg);color:var(--good)}.status-blocked{background:var(--bad-bg);color:var(--bad)}
.kpis{display:flex;flex-wrap:wrap;gap:8px 24px;margin:12px 0 4px;padding:0;list-style:none}
.kpis b{display:block;font-size:22px;line-height:1.2}
.items{padding-left:22px;margin:0}
.items>li{margin:0 0 12px;padding-left:4px;overflow-wrap:anywhere;font-weight:600}
.items>li>div{font-weight:400}
.commands .items>li{font-weight:400}
.count{font-size:13px;color:var(--muted);font-weight:400}
code,pre{font:13px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--code-bg);border-radius:6px}
code{padding:1px 6px;overflow-wrap:anywhere;word-break:break-word}
pre{padding:10px 12px;white-space:pre-wrap;overflow-wrap:anywhere;margin:6px 0 0}
.fix{margin-top:2px}.fix b,a{color:var(--accent)}
.blockers .items>li{color:var(--bad)}.blockers .items>li>div{color:var(--text)}
.empty{color:var(--muted);margin:0}
details{margin:6px 0;border-top:1px solid var(--line);padding-top:6px}summary{cursor:pointer}
footer{margin-top:32px;padding-top:12px;border-top:1px solid var(--line);color:var(--muted);font-size:13px}
"##;
