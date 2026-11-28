//! Native CLI handlers owned by `rust-07` (see `steps/mod.rs`).
//!
//! `init` copies the example files into the data root, as Python's
//! `cli.cmd_init` does. Every check that could make Python fail, and every
//! input that could make its `privacy.redact` change the output, is read
//! before the first write; any doubt refuses (exit 64) and Python runs it.
//!
//! `machine set personal [--label <label>]` writes `machine.yaml`, as
//! Python's `machine.set_role` does for the personal role. It is served only
//! when the file is missing, empty, or holds just `role` and `label`, and
//! every value written is a plain YAML word (see `plain_word`), so the text
//! equals PyYAML's `safe_dump`. The corporate role (which deletes folders)
//! and anything else go to Python.
//!
//! `plugin refresh` and `machine split-plugin-evidence [--dry-run]` are
//! served only with no plugin installed: no `plugins.lock.json` (which is
//! also a redaction input) and no agent command fragment. Python then
//! reinstalls nothing, runs no deployer and edits no plugin file; the
//! split only counts the entries of `work/sources.yaml` and
//! `work/connectors.yaml`. Any plugin, base evidence folder or entry shape
//! Python could fail on goes to Python.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust07.rs`.
pub const COMMANDS: &[&str] = &[
    "init",
    "machine.set",
    "plugin.refresh",
    "machine.split-plugin-evidence",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    match name {
        "init" => init(ctx),
        "machine.set" => machine_set(args.text("role")?, args, ctx),
        "plugin.refresh" => plugin_refresh(ctx),
        "machine.split-plugin-evidence" => split_plugin_evidence(args.flag("dry_run"), ctx),
        _ => None,
    }
}

/// `privacy.TEMPLATE_PLACEHOLDERS`: unedited example values, never terms.
const PLACEHOLDERS: &[&str] = &["Your Name", "Your Employer"];

/// Characters Python's `str.splitlines` splits on, besides `\n`, and the
/// universal-newline `\r` that `read_text` would translate. Example text
/// holding any of them is left to Python.
fn odd_text(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\r' | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{1f}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    })
}

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

/// The key file and home the vault context reads must be the ones Python
/// resolves: a set, non-empty `HOME`, and no `~user` key path.
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

/// `machine.role() == "corporate"`, or None when Python could fail or read
/// the file differently (a top level that is neither a mapping nor empty,
/// two `role` keys, an unreadable or sealed file without its key).
fn corporate(store: &careerbrew_vault::store::Store, root: &Path) -> Option<bool> {
    let info = store
        .read_yaml(&root.join("machine.yaml"), Canon::Map(Vec::new()), true)
        .ok()?;
    let Canon::Map(items) = info else {
        return None;
    };
    let roles: Vec<&Canon> = items
        .iter()
        .filter(|(key, _)| matches!(key, Canon::Str(key) if key == "role"))
        .map(|(_, value)| value)
        .collect();
    if roles.len() > 1 {
        return None;
    }
    Some(matches!(roles.first(), Some(Canon::Str(role)) if role == "corporate"))
}

/// The repo whose `profile/examples` Python reads: the console script
/// passes it in `CAREERBREW_NATIVE_REPO`; a direct run uses the checkout
/// the binary was built from.
fn examples_dir() -> Option<PathBuf> {
    let repo = match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => PathBuf::from(repo),
        Ok(_) => return None,
        Err(_) => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")),
    };
    let dir = repo.join("profile/examples");
    dir.is_dir().then_some(dir)
}

/// Every file under `dir`, relative to `base`. None on a symlink, a
/// non-UTF-8 name or a read error: `Path.rglob` may treat those otherwise.
fn example_files(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Option<()> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let meta = std::fs::symlink_metadata(&path).ok()?;
        path.file_name()?.to_str()?;
        if meta.file_type().is_symlink() {
            return None;
        } else if meta.is_dir() {
            example_files(base, &path, out)?;
        } else if meta.is_file() {
            out.push(path.strip_prefix(base).ok()?.to_path_buf());
        } else {
            return None;
        }
    }
    Some(())
}

/// True when `privacy.sensitive_terms` finds no term in this
/// `sensitive-terms.txt` text (lines that are empty, comments, or shorter
/// than three characters).
fn no_terms_in_list(text: &str) -> bool {
    text.split('\n').all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with('#') || line.chars().count() < 3
    })
}

/// True when `privacy.sensitive_terms` takes no term from this
/// `profile.yaml` text: name, employer and email are absent, short, not
/// text, or placeholders, and `sensitive` holds no text of three or more.
fn no_terms_in_profile(text: &str) -> bool {
    let Ok(value) = careerbrew_vault::json_yaml::yaml_read(text) else {
        return false;
    };
    let items = match value {
        Canon::Null => return true,
        Canon::Map(items) => items,
        _ => return false,
    };
    let mut seen: Vec<&str> = Vec::new();
    for (key, value) in &items {
        let Canon::Str(key) = key else {
            continue;
        };
        if seen.contains(&key.as_str()) {
            return false;
        }
        seen.push(key);
        match key.as_str() {
            "name" | "employer" | "email" => {
                if let Canon::Str(text) = value {
                    if text.chars().count() >= 3 && !PLACEHOLDERS.contains(&text.as_str()) {
                        return false;
                    }
                }
            }
            "sensitive" => match value {
                Canon::Null | Canon::Bool(false) => {}
                Canon::List(extra) => {
                    if extra
                        .iter()
                        .any(|item| matches!(item, Canon::Str(text) if text.chars().count() >= 3))
                    {
                        return false;
                    }
                }
                _ => return false,
            },
            _ => {}
        }
    }
    true
}

/// True when every folder between `root` and `target` is a folder or is
/// missing, so `mkdir(parents=True)` cannot fail.
fn parents_ok(root: &Path, target: &Path) -> bool {
    let mut dir = target.parent();
    while let Some(path) = dir {
        if path == root {
            return true;
        }
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
        dir = path.parent();
    }
    false
}

fn init(ctx: &Ctx) -> Option<Outcome> {
    // Python prints through `privacy.redact`, which reads these files. They
    // are absent here, so the only terms it can read are the ones this run
    // writes, which are checked below.
    if ctx.redact_inputs_present() || !env_is_plain() {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    if !root_text.starts_with('/') || !plain_path(root_text) {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    let corporate = corporate(&store, &ctx.root)?;
    let examples = examples_dir()?;
    let mut files = Vec::new();
    example_files(&examples, &examples, &mut files)?;
    files.sort();
    let mut plan: Vec<(PathBuf, String, String)> = Vec::new();
    for relative in files {
        let first = relative.components().next()?.as_os_str().to_str()?;
        if corporate && careerbrew_setup::machine::PERSONAL_DIRS.contains(&first) {
            continue;
        }
        let target = ctx.root.join(&relative);
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => return None,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        if !parents_ok(&ctx.root, &target) {
            return None;
        }
        let mut text = std::fs::read_to_string(examples.join(&relative)).ok()?;
        if odd_text(&text) {
            return None;
        }
        let shown = relative.to_str()?.to_owned();
        if corporate && shown == "profile.yaml" {
            text = careerbrew_setup::machine::work_profile(&text);
        }
        if shown == "sensitive-terms.txt" && !no_terms_in_list(&text) {
            return None;
        }
        if shown == "profile.yaml" && !no_terms_in_profile(&text) {
            return None;
        }
        plan.push((target, text, shown));
    }
    // The first sealed write creates the key in Python; load (or create) it
    // first so a key error refuses before any data file is written.
    if !plan.is_empty() && store.encryption_enabled() {
        careerbrew_vault::synthetic::key_for_data(&vault.root, &vault.key_file, true).ok()?;
    }
    let mut created = Vec::new();
    for (target, text, shown) in plan {
        let parent = target.parent()?;
        if std::fs::create_dir_all(parent).is_err() || store.write_text(&target, &text).is_err() {
            // Nothing is written before the first file; a later IO failure
            // is left to Python, whose rerun skips the files already there.
            return None;
        }
        created.push(shown);
    }
    let next = if corporate {
        "next: set name and employer with `careerbrew data set profile.yaml --name '<name>' --employer '<employer>'`, then follow docs/corporate-machine.md"
    } else {
        "next: `careerbrew data edit profile.yaml` (also jobs/criteria.yaml, jobs/sources.yaml), then `careerbrew jobs scan`"
    };
    let list = if created.is_empty() {
        "none (all present)".to_owned()
    } else {
        created.join(", ")
    };
    Some(Outcome {
        stdout: format!(
            "data root: {root_text}\ncreated {} file(s): {list}\n{next}\n",
            created.len()
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

/// `machine.set_role("personal", label)`: `{**info(), "role": "personal",
/// "label": label or info().get("label") or "personal"}` written with
/// `store.write_yaml`. The output line goes through `privacy.redact`, which
/// reads nothing here (`redact_inputs_present` refuses otherwise).
fn machine_set(role: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if role != "personal" || ctx.redact_inputs_present() || !env_is_plain() {
        return None;
    }
    let label = args.text("label").unwrap_or("");
    if !label.is_empty() && !plain_word(label) {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    let path = ctx.root.join("machine.yaml");
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let info = store.read_yaml(&path, Canon::Map(Vec::new()), true).ok()?;
    let Canon::Map(items) = info else {
        return None;
    };
    // Only `role` and `label`, once each, as plain words: PyYAML then
    // writes each as `key: value` in the mapping's order.
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (key, value) in items {
        let (Canon::Str(key), Canon::Str(value)) = (key, value) else {
            return None;
        };
        if !matches!(key.as_str(), "role" | "label")
            || pairs.iter().any(|(seen, _)| *seen == key)
            || !plain_word(&value)
        {
            return None;
        }
        pairs.push((key, value));
    }
    let old_label = pairs
        .iter()
        .find(|(key, _)| key == "label")
        .map(|(_, value)| value.clone());
    let new_label = if label.is_empty() {
        old_label.unwrap_or_else(|| "personal".to_owned())
    } else {
        label.to_owned()
    };
    for (key, value) in [("role", role.to_owned()), ("label", new_label)] {
        match pairs.iter_mut().find(|(name, _)| name == key) {
            Some(pair) => pair.1 = value,
            None => pairs.push((key.to_owned(), value)),
        }
    }
    let text: String = pairs
        .iter()
        .map(|(key, value)| format!("{key}: {value}\n"))
        .collect();
    // The first sealed write creates the key in Python; load (or create) it
    // first so a key error refuses before the file is written.
    if store.encryption_enabled() {
        careerbrew_vault::synthetic::key_for_data(&vault.root, &vault.key_file, true).ok()?;
    }
    store.write_text(&path, &text).ok()?;
    Some(Outcome {
        stdout: format!("role set: {role}\n"),
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// True when `path` is absent: no file, folder or link (even a broken one).
fn absent(path: &Path) -> Option<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Some(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
        Err(_) => None,
    }
}

/// No plugin is installed and no agent command fragment exists, so
/// Python's `plugins.installed()` is empty and `_sync_command_sources`
/// writes nothing and runs no deployer. The lock is also a redaction input.
fn no_plugins(ctx: &Ctx) -> Option<()> {
    if ctx.redact_inputs_present() {
        return None;
    }
    if !absent(&ctx.root.join("plugins.lock.json"))? || !absent(&ctx.root.join("agentbrew"))? {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    root_text.starts_with('/').then_some(())
}

fn plugin_refresh(ctx: &Ctx) -> Option<Outcome> {
    no_plugins(ctx)?;
    Some(Outcome {
        stdout: "refreshed 0 plugin(s): none installed\n".to_owned(),
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// The number of entries under `key` in one private evidence file, as
/// both `split_plugin_evidence` (`.get(key) or []`) and
/// `evidence.sources_list` (`.get(key, [])`, each entry unpacked as a
/// mapping) read it; None where either could fail or read it otherwise.
fn evidence_count(store: &careerbrew_vault::store::Store, path: &Path, key: &str) -> Option<usize> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(0),
        Err(_) => return None,
    }
    let value = store.read_yaml(path, Canon::Map(Vec::new()), false).ok()?;
    let Canon::Map(items) = value else {
        return None;
    };
    let mut found: Option<&Canon> = None;
    let mut seen: Vec<&str> = Vec::new();
    for (name, value) in &items {
        let Canon::Str(name) = name else {
            continue;
        };
        if seen.contains(&name.as_str()) {
            return None;
        }
        seen.push(name);
        if name == key {
            found = Some(value);
        }
    }
    match found {
        None => Some(0),
        Some(Canon::List(entries)) if entries.iter().all(|it| matches!(it, Canon::Map(_))) => {
            Some(entries.len())
        }
        Some(_) => None,
    }
}

/// The repo whose `knowledge/` Python reads (`paths.REPO_ROOT`).
fn repo_root() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(_) => Some(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))),
    }
}

/// The `machine.split_evidence` row Python's `events.emit` appends, keys
/// sorted.
fn split_row(dry_run: bool) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let status = if dry_run { "dry_run" } else { "none" };
    let mut out = format!(
        "{{\"event\": \"machine.split_evidence\", \"left\": 0, \"moved\": 0, \"skipped\": 0, \"status\": \"{status}\", \"ts\": "
    );
    crate::render::py_string(&crate::events::stamp(secs), &mut out);
    out.push_str(", \"v\": ");
    crate::render::py_string(crate::events::version(), &mut out);
    out.push_str("}\n");
    out
}

/// `machine.split_plugin_evidence` with no plugin installed: nothing moves
/// and nothing is written except the `work` folder `paths.sub` makes.
fn split_plugin_evidence(dry_run: bool, ctx: &Ctx) -> Option<Outcome> {
    no_plugins(ctx)?;
    // Base evidence files would add to the count and the per-person check.
    if !absent(&repo_root()?.join("knowledge/evidence_sources"))? {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    let work = ctx.root.join("work");
    match std::fs::symlink_metadata(&work) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    let sources = evidence_count(&store, &work.join("sources.yaml"), "sources")?;
    let connectors = evidence_count(&store, &work.join("connectors.yaml"), "connectors")?;
    let total = sources + connectors;
    std::fs::create_dir_all(&work).ok()?;
    let mut stdout = String::from(
        "nothing to move: no plugin folder here holds per-person sources or connectors\n",
    );
    stdout.push_str(&format!(
        "your work/sources.yaml: {sources} -> {sources} source(s); work/connectors.yaml: {connectors} -> {connectors} connector(s)\n"
    ));
    stdout.push_str(&format!(
        "evidence sources work collect reads: {total} -> {total}\n"
    ));
    if dry_run {
        stdout.push_str("dry run: nothing written; rerun without --dry-run to move them\n");
    } else {
        stdout.push_str(&format!(
            "ok    {}\n",
            careerbrew_setup::machine::PER_USER_CHECK
        ));
    }
    if std::env::var("CAREERBREW_EVENTS").as_deref() != Ok("off") {
        let logs = ctx.root.join("logs");
        if std::fs::create_dir_all(&logs).is_ok() {
            if let Ok(mut log) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(logs.join("events.jsonl"))
            {
                use std::io::Write;
                let _ = log.write_all(split_row(dry_run).as_bytes());
            }
        }
    }
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}
