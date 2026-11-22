//! `cmd_data_edit` and `cmd_secret_set` behind injected adapters. These
//! functions are component-only; the production Python handlers stay routed.

use crate::canon::Canon;
use crate::events::{self, Clock};
use crate::helpers::{self, OutlineError};
use crate::secrets::{self, Adapter as SecretAdapter};
use crate::store::Store;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: u8,
    pub stdout: String,
}

impl Outcome {
    fn line(code: u8, line: impl Into<String>) -> Self {
        Self {
            code,
            stdout: format!("{}\n", line.into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError(pub &'static str);

pub trait Terminal {
    fn stdin_is_terminal(&self) -> bool;
    fn read_hidden(&mut self, prompt: &str) -> Result<String, CliError>;
    /// Called after every hidden-input attempt, including a read error.
    fn restore_echo(&mut self);
}

pub trait Editor {
    fn run(&mut self, argv: &[String]) -> Result<(), CliError>;
}

/// Python's `shlex.split` for an editor command: whitespace splits unquoted
/// text, quotes group it, and backslash follows POSIX double-quote rules.
pub fn editor_argv(text: &str) -> Result<Vec<String>, CliError> {
    #[derive(Clone, Copy, PartialEq)]
    enum Quote {
        None,
        Single,
        Double,
    }
    let mut quote = Quote::None;
    let mut escaped = false;
    let mut word = String::new();
    let mut started = false;
    let mut out = Vec::new();
    for ch in text.chars() {
        if escaped {
            // POSIX shlex inside double quotes removes a backslash only
            // before $, backtick, double quote, backslash or newline. Before
            // any other character (the a\q control), the backslash remains.
            if quote == Quote::Double && !matches!(ch, '$' | '`' | '"' | '\\' | '\n') {
                word.push('\\');
            }
            if ch != '\n' {
                word.push(ch);
            }
            escaped = false;
            started = true;
            continue;
        }
        match quote {
            Quote::Single => {
                if ch == '\'' {
                    quote = Quote::None;
                } else {
                    word.push(ch);
                }
                started = true;
            }
            Quote::Double => match ch {
                '"' => {
                    quote = Quote::None;
                    started = true;
                }
                '\\' => escaped = true,
                _ => {
                    word.push(ch);
                    started = true;
                }
            },
            Quote::None => match ch {
                '\\' => {
                    escaped = true;
                    started = true;
                }
                '\'' => {
                    quote = Quote::Single;
                    started = true;
                }
                '"' => {
                    quote = Quote::Double;
                    started = true;
                }
                c if c.is_whitespace() => {
                    if started {
                        out.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                _ => {
                    word.push(ch);
                    started = true;
                }
            },
        }
    }
    if escaped || quote != Quote::None {
        return Err(CliError("editor-command"));
    }
    if started {
        out.push(word);
    }
    if out.is_empty() {
        return Err(CliError("editor-command"));
    }
    Ok(out)
}

fn data_file(root: &Path, name: &str) -> Result<PathBuf, Outcome> {
    let missing = || {
        Outcome::line(
            1,
            format!("no such data file: {name} (paths are relative to the data root, e.g. profile.yaml)"),
        )
    };
    let root = root.canonicalize().map_err(|_| missing())?;
    let target = root.join(name).canonicalize().map_err(|_| missing())?;
    if !target.starts_with(&root) || !target.is_file() {
        return Err(missing());
    }
    Ok(target)
}

pub fn cmd_data_show(
    store: &Store,
    name: &str,
    keys: bool,
    may_show_business: bool,
    events_env: Option<&str>,
    clock: &dyn Clock,
) -> Result<Outcome, CliError> {
    let name = name.strip_prefix("private/").unwrap_or(name);
    let target = match data_file(store.root.as_path(), name) {
        Ok(target) => target,
        Err(outcome) => return Ok(outcome),
    };
    let root = store
        .root
        .canonicalize()
        .map_err(|_| CliError("data-file"))?;
    let relative = target
        .strip_prefix(&root)
        .map_err(|_| CliError("data-file"))?;
    let rel = relative.to_string_lossy().replace('\\', "/");
    let personal = relative.components().count() == 1
        || relative.components().next().is_some_and(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("jobs" | "tutor" | "prep" | "linkedin")
            )
        });
    let hidden = !personal && !may_show_business;
    let emit = |status: &str, items: Option<usize>| -> Result<(), CliError> {
        let mut fields = vec![(Canon::Str("status".into()), Canon::Str(status.into()))];
        if let Some(items) = items {
            fields.push((
                Canon::Str("items".into()),
                Canon::Int(num_bigint::BigInt::from(items)),
            ));
        }
        events::emit(
            &store.root,
            events_env,
            "data.show",
            &Canon::Map(fields),
            clock,
        )
        .map_err(|_| CliError("data-event"))
    };

    if keys {
        let outline = match helpers::outline(store, &target, hidden) {
            Ok(outline) => outline,
            Err(OutlineError::Store(crate::atomic::StoreError::Utf8)) => {
                return Ok(Outcome::line(1, format!("{rel} is not a text file")))
            }
            Err(OutlineError::Store(_)) => return Err(CliError("data-read")),
            Err(OutlineError::Value(message)) => {
                return Ok(Outcome::line(1, format!("{rel}: {message}")))
            }
        };
        let unit = if outline.kind == "jsonl" {
            "record(s)"
        } else {
            "line(s)"
        };
        let note = if outline.kind == "text" {
            " (no keys: not YAML, JSON, or JSONL)"
        } else {
            ""
        };
        let mut output = format!(
            "{rel}: {}, {} on disk, {} {unit}{note}\n",
            outline.kind,
            if outline.sealed {
                "encrypted"
            } else {
                "plaintext"
            },
            outline.count
        );
        for line in &outline.keys {
            output.push_str(line);
            output.push('\n');
        }
        emit("keys", Some(outline.keys.len()))?;
        return Ok(Outcome {
            code: 0,
            stdout: output,
        });
    }
    if hidden {
        let output = Outcome::line(
            3,
            format!(
                "{rel} may hold business data: its text prints only to your own terminal or the web app, or when a plugin policy sets agent_may_read_business. Agents can read files at the top of the data root and in jobs/, tutor/, prep/, linkedin/. For its structure: careerbrew data show {rel} --keys"
            ),
        );
        emit("hidden", None)?;
        return Ok(output);
    }
    let text = match store.read_text(&target, Some(""), true) {
        Ok(text) => text.unwrap_or_default(),
        Err(crate::atomic::StoreError::Utf8) => {
            return Ok(Outcome::line(1, format!("{rel} is not a text file")))
        }
        Err(_) => return Err(CliError("data-read")),
    };
    let count = helpers::splitlines(&text).len();
    let output = Outcome::line(0, text.trim_end_matches('\n'));
    emit("text", Some(count))?;
    Ok(output)
}

pub fn cmd_data_edit(
    store: &Store,
    name: &str,
    editor_command: &str,
    terminal: &dyn Terminal,
    editor: &mut dyn Editor,
) -> Result<Outcome, CliError> {
    let root = store.root.as_path();
    let target = match data_file(root, name) {
        Ok(target) => target,
        Err(outcome) => return Ok(outcome),
    };
    if !terminal.stdin_is_terminal() {
        return Ok(Outcome::line(
            1,
            "data edit needs a terminal: it opens your $EDITOR",
        ));
    }
    let filename = target
        .file_name()
        .ok_or(CliError("data-file"))?
        .to_os_string();
    let tmp = root.join(".edit").join(filename);
    let edit_dir = tmp.parent().ok_or(CliError("data-file"))?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(edit_dir).map_err(|_| CliError("edit-dir"))?;
    let argv = editor_argv(editor_command)?;
    let result = (|| -> Result<(), CliError> {
        let initial = store
            .read_text(&target, Some(""), true)
            .map_err(|_| CliError("data-read"))?
            .unwrap_or_default();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|_| CliError("edit-write"))?;
        file.write_all(initial.as_bytes())
            .map_err(|_| CliError("edit-write"))?;
        drop(file);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| CliError("edit-mode"))?;
        let mut invocation = argv.clone();
        invocation.push(tmp.to_string_lossy().into_owned());
        editor.run(&invocation)?;
        let text = std::fs::read_to_string(&tmp).map_err(|_| CliError("edit-read"))?;
        match target.extension().and_then(|suffix| suffix.to_str()) {
            Some("yaml" | "yml") => {
                crate::json_yaml::yaml_read(&text).map_err(|_| CliError("invalid-yaml"))?;
            }
            Some("json") => {
                crate::json_yaml::json_read(&text).map_err(|_| CliError("invalid-json"))?;
            }
            _ => {}
        }
        store
            .write_text(&target, &text)
            .map_err(|_| CliError("data-write"))?;
        Ok(())
    })();
    let cleanup = std::fs::remove_file(&tmp);
    if let Err(error) = result {
        let _ = cleanup;
        return Err(error);
    }
    match cleanup {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CliError("edit-cleanup")),
    }
    Ok(Outcome::line(
        0,
        if store.encryption_enabled() {
            format!("saved {name} (encrypted)")
        } else {
            format!("saved {name}")
        },
    ))
}

pub fn cmd_secret_set(
    service: &str,
    terminal: &mut dyn Terminal,
    keychain: &dyn SecretAdapter,
) -> Result<Outcome, CliError> {
    if !terminal.stdin_is_terminal() {
        return Ok(Outcome::line(
            3,
            "refusing to read a secret from a pipe; run this in your terminal",
        ));
    }
    let prompt = format!("value for {service} (input hidden): ");
    let read = terminal.read_hidden(&prompt);
    terminal.restore_echo();
    let value = read?;
    if value.is_empty() {
        return Ok(Outcome::line(1, "nothing stored"));
    }
    let reference = secrets::store_keychain(keychain, service, &value)
        .map_err(|_| CliError("keychain-write"))?;
    Ok(Outcome::line(0, format!("stored as {reference}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_commands_keep_quotes_and_refuse_open_quotes() {
        assert_eq!(
            editor_argv("editor --wait 'two words'").unwrap(),
            ["editor", "--wait", "two words"]
        );
        assert!(editor_argv("editor 'open").is_err());
        assert!(editor_argv("").is_err());
        // Python shlex: inside double quotes, a backslash before q stays.
        assert_eq!(
            editor_argv(r#"editor "a\q""#).unwrap(),
            ["editor", r#"a\q"#]
        );
    }
}
