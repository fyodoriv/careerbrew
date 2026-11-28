//! Native CLI handlers owned by `rust-06` (see `steps/mod.rs`).
//!
//! `sync export <file>` writes the encrypted personal → corporate settings
//! bundle, as Python's `sync.export` does. Only the success path is served:
//! every case where Python prints `error:` (a corporate machine, a bundle
//! inside the data folder, no pairing key) refuses before any write, and
//! Python then prints its own error.
//!
//! `sync import <file>` reads such a bundle on a corporate machine and
//! writes the accepted ladder and target level into `settings.yaml`, as
//! Python's `sync.import_bundle` does. It is served only when every value
//! `settings.yaml` holds, before and after, is a plain YAML word (see
//! `plain_word`), so the text equals PyYAML's `safe_dump`; the ladders are
//! the repo's own (no plugin, no `ladders/` folder in the data root). Every
//! error case, and any input Python could read otherwise, refuses before
//! the write.
//!
//! `backup restore <file> <dest>` extracts a backup into a folder on a
//! personal machine, as Python's `backup.restore` does. It is served only
//! when both paths are absolute and plain, the folder is not the data root
//! or above it, and a trial extraction into a private scratch folder shows
//! that every member lands on a missing path or on an owner-writable
//! regular file. A corporate machine, every error Python would print, and
//! any doubt refuse before the first write to the destination. Python's
//! `tarfile` also sets each file's modification time from the archive; the
//! vault reader does not expose it, so restored files keep the time of the
//! write (file bytes, names and modes match).
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust06.rs`.
pub const COMMANDS: &[&str] = &["sync.export", "sync.import", "backup.restore"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    match name {
        "sync.export" => export(args.text("file")?, ctx),
        "sync.import" => import(args.text("file")?, ctx),
        "backup.restore" => restore(args.text("file")?, args.text("dest")?, ctx),
        _ => None,
    }
}

/// `sync.SETTINGS_KEYS`, in Python's order.
const SETTINGS_KEYS: &[&str] = &["ladder", "target_level"];

/// True when `pathlib.Path` prints `path` unchanged (a private copy of the
/// `routed` rule).
fn plain_path(path: &str) -> bool {
    !(path.contains("//")
        || path.contains("/./")
        || path.starts_with("./")
        || path.ends_with("/.")
        || (path.len() > 1 && path.ends_with('/'))
        || path == ".")
}

/// A set, non-empty `HOME`, and no `~user` key path: the vault context then
/// resolves the same key files Python does.
fn env_is_plain() -> bool {
    if std::env::var("HOME").map_or(true, |home| home.is_empty()) {
        return false;
    }
    for key in ["CAREERBREW_KEY_FILE", "CAREERBREW_PAIR_KEY_FILE"] {
        if let Ok(value) = std::env::var(key) {
            if value.starts_with('~') && !(value == "~" || value.starts_with("~/")) {
                return false;
            }
        }
    }
    true
}

/// The entries of a YAML mapping read through the store, or None when
/// Python could read it otherwise (not a mapping, a repeated key).
fn mapping(value: Canon) -> Option<Vec<(String, Canon)>> {
    let Canon::Map(items) = value else {
        return None;
    };
    let mut out: Vec<(String, Canon)> = Vec::new();
    for (key, value) in items {
        let Canon::Str(key) = key else {
            continue;
        };
        if out.iter().any(|(seen, _)| *seen == key) {
            return None;
        }
        out.push((key, value));
    }
    Some(out)
}

/// `machine.role() == "personal"`: no `role` key, or the text `personal`.
/// Python writes any other role into the bundle and the event row (or
/// refuses a corporate one), so those go to Python. None when Python could
/// fail reading the file.
fn personal(store: &careerbrew_vault::store::Store, root: &Path) -> Option<bool> {
    let info = store
        .read_yaml(&root.join("machine.yaml"), Canon::Map(Vec::new()), true)
        .ok()?;
    let info = mapping(info)?;
    Some(info.iter().all(|(key, value)| {
        key != "role" || matches!(value, Canon::Str(role) if role == "personal")
    }))
}

/// One settings value as Python's `json.dumps` writes it; None for a kind
/// whose YAML reading or JSON form could differ (floats, dates, lists).
fn json_value(value: &Canon) -> Option<String> {
    Some(match value {
        Canon::Null => "null".to_owned(),
        Canon::Bool(true) => "true".to_owned(),
        Canon::Bool(false) => "false".to_owned(),
        Canon::Int(number) => number.to_string(),
        Canon::Str(text) => {
            let mut out = String::new();
            crate::render::py_string(text, &mut out);
            out
        }
        _ => return None,
    })
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `calendar.now_utc().isoformat(timespec="seconds")` for a pinned
/// `CAREERBREW_NOW` of the form `YYYY-MM-DDTHH:MM:SS` with `Z` or
/// `+00:00`; None for any other form (Python may read it as local time).
fn pinned_at(value: &str) -> Option<String> {
    let body = value
        .strip_suffix("+00:00")
        .or_else(|| value.strip_suffix('Z'))?;
    let bytes = body.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    for (at, byte) in bytes.iter().enumerate() {
        let ok = match at {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        };
        if !ok {
            return None;
        }
    }
    let num = |from: usize, to: usize| body[from..to].parse::<u32>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    Some(format!("{body}+00:00"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Python's `Path(env).expanduser()` for the pairing key path, or None for
/// a form this port does not resolve.
fn pair_key_path(home: &str) -> Option<PathBuf> {
    match std::env::var("CAREERBREW_PAIR_KEY_FILE") {
        Ok(value) if !value.is_empty() => {
            let path = crate::events::expand_user(&value, Some(home.to_owned()))?;
            Some(PathBuf::from(path))
        }
        _ => Some(PathBuf::from(home).join(".config/careerbrew/pair.key")),
    }
}

/// The `sync.export` row Python's `events.emit` appends, keys sorted.
fn export_row() -> String {
    let mut out = String::from(
        "{\"event\": \"sync.export\", \"source_kind\": \"personal\", \"status\": \"ok\", \"ts\": ",
    );
    crate::render::py_string(&crate::events::stamp(now_secs()), &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(crate::events::version(), &mut out);
    out.push_str("}\n");
    out
}

fn export(file: &str, ctx: &Ctx) -> Option<Outcome> {
    // The output line goes through `privacy.redact`; with none of its
    // inputs present it reads no term and prints the line unchanged.
    if ctx.redact_inputs_present() || !env_is_plain() {
        return None;
    }
    // Python writes to the path as typed but checks it with `~` expanded;
    // a relative path depends on the working folder. Both go to Python.
    if !file.starts_with('/') || !plain_path(file) {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    if !root_text.starts_with('/') {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    // `sync.export`: the role, then the bundle path, then the settings,
    // then the clock, then the pairing key, as Python reads them.
    if !personal(&store, &ctx.root)? {
        return None;
    }
    let dest = Path::new(file);
    careerbrew_vault::keyfile::require_outside_data_root(dest, &ctx.root).ok()?;
    let settings = store
        .read_yaml(
            &ctx.root.join("settings.yaml"),
            Canon::Map(Vec::new()),
            true,
        )
        .ok()?;
    let settings = mapping(settings)?;
    let mut picked: Vec<String> = Vec::new();
    for key in SETTINGS_KEYS {
        if let Some((_, value)) = settings.iter().find(|(name, _)| name == key) {
            let mut item = String::new();
            crate::render::py_string(key, &mut item);
            item.push_str(": ");
            item.push_str(&json_value(value)?);
            picked.push(item);
        }
    }
    let at = match std::env::var("CAREERBREW_NOW") {
        Ok(value) if !value.is_empty() => pinned_at(&value)?,
        Ok(_) => return None,
        Err(_) => {
            // Unpinned, Python reads the `timezone` setting first, and an
            // unknown zone is an error there.
            if settings.iter().any(|(name, _)| name == "timezone") {
                return None;
            }
            let stamp = crate::events::stamp(now_secs());
            format!("{}+00:00", stamp.strip_suffix('Z')?)
        }
    };
    let pair = pair_key_path(&home)?;
    careerbrew_vault::keyfile::require_outside_data_root(&pair, &ctx.root).ok()?;
    let key = std::fs::read_to_string(&pair).ok()?;
    let key = key.trim();
    // Only the canonical form `Fernet.generate_key` writes: 43 URL-safe
    // characters and one `=`. Python's lenient decoder and the Rust one
    // read that form alike; any other key goes to Python.
    if key.len() != 44
        || !key.ends_with('=')
        || !key[..43]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let plaintext = format!(
        "{{\"from\": \"personal\", \"at\": \"{at}\", \"payload\": {{\"settings\": {{{}}}}}}}",
        picked.join(", ")
    );
    let sealed = careerbrew_vault::seal(key, plaintext.as_bytes()).ok()?;
    let token = sealed.strip_prefix(careerbrew_vault::MAGIC)?;
    // Nothing is written before this point: a failed write refuses, and
    // Python then prints its own error.
    std::fs::write(dest, token).ok()?;
    let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o600));
    if std::env::var("CAREERBREW_EVENTS").as_deref() != Ok("off") {
        let logs = ctx.root.join("logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            if let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("events.jsonl"))
            {
                let _ = log.write_all(export_row().as_bytes());
            }
        }
    }
    Some(Outcome {
        stdout: format!(
            "exported from personal machine: level=settings items={} -> {file}\n",
            picked.len()
        ),
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// True when PyYAML's `safe_dump` writes `text` as a plain scalar that
/// `safe_load` reads back as the same text: a letter, then letters,
/// digits, `_`, `.`, `/` or `-`, at most 64 characters, and not one of the
/// YAML 1.1 boolean or null words.
fn plain_word(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphabetic()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'))
        && !["yes", "no", "on", "off", "true", "false", "null"]
            .contains(&text.to_ascii_lowercase().as_str())
}

/// The entries of a mapping whose keys are all text and appear once, or
/// None.
fn strict_map(value: &Canon) -> Option<Vec<(&str, &Canon)>> {
    let Canon::Map(items) = value else {
        return None;
    };
    let mut out: Vec<(&str, &Canon)> = Vec::new();
    for (key, value) in items {
        let Canon::Str(key) = key else {
            return None;
        };
        if out.iter().any(|(seen, _)| *seen == key) {
            return None;
        }
        out.push((key, value));
    }
    Some(out)
}

fn field<'a>(items: &[(&str, &'a Canon)], name: &str) -> Option<&'a Canon> {
    items
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

/// The repo whose `knowledge/ladders` Python reads (`paths.REPO_ROOT`): the
/// console script passes it in `CAREERBREW_NATIVE_REPO`; a direct run uses
/// the checkout the binary was built from.
fn repo_root() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(_) => Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))),
    }
}

/// `promo.validate_ladder` for one ladder file: its id and levels, or None
/// where Python raises (or could read the file otherwise).
fn ladder_of(value: &Canon) -> Option<(String, Vec<String>)> {
    let items = strict_map(value)?;
    let Some(Canon::Str(id)) = field(&items, "id") else {
        return None;
    };
    let Some(Canon::List(levels)) = field(&items, "levels") else {
        return None;
    };
    let Some(Canon::List(competencies)) = field(&items, "competencies") else {
        return None;
    };
    if id.is_empty() || levels.is_empty() || competencies.is_empty() {
        return None;
    }
    let levels = levels
        .iter()
        .map(|level| match level {
            Canon::Str(level) => Some(level.clone()),
            _ => None,
        })
        .collect::<Option<Vec<String>>>()?;
    for competency in competencies {
        let comp = strict_map(competency)?;
        for key in ["id", "name", "weight", "signals", "expect"] {
            field(&comp, key)?;
        }
        let expect = strict_map(field(&comp, "expect")?)?;
        if levels.iter().any(|level| field(&expect, level).is_none()) {
            return None;
        }
        for key in ["descriptions", "examples"] {
            if field(&comp, key).is_some_and(|value| !matches!(value, Canon::Map(_))) {
                return None;
            }
        }
    }
    Some((id.clone(), levels))
}

/// `promo.ladders()` with no plugin and no user ladders: every file in the
/// repo's `knowledge/ladders`, in name order, the later id winning.
fn base_ladders(store: &careerbrew_vault::store::Store) -> Option<Vec<(String, Vec<String>)>> {
    let dir = repo_root()?.join("knowledge/ladders");
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    if !dir.is_dir() {
        return Some(out);
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()? {
        let path = entry.ok()?.path();
        path.file_name()?.to_str()?;
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    for path in files {
        let value = store.read_yaml(&path, Canon::Map(Vec::new()), true).ok()?;
        let (id, levels) = ladder_of(&value)?;
        match out.iter_mut().find(|(seen, _)| *seen == id) {
            Some(entry) => entry.1 = levels,
            None => out.push((id, levels)),
        }
    }
    Some(out)
}

/// `machine.role() == "corporate"`, or None when Python could fail or read
/// `machine.yaml` otherwise.
fn corporate(store: &careerbrew_vault::store::Store, root: &Path) -> Option<bool> {
    let info = store
        .read_yaml(&root.join("machine.yaml"), Canon::Map(Vec::new()), true)
        .ok()?;
    let items = mapping(info)?;
    Some(items.iter().any(|(key, value)| {
        key == "role" && matches!(value, Canon::Str(role) if role == "corporate")
    }))
}

/// The `sync.import` row Python's `events.emit` appends, keys sorted.
fn import_row(items: usize) -> String {
    let mut out =
        format!("{{\"event\": \"sync.import\", \"items\": {items}, \"status\": \"ok\", \"ts\": ");
    crate::render::py_string(&crate::events::stamp(now_secs()), &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(crate::events::version(), &mut out);
    out.push_str("}\n");
    out
}

fn import(file: &str, ctx: &Ctx) -> Option<Outcome> {
    // The output lines go through `privacy.redact`; with none of its inputs
    // present it reads no term (and no plugin ladder exists).
    if ctx.redact_inputs_present() || !env_is_plain() {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    if !root_text.starts_with('/') {
        return None;
    }
    let home = std::env::var("HOME").ok()?;
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    // `sync._fernet()`: the pairing key, outside the data folder, in the
    // form `Fernet.generate_key` writes.
    let pair = pair_key_path(&home)?;
    careerbrew_vault::keyfile::require_outside_data_root(&pair, &ctx.root).ok()?;
    let key = std::fs::read_to_string(&pair).ok()?;
    let key = key.trim();
    if key.len() != 44
        || !key.ends_with('=')
        || !key[..43]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let raw = std::fs::read(file).ok()?;
    let mut sealed = careerbrew_vault::MAGIC.to_vec();
    sealed.extend_from_slice(&raw);
    let plain = careerbrew_vault::open(Some(key), &sealed).ok()?;
    // `json.loads` reads bytes as UTF-8 only when the first two bytes are
    // not zero and there is no byte-order mark: an object starts with `{`.
    if plain.first() != Some(&b'{') || plain.get(1).is_none_or(|b| *b == 0) {
        return None;
    }
    let text = String::from_utf8(plain).ok()?;
    let bundle = careerbrew_vault::json_yaml::json_read(&text).ok()?;
    let bundle = strict_map(&bundle)?;
    let Some(Canon::Str(from)) = field(&bundle, "from") else {
        return None;
    };
    let payload = strict_map(field(&bundle, "payload")?)?;
    // Only the corporate side imports; a bundle from another corporate
    // machine, or any payload besides settings, is a Python error.
    if !corporate(&store, &ctx.root)? || from == "corporate" {
        return None;
    }
    if payload.iter().any(|(key, _)| *key != "settings") {
        return None;
    }
    let incoming = match field(&payload, "settings") {
        None | Some(Canon::Null) => Vec::new(),
        Some(value @ Canon::Map(_)) => strict_map(value)?,
        Some(_) => return None,
    };
    let text_of = |name: &str| match field(&incoming, name) {
        Some(Canon::Str(value)) if value.chars().count() <= 64 => Some(value.as_str()),
        _ => None,
    };
    if ctx.root.join("ladders").exists() {
        return None;
    }
    let ladders = base_ladders(&store)?;
    let levels_of = |id: &str| {
        ladders
            .iter()
            .find(|(seen, _)| seen == id)
            .map(|(_, levels)| levels)
    };
    let path = ctx.root.join("settings.yaml");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let settings = store.read_yaml(&path, Canon::Map(Vec::new()), true).ok()?;
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (key, value) in strict_map(&settings)? {
        let Canon::Str(value) = value else {
            return None;
        };
        if !plain_word(key) || !plain_word(value) {
            return None;
        }
        pairs.push((key.to_owned(), value.clone()));
    }
    let mut applied: Vec<(&str, String)> = Vec::new();
    if let Some(ladder) = text_of("ladder").filter(|id| levels_of(id).is_some()) {
        applied.push(("ladder", ladder.to_owned()));
    }
    let ladder_id = match applied.first() {
        Some((_, id)) => id.clone(),
        None => pairs
            .iter()
            .find(|(key, _)| key == "ladder")
            .map_or_else(|| "generic-swe".to_owned(), |(_, value)| value.clone()),
    };
    if let (Some(levels), Some(level)) = (levels_of(&ladder_id), text_of("target_level")) {
        if levels.iter().any(|known| known == level) {
            applied.push(("target_level", level.to_owned()));
        }
    }
    for (key, value) in &applied {
        if !plain_word(value) {
            return None;
        }
        match pairs.iter_mut().find(|(name, _)| name == key) {
            Some(pair) => pair.1 = value.clone(),
            None => pairs.push(((*key).to_owned(), value.clone())),
        }
    }
    let yaml = if pairs.is_empty() {
        "{}\n".to_owned()
    } else {
        pairs
            .iter()
            .map(|(key, value)| format!("{key}: {value}\n"))
            .collect()
    };
    let added = applied.len();
    let ignored = incoming.len() - added;
    // The first sealed write creates the key in Python; load (or create) it
    // first so a key error refuses before the file is written.
    if store.encryption_enabled() {
        careerbrew_vault::synthetic::key_for_data(&vault.root, &vault.key_file, true).ok()?;
    }
    store.write_text(&path, &yaml).ok()?;
    if std::env::var("CAREERBREW_EVENTS").as_deref() != Ok("off") {
        let logs = ctx.root.join("logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            if let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("events.jsonl"))
            {
                let _ = log.write_all(import_row(added).as_bytes());
            }
        }
    }
    let mut stdout = format!("imported bundle from {from} machine: {added} new record(s)\n");
    if ignored > 0 {
        stdout.push_str(&format!(
            "ignored {ignored} setting(s): only a known ladder and one of its levels are accepted on a corporate machine\n"
        ));
    }
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// True for an absolute path that `pathlib` prints unchanged and that holds
/// no `.` or `..` part, so Python's `resolve()` only follows links.
fn plain_absolute(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && plain_path(path)
        && !path.split('/').any(|part| part == "." || part == "..")
}

/// `Path.resolve()` (not strict) for a plain absolute path: the real path
/// of its longest existing part, then the missing parts as given. None on
/// any error other than a missing part.
fn resolve_plain(path: &Path) -> Option<PathBuf> {
    let mut base = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match std::fs::canonicalize(&base) {
            Ok(mut out) => {
                for part in rest.iter().rev() {
                    out.push(part);
                }
                return Some(out);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                rest.push(base.file_name()?.to_owned());
                base = base.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}

/// Every regular file under `dir` (relative to `base`) with its mode; None
/// on anything else (a link, a special file, a non-UTF-8 name).
fn tree_files(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, u32)>) -> Option<()> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        path.file_name()?.to_str()?;
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if meta.is_dir() {
            tree_files(base, &path, out)?;
        } else if meta.file_type().is_file() {
            out.push((
                path.strip_prefix(base).ok()?.to_path_buf(),
                meta.permissions().mode() & 0o7777,
            ));
        } else {
            return None;
        }
    }
    Some(())
}

/// The mode `tarfile`'s `data` filter gives a regular file.
fn data_filter_mode(mode: u32) -> u32 {
    let mut mode = mode & 0o755;
    if mode & 0o100 == 0 {
        mode &= !0o111;
    }
    mode | 0o600
}

/// A private scratch folder for the trial extraction, removed on drop.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch_dir() -> Option<Scratch> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "careerbrew-native-restore-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir(&path).ok()?;
    let scratch = Scratch(path);
    std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o700)).ok()?;
    Some(scratch)
}

/// True when writing a member at `target` cannot fail on the path's shape:
/// every folder from `dest` down is missing, or a folder (never a link)
/// this process (`me`) owns and may write, and the file itself is missing
/// or a regular file it owns and may write.
fn member_target_ok(dest: &Path, target: &Path, me: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(relative) = target.strip_prefix(dest) else {
        return false;
    };
    // A folder a member is written into must be one this process may
    // write and search, or a later member could fail after earlier ones.
    let open_dir = |meta: &std::fs::Metadata| {
        meta.is_dir() && meta.uid() == me && meta.permissions().mode() & 0o300 == 0o300
    };
    match std::fs::symlink_metadata(dest) {
        Ok(meta) if open_dir(&meta) => {}
        Ok(_) => return false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    }
    let parts: Vec<_> = relative.components().collect();
    let mut path = dest.to_path_buf();
    for (at, part) in parts.iter().enumerate() {
        path.push(part);
        let last = at + 1 == parts.len();
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            Err(_) => return false,
            Ok(meta) if meta.file_type().is_symlink() => return false,
            Ok(meta) if !last => {
                if !open_dir(&meta) {
                    return false;
                }
            }
            Ok(meta) => {
                return meta.file_type().is_file()
                    && meta.uid() == me
                    && meta.permissions().mode() & 0o200 != 0;
            }
        }
    }
    false
}

/// The `backup.restore` row Python's `events.emit` appends, keys sorted.
fn restore_row(files: usize) -> String {
    let mut out = format!(
        "{{\"event\": \"backup.restore\", \"files\": {files}, \"status\": \"ok\", \"ts\": "
    );
    crate::render::py_string(&crate::events::stamp(now_secs()), &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(crate::events::version(), &mut out);
    out.push_str("}\n");
    out
}

fn restore(file: &str, dest: &str, ctx: &Ctx) -> Option<Outcome> {
    use std::os::unix::fs::MetadataExt;
    // The output lines go through `privacy.redact`; with none of its inputs
    // present it reads no term. Relative or `~` paths depend on the working
    // folder or the home, and go to Python.
    if ctx.redact_inputs_present() || !env_is_plain() {
        return None;
    }
    if !plain_absolute(file) || !plain_absolute(dest) {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    if !root_text.starts_with('/') {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    // A corporate machine checks cloud folders and job-hunt members, and
    // writes `machine.yaml` into the folder: Python runs it.
    if !personal(&store, &ctx.root)? {
        return None;
    }
    let resolved = resolve_plain(Path::new(dest))?;
    let root = std::fs::canonicalize(&ctx.root).ok()?;
    // Restoring into the data root (or a folder above it) changes the files
    // `privacy.redact` reads before Python prints.
    if root.starts_with(&resolved) {
        return None;
    }
    match std::fs::symlink_metadata(&resolved) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let backup = Path::new(file);
    careerbrew_vault::backup::verify(backup, &vault.key_file, &ctx.root, None).ok()?;
    // A trial extraction proves the archive's names, member types and
    // layout (no file where another member needs a folder) before the
    // destination is touched.
    let scratch = scratch_dir()?;
    let me = std::fs::metadata(&scratch.0).ok()?.uid();
    let trial = scratch.0.join("trial");
    let count = careerbrew_vault::backup::restore(
        backup,
        &trial,
        &vault.key_file,
        &ctx.root,
        careerbrew_vault::backup::Role::Personal,
    )
    .ok()?;
    let mut files = Vec::new();
    tree_files(&trial, &trial, &mut files)?;
    // Two members with one name: Python writes both; leave it to Python.
    if files.len() != count {
        return None;
    }
    for (relative, _) in &files {
        if !member_target_ok(&resolved, &resolved.join(relative), me) {
            return None;
        }
    }
    // Nothing is written to the destination before this point.
    let written = careerbrew_vault::backup::restore(
        backup,
        &resolved,
        &vault.key_file,
        &ctx.root,
        careerbrew_vault::backup::Role::Personal,
    )
    .ok()?;
    if written != count {
        return None;
    }
    for (relative, mode) in &files {
        std::fs::set_permissions(
            resolved.join(relative),
            std::fs::Permissions::from_mode(data_filter_mode(*mode)),
        )
        .ok()?;
    }
    drop(scratch);
    if std::env::var("CAREERBREW_EVENTS").as_deref() != Ok("off") {
        let logs = ctx.root.join("logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            if let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("events.jsonl"))
            {
                let _ = log.write_all(restore_row(count).as_bytes());
            }
        }
    }
    Some(Outcome {
        stdout: format!(
            "restored {count} file(s) to {dest}\nthe machine role is not part of a backup: a folder restored on a corporate machine is marked corporate; check with `CAREERBREW_HOME=<folder> careerbrew machine show`\n"
        ),
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}
