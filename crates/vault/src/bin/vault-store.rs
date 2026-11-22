//! Synthetic-only transport for the rust-05 component checks. No production
//! dispatch selects this binary. Usage errors exit 2 and refusals exit 1.

use careerbrew_vault::atomic::StoreError;
use careerbrew_vault::cli::{self, CliError, Editor, Terminal};
use careerbrew_vault::receipts::{ReceiptError, Receipts, Runtime};
use careerbrew_vault::secrets::{Adapter as SecretAdapter, SecretError};
use careerbrew_vault::store::Store;
use careerbrew_vault::{atomic, data, keyfile, lock, synthetic};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

fn key(path: &Path, root: &Path, create: bool) -> Result<String, String> {
    let no_sealed = || false;
    let facts = keyfile::KeyFacts {
        env_key_file: None,
        home: Path::new("/synthetic-home"),
        data_root: root,
        has_sealed_data: &no_sealed,
    };
    let bytes = keyfile::load_key(create, Some(path), &facts).map_err(|e| match e {
        keyfile::KeyError::InsideDataRoot => "inside-data-root".to_string(),
        keyfile::KeyError::Missing(_) => "missing-key".to_string(),
        _ => "key-error".to_string(),
    })?;
    String::from_utf8(bytes).map_err(|_| "key-error".to_string())
}

fn owned_root() -> Result<PathBuf, String> {
    std::env::var_os("CAREERBREW_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "home".into())
}

fn data_home() -> Result<PathBuf, String> {
    std::env::var_os("CAREERBREW_DATA_OWNED")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "data-home".into())
}

fn run_all() -> Result<String, String> {
    let root = owned_root()?;
    let key_file = root.parent().ok_or("home")?.join("keys/backup.key");
    let report = synthetic::run_all(&root, &key_file)?;
    let text = report.to_string();
    if text.contains(synthetic::CANARY) || text.contains(synthetic::SECRET) {
        return Err("privacy".into());
    }
    Ok(text + "\n")
}

struct ProcessEditor;
impl Editor for ProcessEditor {
    fn run(&mut self, argv: &[String]) -> Result<(), CliError> {
        let (program, args) = argv.split_first().ok_or(CliError("editor-command"))?;
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|_| CliError("editor-start"))?;
        if status.success() {
            Ok(())
        } else {
            Err(CliError("editor-exit"))
        }
    }
}

struct ProcessTerminal {
    saved: Option<libc::termios>,
}

impl ProcessTerminal {
    fn new() -> Self {
        Self { saved: None }
    }
}

impl Terminal for ProcessTerminal {
    fn stdin_is_terminal(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    fn read_hidden(&mut self, prompt: &str) -> Result<String, CliError> {
        {
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(prompt.as_bytes())
                .map_err(|_| CliError("terminal"))?;
            stderr.flush().map_err(|_| CliError("terminal"))?;
        }
        // SAFETY: the owned PTY descriptor is checked by libc; `saved` lives
        // until restore_echo in the same invocation.
        unsafe {
            let mut attributes: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut attributes) != 0 {
                return Err(CliError("terminal"));
            }
            self.saved = Some(attributes);
            let mut hidden = attributes;
            hidden.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &hidden) != 0 {
                return Err(CliError("terminal"));
            }
        }
        let mut value = String::new();
        std::io::stdin()
            .read_line(&mut value)
            .map_err(|_| CliError("terminal"))?;
        // getpass.unix_getpass writes one newline after hidden input, before
        // the caller's result. Keep the synthetic PTY byte stream identical.
        std::io::stderr()
            .write_all(b"\n")
            .map_err(|_| CliError("terminal"))?;
        Ok(value.trim_end_matches(['\r', '\n']).to_owned())
    }

    fn restore_echo(&mut self) {
        if let Some(attributes) = self.saved.take() {
            // SAFETY: restore the exact attributes read from this PTY.
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &attributes);
            }
        }
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

struct ProcessSecrets;
impl SecretAdapter for ProcessSecrets {
    fn allowed_backends(&self) -> Option<Vec<String>> {
        None
    }
    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
    fn keychain_read(&self, service: &str, user: &str) -> Option<String> {
        Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", user, "-w"])
            .output()
            .ok()
            .filter(|result| result.status.success())
            .and_then(|result| String::from_utf8(result.stdout).ok())
            .map(|value| value.trim().to_owned())
    }
    fn op_read(&self, reference: &str) -> Option<String> {
        Command::new("op")
            .args(["read", reference])
            .output()
            .ok()
            .filter(|result| result.status.success())
            .and_then(|result| String::from_utf8(result.stdout).ok())
            .map(|value| value.trim().to_owned())
    }
    fn keychain_write(&self, service: &str, user: &str, value: &str) -> Result<(), SecretError> {
        let line = [
            "add-generic-password".to_owned(),
            "-U".into(),
            "-s".into(),
            shell_quote(service),
            "-a".into(),
            shell_quote(user),
            "-w".into(),
            shell_quote(value),
        ]
        .join(" ");
        let mut child = Command::new("security")
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SecretError::keychain_write())?;
        child
            .stdin
            .take()
            .ok_or_else(SecretError::keychain_write)?
            .write_all((line + "\n").as_bytes())
            .map_err(|_| SecretError::keychain_write())?;
        if child
            .wait()
            .map_err(|_| SecretError::keychain_write())?
            .success()
        {
            Ok(())
        } else {
            Err(SecretError::keychain_write())
        }
    }
    fn user(&self) -> String {
        ["LOGNAME", "USER", "LNAME", "USERNAME"]
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .unwrap_or_else(|| "synthetic".into())
    }
    fn missing_event(&self, _: &str) {}
}

fn native_cli(args: &[String]) -> Result<String, String> {
    match args {
        [data, edit, name] if data == "data" && edit == "edit" => {
            let root = owned_root()?;
            let key_file = root.parent().ok_or("home")?.join("keys/backup.key");
            let keys = |create: bool| synthetic::key_for_data(&root, &key_file, create);
            let store = Store::new(&root, true, &keys);
            let terminal = ProcessTerminal::new();
            let mut editor = ProcessEditor;
            let editor_command = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
            let outcome = cli::cmd_data_edit(&store, name, &editor_command, &terminal, &mut editor)
                .map_err(|error| error.0.to_string())?;
            Ok(format!("{}\u{1}{}", outcome.code, outcome.stdout))
        }
        [data, show, name] if data == "data" && show == "show" => {
            let root = owned_root()?;
            let key_file = root.parent().ok_or("home")?.join("keys/backup.key");
            let keys = |create: bool| synthetic::key_for_data(&root, &key_file, create);
            let store = Store::new(&root, true, &keys);
            let events = std::env::var("CAREERBREW_EVENTS").ok();
            let outcome = cli::cmd_data_show(
                &store,
                name,
                false,
                std::io::stdout().is_terminal(),
                events.as_deref(),
                &synthetic::FixedEventClock,
            )
            .map_err(|error| error.0.to_string())?;
            Ok(format!("{}\u{1}{}", outcome.code, outcome.stdout))
        }
        [data, show, name, keys] if data == "data" && show == "show" && keys == "--keys" => {
            let root = owned_root()?;
            let key_file = root.parent().ok_or("home")?.join("keys/backup.key");
            let keys = |create: bool| synthetic::key_for_data(&root, &key_file, create);
            let store = Store::new(&root, true, &keys);
            let events = std::env::var("CAREERBREW_EVENTS").ok();
            let outcome = cli::cmd_data_show(
                &store,
                name,
                true,
                std::io::stdout().is_terminal(),
                events.as_deref(),
                &synthetic::FixedEventClock,
            )
            .map_err(|error| error.0.to_string())?;
            Ok(format!("{}\u{1}{}", outcome.code, outcome.stdout))
        }
        [secret, set, service] if secret == "secret" && set == "set" => {
            let mut terminal = ProcessTerminal::new();
            let outcome = cli::cmd_secret_set(service, &mut terminal, &ProcessSecrets)
                .map_err(|error| error.0.to_string())?;
            Ok(format!("{}\u{1}{}", outcome.code, outcome.stdout))
        }
        _ => Err("usage".into()),
    }
}

struct CrashRuntime;
impl Runtime for CrashRuntime {
    fn now(&self) -> String {
        "2026-03-01T09:30:00+00:00".into()
    }
    fn new_id(&self) -> String {
        "a1b2c3d4".into()
    }
    fn emit(&self, _: &str, _: &str) -> Result<(), ReceiptError> {
        std::process::exit(86)
    }
}

fn crash_write(key_file: &Path, root: &Path, file: &Path, stage: &str) -> Result<String, String> {
    let point = match stage {
        "before-temp" => atomic::WritePoint::BeforeTempWrite,
        "after-temp" => atomic::WritePoint::AfterTempWrite,
        "before-rename" => atomic::WritePoint::BeforeRename,
        "after-rename" => atomic::WritePoint::AfterRename,
        _ => return Err("usage".into()),
    };
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|_| "io".to_string())?;
    let key = synthetic::key_for_data(root, key_file, true).map_err(|_| "key".to_string())?;
    atomic::write_text_with(file, Some(&key), true, &text, |at| {
        if at == point {
            std::process::exit(86);
        }
        Ok(())
    })
    .map_err(|_| "write".to_string())?;
    Ok(String::new())
}

fn run(args: &[String]) -> Result<String, String> {
    if args == ["--all"] {
        return run_all();
    }
    if let [flag, trial, case, role] = args {
        if flag == "--careerbrew-data-contract" {
            let owned = data_home()?;
            let fixture = std::env::var_os("CAREERBREW_DATA_FIXTURE").ok_or("data-fixture")?;
            let report = std::env::var_os("CAREERBREW_DATA_REPORT").ok_or("data-report")?;
            data::run_phase(
                &owned,
                Path::new(&fixture),
                Path::new(&report),
                trial,
                case,
                role,
            )?;
            return Ok(String::new());
        }
    }
    if let [op, key_file, root, file, stage] = args {
        if op == "crash-write" {
            return crash_write(Path::new(key_file), Path::new(root), Path::new(file), stage);
        }
    }
    if let [op, key_file, root] = args {
        if op == "crash-receipt" {
            let root = Path::new(root);
            let key_file = Path::new(key_file);
            let keys = |create: bool| synthetic::key_for_data(root, key_file, create);
            let store = Store::new(root, true, &keys);
            Receipts::new(&store, &CrashRuntime)
                .request("Synthetic crash", "", "agent")
                .map_err(|_| "receipt".to_string())?;
            return Ok(String::new());
        }
    }
    if let [op, key_file, root, id] = args {
        if op == "receipt-request" {
            let root = Path::new(root);
            let key_file = Path::new(key_file);
            let keys = |create: bool| synthetic::key_for_data(root, key_file, create);
            let store = Store::new(root, true, &keys);
            let runtime = synthetic::EventRuntime::new(&store, vec![id.clone()]);
            Receipts::new(&store, &runtime)
                .request("Synthetic mixed writer", "", "agent")
                .map_err(|_| "receipt".to_string())?;
            return Ok(String::new());
        }
    }
    if args
        .first()
        .is_some_and(|arg| arg == "data" || arg == "secret")
    {
        return native_cli(args);
    }
    match args {
        [op, key_file, root, file] if op == "read" => {
            let key = key(Path::new(key_file), Path::new(root), false)?;
            let stored = std::fs::read(file).map_err(|_| "io".to_string())?;
            let plain =
                careerbrew_vault::open(Some(&key), &stored).map_err(|_| "vault".to_string())?;
            String::from_utf8(plain).map_err(|_| "utf8".to_string())
        }
        [op, key_file, root, file, mode]
            if (op == "write" || op == "write-if-changed")
                && (mode == "seal" || mode == "plain") =>
        {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|_| "io".to_string())?;
            let seal = mode == "seal";
            let key = if seal {
                Some(key(Path::new(key_file), Path::new(root), true)?)
            } else {
                None
            };
            let path = Path::new(file);
            if op == "write" {
                atomic::write_text(path, key.as_deref(), seal, &text)
                    .map_err(|_| "write".to_string())?;
                Ok(String::new())
            } else {
                let wrote = atomic::write_text_if_changed(path, key.as_deref(), seal, &text)
                    .map_err(|error| match error {
                        StoreError::Utf8 => "utf8".to_string(),
                        _ => "write".to_string(),
                    })?;
                Ok(if wrote { "wrote" } else { "unchanged" }.to_string())
            }
        }
        [op, key_file, root, file, mode]
            if op == "append" && (mode == "seal" || mode == "plain") =>
        {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|_| "io".to_string())?;
            let seal = mode == "seal";
            let key = if seal {
                Some(key(Path::new(key_file), Path::new(root), true)?)
            } else {
                None
            };
            lock::append_jsonl_text(Path::new(file), key.as_deref(), seal, &text)
                .map_err(|_| "append".to_string())?;
            Ok(String::new())
        }
        _ => Err("usage".to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            if let Some((code, text)) = out.split_once('\u{1}') {
                let _ = std::io::stdout().write_all(text.as_bytes());
                return ExitCode::from(code.parse().unwrap_or(1));
            }
            let _ = std::io::stdout().write_all(out.as_bytes());
            ExitCode::SUCCESS
        }
        Err(kind) if kind == "usage" => {
            eprintln!("usage: vault-store --all | read|write|write-if-changed|append <key-file> <data-root> <file> [seal|plain]");
            ExitCode::from(2)
        }
        Err(kind) => {
            eprintln!("refused: {kind}");
            ExitCode::from(1)
        }
    }
}
