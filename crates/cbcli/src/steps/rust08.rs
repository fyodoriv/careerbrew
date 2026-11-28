//! Native CLI handlers owned by `rust-08` (see `steps/mod.rs`).
//!
//! `pair init` and `pair join <key>` write the pairing key, as Python's
//! `sync.pair_init` and `sync.pair_join` do. Only the success path is
//! served. A run refuses (exit 64, Python then runs it) before any write
//! when Python could print `error:`, read a terminal, or print a path that
//! `pathlib` would spell otherwise:
//! - `pair init` on a terminal (Python then prints the key itself), and
//!   `pair join` with no key (Python reads a hidden prompt or stdin);
//! - a key that is not the canonical 44-character form;
//! - a key path inside the data folder, relative, or not plain;
//! - a key path or a folder above it that is not a file or folder;
//! - any redaction input, or a `machine.yaml` Python could read otherwise.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust08.rs`.
pub const COMMANDS: &[&str] = &["pair.init", "pair.join"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    match name {
        "pair.init" => pair_init(ctx),
        "pair.join" => pair_join(args.text("key")?, ctx),
        _ => None,
    }
}

const CORPORATE_LINE: &str = "export level: none. careerbrew never copies corporate data to your personal machine; pairing only receives settings (ladder, target level) from your personal machine.";

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

/// The pairing key path Python's `sync.key_path()` returns, as it prints
/// it, or None for a form this port does not resolve (a relative or
/// `~user` path, a path `pathlib` would print otherwise, an unset `HOME`).
fn key_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    if !home.starts_with('/') || !plain_path(&home) {
        return None;
    }
    let path = match std::env::var("CAREERBREW_PAIR_KEY_FILE") {
        Ok(value) if !value.is_empty() => crate::events::expand_user(&value, Some(home.clone()))?,
        _ => format!("{}/.config/careerbrew/pair.key", home.trim_end_matches('/')),
    };
    if !path.starts_with('/') || !plain_path(&path) || path == "/" {
        return None;
    }
    Some(PathBuf::from(path))
}

/// True when Python's `_write_key` cannot fail on the path's shape: the
/// key path is missing or a regular file (not a symlink), and every folder
/// above it is a folder or missing.
fn writable_shape(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return false,
    }
    let mut dir = path.parent();
    while let Some(folder) = dir {
        match std::fs::metadata(folder) {
            Ok(meta) => return meta.is_dir(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return false,
        }
        dir = folder.parent();
    }
    false
}

/// The checks both commands share, before any write: the key path, its
/// shape, and that it lies outside the data folder.
fn checked_key_path(ctx: &Ctx) -> Option<PathBuf> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let root_text = ctx.root.to_str()?;
    if !root_text.starts_with('/') {
        return None;
    }
    let path = key_path()?;
    path.to_str()?;
    careerbrew_vault::keyfile::require_outside_data_root(&path, &ctx.root).ok()?;
    if !writable_shape(&path) {
        return None;
    }
    Some(path)
}

/// `machine.role() == "corporate"`, or None when Python could fail or read
/// `machine.yaml` otherwise (not a mapping, a repeated key, a read error).
fn corporate(ctx: &Ctx) -> Option<bool> {
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let store = vault.store();
    let info = store
        .read_yaml(&ctx.root.join("machine.yaml"), Canon::Map(Vec::new()), true)
        .ok()?;
    let Canon::Map(items) = info else {
        return None;
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut role = None;
    for (key, value) in &items {
        let Canon::Str(key) = key else {
            continue;
        };
        if seen.contains(&key.as_str()) {
            return None;
        }
        seen.push(key);
        if key == "role" {
            role = Some(value);
        }
    }
    Some(matches!(role, Some(Canon::Str(role)) if role == "corporate"))
}

fn served(stdout: String) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

fn pair_init(ctx: &Ctx) -> Option<Outcome> {
    // On a terminal Python prints the key itself. The console script runs
    // this binary with its output captured, so a terminal shows only on
    // stdin there: either one goes to Python.
    if ctx.env.stdout_is_terminal || std::io::stdin().is_terminal() {
        return None;
    }
    let path = checked_key_path(ctx)?;
    let corporate = corporate(ctx)?;
    // Nothing is written before this point. A failed write is left to
    // Python, which makes the same folders and prints its own error.
    careerbrew_vault::sync::pair_init(&path, &ctx.root).ok()?;
    let mut stdout = String::new();
    if corporate {
        stdout.push_str(CORPORATE_LINE);
        stdout.push('\n');
    }
    stdout.push_str(&format!(
        "pairing key written to {} (not printed: stdout is not a terminal). Copy it to your password manager yourself.\n",
        path.to_str()?
    ));
    served(stdout)
}

/// The form `Fernet.generate_key` writes: 43 URL-safe characters and one
/// `=`. Python's lenient decoder and the Rust one read it alike.
fn canonical_key(key: &str) -> bool {
    key.len() == 44
        && key.ends_with('=')
        && key[..43]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn pair_join(key: &str, ctx: &Ctx) -> Option<Outcome> {
    // An empty key makes Python read stdin or a hidden prompt.
    if key.is_empty() || !canonical_key(key) {
        return None;
    }
    // The Rust Fernet reader must accept the key too; a key it refuses
    // (non-zero trailing bits) goes to Python, which may accept it.
    careerbrew_vault::seal(key, b"").ok()?;
    let path = checked_key_path(ctx)?;
    careerbrew_vault::sync::pair_join(&path, &ctx.root, key).ok()?;
    served("paired\n".to_owned())
}
