//! rust-26: registry commands whose domain crate is merged and whose
//! end-to-end parity test passes (`crates/cbcli/tests/parity.rs`). Each one
//! calls the domain crate's own handler; this module only parses argv and
//! decides when to refuse.
//!
//! A run refuses with exit 64 (Python then runs the command) when:
//! - the argv is not the exact bare form of the command (any flag or extra
//!   argument),
//! - the data root cannot be resolved the way Python resolves it,
//! - Python's `privacy.redact` would read an input (`REDACT_INPUTS`), or
//! - the domain handler returns an error (Python prints its own `error:`
//!   line and exit 2).
//!
//! Every routed command is read-only, so a refusal never follows a write.
use crate::{events, Env, Outcome, REDACT_INPUTS, UNSUPPORTED};

/// The routed commands, as route-table keys. The parity test fails when a
/// `native` route in `config/native-routes.txt` is not listed here.
pub const COMMANDS: &[&str] = &[
    "machine.show",
    "plugin.list",
    "plugin.verify",
    "connectors.list",
    "data.status",
    "backup.verify",
];

fn refuse(name: &str) -> Outcome {
    Outcome {
        stdout: String::new(),
        stderr: format!("native: unsupported {name}\n"),
        code: UNSUPPORTED,
        emit_event: false,
    }
}

/// True when `pathlib.Path` prints `path` unchanged.
fn plain_path(path: &str) -> bool {
    !(path.contains("//")
        || path.contains("/./")
        || path.starts_with("./")
        || path.ends_with("/.")
        || (path.len() > 1 && path.ends_with('/'))
        || path == ".")
}

/// The route-table key an argv names, when it is one of `COMMANDS` in its
/// bare form (exactly two words, no flag).
pub fn command_of(args: &[String]) -> Option<&'static str> {
    let [group, sub] = args else {
        return None;
    };
    let key = format!("{group}.{sub}");
    COMMANDS.iter().copied().find(|name| *name == key)
}

/// Runs a routed command, or returns None when `args` names none.
pub fn run(args: &[String], env: &Env) -> Option<Outcome> {
    let name = command_of(args)?;
    let Some(root) = env.caps.data_root() else {
        return Some(refuse(name));
    };
    if REDACT_INPUTS.iter().any(|file| root.join(file).exists()) {
        return Some(refuse(name));
    }
    // The domain crates read the data root from `CAREERBREW_HOME`, as the
    // synthetic components do. `main` sets it from `Caps` before `run`.
    if std::env::var_os("CAREERBREW_HOME").as_deref() != Some(root.as_os_str()) {
        return Some(refuse(name));
    }
    let result = match name {
        "machine.show" => careerbrew_setup::cli::cmd_machine_show().map(|out| (out, 0)),
        "plugin.list" => careerbrew_setup::cli::cmd_plugin_list().map(|out| (out, 0)),
        "plugin.verify" => careerbrew_setup::cli::cmd_plugin_verify(),
        "connectors.list" => careerbrew_setup::cli::cmd_connectors_list().map(|out| (out, 0)),
        "data.status" | "backup.verify" => {
            // `data status` prints the key path. Python prints it through
            // `pathlib`, which folds `//`, `/./` and a trailing `/`; a path
            // in that form goes to Python.
            if ["HOME", "CAREERBREW_KEY_FILE"]
                .iter()
                .any(|key| std::env::var(key).is_ok_and(|value| !plain_path(&value)))
            {
                return Some(refuse(name));
            }
            let Ok(ctx) = careerbrew_vault::transfer::Context::from_env() else {
                return Some(refuse(name));
            };
            let out = if name == "data.status" {
                ctx.cmd_data_status()
            } else {
                ctx.cmd_backup_verify(None)
            };
            return Some(match out {
                Ok(stdout) => Outcome {
                    stdout,
                    stderr: String::new(),
                    code: 0,
                    emit_event: true,
                },
                Err(_) => refuse(name),
            });
        }
        _ => return Some(refuse(name)),
    };
    Some(match result {
        Ok((stdout, code)) => Outcome {
            stdout,
            stderr: String::new(),
            code,
            emit_event: true,
        },
        Err(_) => refuse(name),
    })
}

/// Sets `CAREERBREW_HOME` to the data root Python resolves (`~` expanded,
/// or the default `<repo>/private`) for a routed argv. Called once in
/// `main`, before any thread starts.
pub fn export_home(args: &[String], caps: &events::Caps) {
    if command_of(args).is_none() && crate::steps::command_of(args).is_none() {
        return;
    }
    if let Some(home) = &caps.home {
        std::env::set_var("CAREERBREW_HOME", home);
    }
}
