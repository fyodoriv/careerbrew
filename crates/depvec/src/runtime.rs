//! Vector set `runtime`: HTTP fixture paths, file locks and subprocess
//! limits, as careerbrew's Python code does them.
//!
//! Ops (each vector names one):
//! - `http-fixture-path`: a URL to the fixture path `connectors._fixture_path`
//!   gives under `CAREERBREW_HTTP_FIXTURES` (`<host>/<path key>.json`);
//! - `http-fixture-read`: `{"url", "files"}` to the text `connectors.http_get`
//!   reads from those synthetic fixture files (`.json`, `.xml`, `.html`, `.csv`
//!   in that order), or an error when none exists;
//! - `lock-probe`: `{"holder", "hold", "probe", "released"}` to
//!   `{"acquired"}`. The holder is this process (`self`), a Python `fcntl.flock`
//!   peer process (`peer`) or nobody (`none`). The probe runs on the other
//!   side, so both directions cross the process and language line;
//! - `run-limited`: `{"program", "cpu", "wall"}` to `{"category",
//!   "exit_code"}`, with the CPU, memory and wall limits of the exercises
//!   runner (`tutor/exercises.py`, `_limits` and `_child`).
//!
//! Locks use the standard library's `File::lock` (flock on Unix); URLs use
//! `http::Uri`; limits use `rlimit` and `wait-timeout`.
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wait_timeout::ChildExt;

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "http-fixture-path" => Ok(Value::String(fixture_path(text(input)?, ".json")?)),
        "http-fixture-read" => fixture_read(input).map(Value::String),
        "lock-probe" => lock_probe(input),
        "run-limited" => run_limited(input),
        _ => Err("unknown op".into()),
    }
}

fn text(input: &Value) -> Result<&str, String> {
    input.as_str().ok_or_else(|| "input is not text".to_owned())
}

fn field<'a>(input: &'a Value, name: &str) -> Result<&'a str, String> {
    input[name]
        .as_str()
        .ok_or_else(|| format!("input has no text field {name}"))
}

static SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// A new empty folder under the temp folder, owned by this run.
fn scratch(label: &str) -> Result<PathBuf, String> {
    let n = SCRATCH.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "careerbrew-depvec-{label}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|_| "cannot make a scratch folder")?;
    Ok(dir)
}

// ---- HTTP fixtures ----

/// `connectors._fixture_path` relative to the fixture root: the URL's netloc,
/// then its path with each run of characters outside `[A-Za-z0-9_.-]` made one
/// `_`, or `root` for an empty path.
fn fixture_path(url: &str, ext: &str) -> Result<String, String> {
    let parsed: http::Uri = url.parse().map_err(|_| "not a URL")?;
    // The authority as written, like `urlsplit(url).netloc`; the fragment is dropped.
    let netloc = parsed.authority().ok_or("URL has no host")?.as_str();
    let mut key = String::new();
    let mut in_run = false;
    for ch in parsed.path().trim_matches('/').chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
            key.push(ch);
            in_run = false;
        } else if !in_run {
            key.push('_');
            in_run = true;
        }
    }
    if key.is_empty() {
        key.push_str("root");
    }
    Ok(format!("{netloc}/{key}{ext}"))
}

fn fixture_read(input: &Value) -> Result<String, String> {
    let url = field(input, "url")?;
    let files = input["files"].as_object().ok_or("input has no files")?;
    let root = scratch("http")?;
    let result = (|| {
        for (name, body) in files {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|_| "cannot write a fixture")?;
            }
            std::fs::write(&path, body.as_str().ok_or("fixture body is not text")?)
                .map_err(|_| "cannot write a fixture")?;
        }
        for ext in [".json", ".xml", ".html", ".csv"] {
            let path = root.join(fixture_path(url, ext)?);
            if path.exists() {
                return std::fs::read_to_string(path).map_err(|_| "cannot read a fixture".into());
            }
        }
        Err("no fixture".to_owned())
    })();
    let _ = std::fs::remove_dir_all(&root);
    result
}

// ---- File locks ----

/// The Python peer: holds or probes a `fcntl.flock` lock on one file.
const PEER: &str = r#"import fcntl, sys
path, mode, action = sys.argv[1:]
flag = {"exclusive": fcntl.LOCK_EX, "shared": fcntl.LOCK_SH}[mode]
with open(path, "a") as fh:
    if action == "probe":
        try:
            fcntl.flock(fh, flag | fcntl.LOCK_NB)
            print("1")
        except BlockingIOError:
            print("0")
    else:
        fcntl.flock(fh, flag)
        print("ready", flush=True)
        sys.stdin.readline()
        fcntl.flock(fh, fcntl.LOCK_UN)
        print("released", flush=True)
        sys.stdin.readline()
"#;

/// The peer's Python: `CAREERBREW_DEPVEC_PYTHON`, else the checkout's
/// `.venv/bin/python` (verify runs the candidate from the repo root).
fn python() -> PathBuf {
    std::env::var_os("CAREERBREW_DEPVEC_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".venv/bin/python"))
}

fn peer(path: &Path, mode: &str, action: &str) -> Result<Child, String> {
    Command::new(python())
        .args(["-I", "-c", PEER])
        .arg(path)
        .args([mode, action])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "cannot start the lock peer".into())
}

fn line(reader: &mut impl BufRead) -> Result<String, String> {
    let mut out = String::new();
    reader
        .read_line(&mut out)
        .map_err(|_| "lock peer did not answer")?;
    Ok(out.trim().to_owned())
}

fn open(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "cannot open the lock file".into())
}

fn mode<'a>(input: &'a Value, name: &str) -> Result<&'a str, String> {
    match field(input, name)? {
        m @ ("exclusive" | "shared") => Ok(m),
        _ => Err("unknown lock mode".into()),
    }
}

fn try_lock(file: &File, mode: &str) -> Result<bool, String> {
    let result = if mode == "exclusive" {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    match result {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(_) => Err("lock error".into()),
    }
}

fn lock_probe(input: &Value) -> Result<Value, String> {
    let holder = field(input, "holder")?;
    let probe = mode(input, "probe")?;
    let released = input["released"].as_bool().ok_or("input has no released")?;
    let dir = scratch("lock")?;
    let path = dir.join("probe.lock");
    let result = (|| -> Result<bool, String> {
        match holder {
            "none" => try_lock(&open(&path)?, probe),
            "self" => {
                let held = open(&path)?;
                if mode(input, "hold")? == "exclusive" {
                    held.lock()
                } else {
                    held.lock_shared()
                }
                .map_err(|_| "lock error")?;
                if released {
                    held.unlock().map_err(|_| "unlock error")?;
                }
                let child = peer(&path, probe, "probe")?;
                let output = child.wait_with_output().map_err(|_| "lock peer failed")?;
                drop(held);
                match String::from_utf8_lossy(&output.stdout).trim() {
                    "1" => Ok(true),
                    "0" => Ok(false),
                    _ => Err("lock peer gave no answer".into()),
                }
            }
            "peer" => {
                let mut child = peer(&path, mode(input, "hold")?, "hold")?;
                let mut stdin = child.stdin.take().ok_or("no peer stdin")?;
                let mut out = BufReader::new(child.stdout.take().ok_or("no peer stdout")?);
                let answer = (|| {
                    if line(&mut out)? != "ready" {
                        return Err("lock peer is not ready".to_owned());
                    }
                    if released {
                        stdin.write_all(b"\n").map_err(|_| "lock peer write")?;
                        if line(&mut out)? != "released" {
                            return Err("lock peer did not release".to_owned());
                        }
                    }
                    try_lock(&open(&path)?, probe)
                })();
                drop(stdin);
                let _ = child.wait();
                answer
            }
            _ => Err("unknown holder".into()),
        }
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result.map(|acquired| json!({ "acquired": acquired }))
}

// ---- Subprocess limits ----

/// `exercises.MEMORY_BYTES`.
const MEMORY_BYTES: u64 = 768 * 1024 * 1024;

fn program(name: &str) -> Result<&'static str, String> {
    Ok(match name {
        "ok" => "exit 0",
        "exit3" => "exit 3",
        "spin" => "while :; do :; done",
        "sleep" => "exec /bin/sleep 30",
        _ => return Err("unknown program".into()),
    })
}

fn run_limited(input: &Value) -> Result<Value, String> {
    let script = program(field(input, "program")?)?;
    let cpu = input["cpu"].as_u64().filter(|n| *n > 0).ok_or("bad cpu")?;
    let wall = input["wall"]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.0)
        .ok_or("bad wall")?;
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", script])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: the closure runs in the child between fork and exec and only
    // calls setrlimit, which is async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            rlimit::setrlimit(rlimit::Resource::CPU, cpu, cpu)?;
            // macOS refuses RLIMIT_AS, as in `exercises._limits`; the wall limit still holds.
            let _ = rlimit::setrlimit(rlimit::Resource::AS, MEMORY_BYTES, MEMORY_BYTES);
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|_| "cannot start the program")?;
    let status = match child
        .wait_timeout(Duration::from_secs_f64(wall))
        .map_err(|_| "wait failed")?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(json!({"category": "wall_timeout", "exit_code": null}));
        }
    };
    // `exercises._child`: signals 9 and 24, or exit 152, are the CPU limit.
    let (category, exit_code) = match (status.code(), status.signal()) {
        (_, Some(9 | 24)) | (Some(152), _) => ("cpu_limit", status.code()),
        (Some(0), _) => ("ok", Some(0)),
        (Some(code), _) => ("nonzero_exit", Some(code)),
        (None, _) => ("signal_exit", None),
    };
    Ok(json!({"category": category, "exit_code": exit_code}))
}
