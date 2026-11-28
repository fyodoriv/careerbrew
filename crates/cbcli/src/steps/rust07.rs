//! Native CLI handlers owned by `rust-07` (see `steps/mod.rs`).
//!
//! `init` copies the example files into the data root, as Python's
//! `cli.cmd_init` does. Every check that could make Python fail, and every
//! input that could make its `privacy.redact` change the output, is read
//! before the first write; any doubt refuses (exit 64) and Python runs it.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust07.rs`.
pub const COMMANDS: &[&str] = &["init"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, _args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    match name {
        "init" => init(ctx),
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
