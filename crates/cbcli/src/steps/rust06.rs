//! Native CLI handlers owned by `rust-06` (see `steps/mod.rs`).
//!
//! `sync export <file>` writes the encrypted personal → corporate settings
//! bundle, as Python's `sync.export` does. Only the success path is served:
//! every case where Python prints `error:` (a corporate machine, a bundle
//! inside the data folder, no pairing key) refuses before any write, and
//! Python then prints its own error.
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
pub const COMMANDS: &[&str] = &["sync.export"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    match name {
        "sync.export" => export(args.text("file")?, ctx),
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
