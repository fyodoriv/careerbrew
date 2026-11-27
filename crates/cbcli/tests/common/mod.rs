//! The parity harness every native CLI test shares (`parity.rs` and each
//! `step_<module>.rs`): fresh synthetic homes built by the Python CLI, and
//! one observation (stdout, exit code, data-root files, event rows) per side.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::process::Command;

pub const BIN: &str = env!("CARGO_BIN_EXE_careerbrew-native");

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

pub fn routes() -> Vec<(String, String)> {
    std::fs::read_to_string(repo().join("config/native-routes.txt"))
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap().to_owned();
            let route = parts.next().unwrap().to_owned();
            assert!(parts.next().is_none(), "bad route line {line:?}");
            (name, route)
        })
        .collect()
}

/// How a scenario's home is prepared before the command runs: Python CLI
/// argv lists (run with `CAREERBREW_NATIVE=off`) and files to write.
pub struct Scenario {
    pub id: &'static str,
    pub cli: &'static [&'static [&'static str]],
    pub files: &'static [(&'static str, &'static str)],
    /// The routed commands the native binary must serve itself here. Any
    /// other routed command may refuse (exit 64); the console script must
    /// then print Python's result.
    pub serves: &'static [&'static str],
    /// The native binary must refuse every routed command here.
    pub must_refuse: bool,
    /// Set `CAREERBREW_KEY_FILE` with a doubled slash, which Python's
    /// `pathlib` folds when it prints the key path.
    pub odd_key_path: bool,
}

/// One run's observable result.
#[derive(Debug, PartialEq)]
pub struct Observed {
    pub code: Option<i32>,
    pub stdout: String,
    pub files: Vec<String>,
    pub events: Vec<String>,
}

pub enum Side {
    Python,
    Console,
    Native,
}

pub struct Home {
    pub _dir: tempfile::TempDir,
    pub home: PathBuf,
    pub data: PathBuf,
    pub key: String,
}

pub fn base(cmd: &mut Command, home: &Home) {
    cmd.env_clear()
        .current_dir(repo())
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home.home)
        .env("CAREERBREW_HOME", &home.data)
        .env("CAREERBREW_KEY_FILE", &home.key)
        .env("CAREERBREW_EVENTS", "on")
        .env("DO_NOT_TRACK", "1")
        .env("COLUMNS", "80")
        .env("LINES", "24");
    if let Some(now) = pinned_now() {
        cmd.env("CAREERBREW_NOW", now);
    }
}

/// Step tests that compare written files pin the clock for both sides with
/// `CAREERBREW_TEST_NOW` (set it before any home is prepared).
pub fn pinned_now() -> Option<String> {
    std::env::var("CAREERBREW_TEST_NOW").ok()
}

/// One data-root file as compared: its path and size, plus a content digest
/// when `CAREERBREW_TEST_CONTENT=1` and the bytes are not a Fernet token
/// (whose IV makes every encryption differ).
pub fn file_line(rel: &str, bytes: &[u8]) -> String {
    let content = std::env::var("CAREERBREW_TEST_CONTENT").as_deref() == Ok("1");
    if !content || bytes.starts_with(b"gAAAAA") {
        return format!("{rel} {}", bytes.len());
    }
    // FNV-1a 64: a change detector, not a security hash.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{rel} {} {hash:016x}", bytes.len())
}

/// Turns on content digests and pins the clock for every run in this test
/// binary. Call it first in each test of a step file.
pub fn strict(now: &str) {
    std::env::set_var("CAREERBREW_TEST_CONTENT", "1");
    std::env::set_var("CAREERBREW_TEST_NOW", now);
}

/// A step scenario: Python CLI setup commands and files, then argv forms.
pub struct Setup {
    pub id: &'static str,
    pub cli: &'static [&'static [&'static str]],
    pub files: &'static [(&'static str, &'static str)],
}

/// Builds a home for a step scenario (no refusal rules: `Setup` has none).
pub fn prepare_setup(setup: &Setup) -> Home {
    prepare(&Scenario {
        id: setup.id,
        cli: setup.cli,
        files: setup.files,
        serves: &[],
        must_refuse: false,
        odd_key_path: false,
    })
}

/// Checks one argv in a prepared home: the native binary must serve it
/// (when `must_serve`) and match Python, and the console script with the
/// committed routes must match Python too. Returns true when it served.
pub fn assert_same(home: &Home, args: &[&str], must_serve: bool, label: &str) -> bool {
    let want = observe(home, args, &Side::Python);
    let direct = observe(home, args, &Side::Native);
    let served = direct.code != Some(64);
    if must_serve {
        assert!(served, "{label}: {args:?} refused natively");
    }
    if served {
        assert_eq!(direct, want, "{label}: {args:?} natively");
    } else {
        assert!(
            direct.stdout.is_empty() && direct.events.is_empty(),
            "{label}: {args:?}"
        );
    }
    let mut console = observe(home, args, &Side::Console);
    if !served {
        if let Some(at) = console
            .events
            .iter()
            .position(|row| row.contains("cli.native_unsupported"))
        {
            console.events.remove(at);
        }
    }
    assert_eq!(console, want, "{label}: {args:?} via the console script");
    served
}

pub fn python(home: &Home, args: &[&str]) -> Command {
    let mut cmd = Command::new(repo().join(".venv/bin/python"));
    cmd.args(["-m", "careerbrew"]).args(args);
    base(&mut cmd, home);
    cmd
}

/// Builds the scenario's home once under `<dir>/live`, then keeps a copy in
/// `<dir>/snap`: every run restores it to the same paths, so each side sees
/// the same bytes and prints the same paths (a key path, a backup name).
pub fn prepare(scenario: &Scenario) -> Home {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live");
    let sep = if scenario.odd_key_path { "//" } else { "/" };
    let home = Home {
        home: live.join("home"),
        data: live.join("data"),
        key: format!("{}{sep}backup.key", live.join("home").display()),
        _dir: dir,
    };
    std::fs::create_dir_all(&home.home).unwrap();
    for (name, text) in scenario.files {
        let path = home.data.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    for args in scenario.cli {
        let done = python(&home, args)
            .env("CAREERBREW_NATIVE", "off")
            .output()
            .unwrap();
        assert!(
            done.status.success(),
            "{}: setup {args:?}: {done:?}",
            scenario.id
        );
    }
    let log = home.data.join("logs/events.jsonl");
    let _ = std::fs::remove_file(log);
    let live = home.home.parent().unwrap();
    let snap = live.with_file_name("snap");
    copy_tree(live, &snap);
    home
}

/// `cp -Rp`: keeps the key file's mode, which the vault checks.
pub fn copy_tree(from: &Path, to: &Path) {
    let done = Command::new("/bin/cp")
        .arg("-Rp")
        .arg(from)
        .arg(to)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
}

pub fn restore(home: &Home) {
    let live = home.home.parent().unwrap();
    std::fs::remove_dir_all(live).unwrap();
    copy_tree(&live.with_file_name("snap"), live);
}

pub fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(root, &path, out);
        } else {
            let bytes = std::fs::read(&path).unwrap_or_default();
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            if rel != "logs/events.jsonl" {
                out.push(file_line(&rel, &bytes));
            }
        }
    }
}

/// An event row without its clock fields (`ts`, `ms`).
pub fn normalize(row: &str) -> String {
    let mut out = String::new();
    for part in row
        .trim_end()
        .trim_matches(|c| c == '{' || c == '}')
        .split(", \"")
    {
        let part = part.trim_start_matches('"');
        if part.starts_with("ts\"") || part.starts_with("ms\"") {
            continue;
        }
        out.push_str(part);
        out.push(';');
    }
    out
}

pub fn observe(home: &Home, args: &[&str], side: &Side) -> Observed {
    restore(home);
    let output = match side {
        Side::Python => python(home, args)
            .env("CAREERBREW_NATIVE", "off")
            .output()
            .unwrap(),
        Side::Console => python(home, args)
            .env("CAREERBREW_NATIVE_BIN", BIN)
            .output()
            .unwrap(),
        Side::Native => {
            let mut cmd = Command::new(BIN);
            cmd.args(args);
            base(&mut cmd, home);
            cmd.output().unwrap()
        }
    };
    let mut files = Vec::new();
    walk(&home.data, &home.data, &mut files);
    files.sort();
    let events = std::fs::read_to_string(home.data.join("logs/events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(normalize)
        .collect();
    Observed {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        files,
        events,
    }
}

pub fn argv(name: &str) -> Vec<&str> {
    name.split('.').collect()
}
