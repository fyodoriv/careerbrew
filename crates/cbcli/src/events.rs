//! The one write the pilot makes: a content-free `cli.run` row appended to
//! `<data root>/logs/events.jsonl`, the row Python's `events.emit` writes
//! after `careerbrew actions`. It never sends telemetry
//! (docs/decisions/0010-native-pilot-hooks.md): the next Python command
//! sends the queue.
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

/// The careerbrew version, from the same `pyproject.toml` Python's package
/// metadata reads.
const PYPROJECT: &str = include_str!("../../../pyproject.toml");

/// What the binary may touch, built from the environment only: nothing in
/// this crate reaches the store or the vault.
pub struct Caps {
    /// The data root, or None when it cannot be resolved the way Python
    /// does (an empty `CAREERBREW_HOME`, `~user`, or an unset home with no
    /// `CAREERBREW_NATIVE_REPO`).
    pub home: Option<PathBuf>,
    /// False when `CAREERBREW_EVENTS=off`.
    pub events_on: bool,
    /// `agent` when an agent host's variable is set, else `cli`.
    pub via: &'static str,
}

impl Caps {
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Caps {
        let home = match get("CAREERBREW_HOME") {
            // Python raises on an empty value; exit 64 lets it do so.
            Some(value) if value.trim().is_empty() => None,
            Some(value) => expand_user(&value, get("HOME")).map(PathBuf::from),
            // Python's default is `<repo>/private`, with the repo found from
            // its own package. The console script passes that repo in
            // `CAREERBREW_NATIVE_REPO`; a run without it goes to Python, as a
            // folder fixed at build time could be another checkout.
            None => get("CAREERBREW_NATIVE_REPO")
                .filter(|repo| repo.starts_with('/'))
                .map(|repo| PathBuf::from(repo).join("private")),
        };
        let agent = ["CLAUDECODE", "CODEX_SANDBOX"]
            .iter()
            .any(|key| get(key).is_some_and(|value| !value.is_empty()));
        Caps {
            home,
            events_on: get("CAREERBREW_EVENTS").as_deref() != Some("off"),
            via: if agent { "agent" } else { "cli" },
        }
    }

    /// Python's `paths.data_root()`: create the root and make it 0700.
    pub fn data_root(&self) -> Option<PathBuf> {
        let root = self.home.clone()?;
        std::fs::create_dir_all(&root).ok()?;
        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
        Some(root)
    }
}

/// Python's `posixpath.expanduser` for `~` and `~/...`. `~user` and a
/// missing `HOME` return None: Python reads the password database there.
pub fn expand_user(path: &str, home: Option<String>) -> Option<String> {
    if !path.starts_with('~') {
        return Some(path.to_owned());
    }
    let rest = &path[1..];
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let home = home?;
    let joined = format!("{}{rest}", home.trim_end_matches('/'));
    Some(if joined.is_empty() {
        "/".to_owned()
    } else {
        joined
    })
}

pub fn version() -> &'static str {
    PYPROJECT
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .map_or("0.0.0", |value| value.trim().trim_matches('"'))
}

/// `%Y-%m-%dT%H:%M:%SZ` in UTC for seconds since the epoch.
pub fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The row, keys sorted as `json.dumps(sort_keys=True)` sorts them.
pub fn row(ts: &str, ms: u64, version: &str, via: &str) -> String {
    command_row("actions", true, ts, ms, version, via)
}

/// The `cli.run` row for one command (`machine.show`), with `status` `ok`
/// for exit 0 and `fail` otherwise, as `cli.main` writes it. `command` is
/// a fixed registry id, so it needs no escaping.
pub fn command_row(command: &str, ok: bool, ts: &str, ms: u64, version: &str, via: &str) -> String {
    let mut out = String::from("{\"command\": ");
    crate::render::py_string(command, &mut out);
    out.push_str(", \"event\": \"cli.run\", \"ms\": ");
    out.push_str(&ms.to_string());
    out.push_str(if ok {
        ", \"status\": \"ok\", \"ts\": "
    } else {
        ", \"status\": \"fail\", \"ts\": "
    });
    crate::render::py_string(ts, &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(version, &mut out);
    out.push_str(", \"via\": ");
    crate::render::py_string(via, &mut out);
    out.push_str("}\n");
    out
}

/// Appends the row when events are on. Errors are ignored: the command's
/// output is already written, as Python's would be.
pub fn emit(caps: &Caps, ms: u64) {
    emit_command(caps, "actions", true, ms);
}

/// The name `cli.main` logs: `cmd.sub` with at most two words, so
/// `work.live.stop` is logged as `work.live`.
pub fn python_name(command: &str) -> &str {
    match command.match_indices('.').nth(1) {
        Some((at, _)) => &command[..at],
        None => command,
    }
}

/// `emit` for a routed command (`crate::routed`).
pub fn emit_command(caps: &Caps, command: &str, ok: bool, ms: u64) {
    if !caps.events_on {
        return;
    }
    let Some(root) = caps.data_root() else {
        return;
    };
    let logs = root.join("logs");
    if std::fs::create_dir_all(&logs).is_err() {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let line = command_row(
        python_name(command),
        ok,
        &stamp(secs),
        ms,
        version(),
        caps.via,
    );
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("events.jsonl"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod name_tests {
    use super::python_name;

    #[test]
    fn a_three_word_command_is_logged_with_its_first_two_words() {
        assert_eq!(python_name("work.live.stop"), "work.live");
        assert_eq!(python_name("day.question.close"), "day.question");
        assert_eq!(python_name("config.unset"), "config.unset");
        assert_eq!(python_name("init"), "init");
    }
}
