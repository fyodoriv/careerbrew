//! rust-26 end-to-end parity over every registry command.
//!
//! For each action in the generated registry:
//! - it has exactly one line in `config/native-routes.txt`;
//! - a `native` route is either `actions` (rust-03, `cli.rs` and US-111) or
//!   one of `routed::COMMANDS`, and has scenarios in `CASES`;
//! - in each scenario's fresh synthetic home, the console script with the
//!   committed routes, and the native binary run directly (when the scenario
//!   says it serves), give the same stdout, exit code, data-root files and
//!   content-free event rows as the console script with `CAREERBREW_NATIVE=off`.
//!
//! The Python side is the repo's own `.venv` interpreter running
//! `python -m careerbrew`. Telemetry sends stay off (`DO_NOT_TRACK=1`): the
//! native binary never sends (docs/decisions/0010-native-pilot-hooks.md).
use careerbrew_cli::registry::Registry;
use careerbrew_cli::routed::COMMANDS;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_careerbrew-native");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn routes() -> Vec<(String, String)> {
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
struct Scenario {
    id: &'static str,
    cli: &'static [&'static [&'static str]],
    files: &'static [(&'static str, &'static str)],
    /// The routed commands the native binary must serve itself here. Any
    /// other routed command may refuse (exit 64); the console script must
    /// then print Python's result.
    serves: &'static [&'static str],
    /// The native binary must refuse every routed command here.
    must_refuse: bool,
    /// Set `CAREERBREW_KEY_FILE` with a doubled slash, which Python's
    /// `pathlib` folds when it prints the key path.
    odd_key_path: bool,
}

const CONNECTORS: &str =
    "connectors:\n- kind: github\n  token: env:SYNTHETIC_GH\n- kind: jira\n  token: keychain:synthetic\n";

/// The routed commands that read only the data root (no backup).
const READERS: &[&str] = &[
    "machine.show",
    "plugin.list",
    "plugin.verify",
    "connectors.list",
    "data.status",
];

const ALL: &[&str] = &[
    "machine.show",
    "plugin.list",
    "plugin.verify",
    "connectors.list",
    "data.status",
    "backup.verify",
];

const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "empty-home",
        cli: &[],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "personal-labelled",
        cli: &[&["machine", "set", "personal", "--label", "desk-1"]],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "connectors-plaintext",
        cli: &[],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "connectors-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "backup-made",
        cli: &[
            &["machine", "set", "personal"],
            &["data", "encrypt"],
            &["backup", "create"],
        ],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: ALL,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "plugin-installed",
        cli: &[&["plugin", "add", "tests/fixtures/plugin-acme"]],
        files: &[],
        serves: &[],
        must_refuse: true,
        odd_key_path: false,
    },
    Scenario {
        id: "redaction-input",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
        serves: &[],
        must_refuse: true,
        odd_key_path: false,
    },
    Scenario {
        id: "malformed-machine",
        cli: &[],
        files: &[("machine.yaml", "role: [\n")],
        serves: &[],
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "odd-key-path",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
        serves: &["machine.show", "connectors.list"],
        must_refuse: false,
        odd_key_path: true,
    },
];

/// One run's observable result.
#[derive(Debug, PartialEq)]
struct Observed {
    code: Option<i32>,
    stdout: String,
    files: Vec<String>,
    events: Vec<String>,
}

enum Side {
    Python,
    Console,
    Native,
}

struct Home {
    _dir: tempfile::TempDir,
    home: PathBuf,
    data: PathBuf,
    key: String,
}

fn base(cmd: &mut Command, home: &Home) {
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
}

fn python(home: &Home, args: &[&str]) -> Command {
    let mut cmd = Command::new(repo().join(".venv/bin/python"));
    cmd.args(["-m", "careerbrew"]).args(args);
    base(&mut cmd, home);
    cmd
}

/// Builds the scenario's home once under `<dir>/live`, then keeps a copy in
/// `<dir>/snap`: every run restores it to the same paths, so each side sees
/// the same bytes and prints the same paths (a key path, a backup name).
fn prepare(scenario: &Scenario) -> Home {
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
fn copy_tree(from: &Path, to: &Path) {
    let done = Command::new("/bin/cp")
        .arg("-Rp")
        .arg(from)
        .arg(to)
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
}

fn restore(home: &Home) {
    let live = home.home.parent().unwrap();
    std::fs::remove_dir_all(live).unwrap();
    copy_tree(&live.with_file_name("snap"), live);
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
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
                out.push(format!("{rel} {}", bytes.len()));
            }
        }
    }
}

/// An event row without its clock fields (`ts`, `ms`).
fn normalize(row: &str) -> String {
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

fn observe(home: &Home, args: &[&str], side: &Side) -> Observed {
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

fn argv(name: &str) -> Vec<&str> {
    name.split('.').collect()
}

#[test]
fn every_registry_action_has_one_route_line() {
    let reg = Registry::load().unwrap();
    let names: Vec<String> = reg
        .rows
        .iter()
        .map(|row| row.get("name").and_then(|n| n.as_str()).unwrap().to_owned())
        .collect();
    let routed: Vec<String> = routes().into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, routed, "run: uv run python scripts/gen-registry.py");
}

#[test]
fn every_native_route_has_a_native_handler() {
    for (name, route) in routes() {
        assert!(route == "native" || route == "python", "{name}: {route}");
        if route == "native" {
            assert!(
                name == "actions" || COMMANDS.contains(&name.as_str()),
                "{name} is routed native but careerbrew-native has no handler for it"
            );
        }
    }
    for name in COMMANDS {
        assert!(
            routes().iter().any(|(n, _)| n == name),
            "{name} is not a registry action"
        );
    }
}

#[test]
fn every_native_routed_command_matches_python_in_every_scenario() {
    let native: Vec<String> = routes()
        .into_iter()
        .filter(|(name, route)| route == "native" && name != "actions")
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        native.len(),
        COMMANDS.len(),
        "every routed handler is switched on"
    );
    let mut cases = 0;
    for scenario in SCENARIOS {
        let home = prepare(scenario);
        for name in &native {
            let args = argv(name);
            let want = observe(&home, &args, &Side::Python);
            let direct = observe(&home, &args, &Side::Native);
            let refused = direct.code == Some(64);
            if refused {
                assert!(
                    !scenario.serves.contains(&name.as_str()),
                    "{name} refused in {}, which it must serve",
                    scenario.id
                );
                assert!(direct.stdout.is_empty(), "{name} in {}", scenario.id);
                assert!(direct.events.is_empty(), "{name} in {}", scenario.id);
            } else {
                assert_eq!(
                    direct, want,
                    "{name} in careerbrew-native in {}",
                    scenario.id
                );
            }
            if scenario.must_refuse {
                assert!(refused, "{name} must refuse in {}", scenario.id);
            }
            let mut console = observe(&home, &args, &Side::Console);
            if refused {
                // A refusal is logged once, content-free, before Python runs.
                let refusal = console
                    .events
                    .iter()
                    .position(|row| row.contains("cli.native_unsupported"))
                    .unwrap_or_else(|| panic!("{name}: no refusal row in {}", scenario.id));
                console.events.remove(refusal);
            }
            assert_eq!(
                console, want,
                "{name} via the console script in {}",
                scenario.id
            );
            cases += 1;
        }
    }
    assert_eq!(cases, SCENARIOS.len() * COMMANDS.len());
}

fn scenario(id: &str) -> &'static Scenario {
    SCENARIOS.iter().find(|s| s.id == id).unwrap()
}

/// Negative controls: the comparison sees a real difference, and the
/// refusal rules fire. The corporate and personal homes print different
/// `machine show` lines; a flag the native handler does not take, and a key
/// path `pathlib` would fold, are refused, not served.
#[test]
fn the_parity_comparison_detects_a_planted_difference() {
    let personal = observe(
        &prepare(scenario("personal-labelled")),
        &["machine", "show"],
        &Side::Native,
    );
    let corporate = observe(
        &prepare(scenario("corporate")),
        &["machine", "show"],
        &Side::Python,
    );
    assert_ne!(personal, corporate);
    assert!(personal.stdout.starts_with("role: personal label: desk-1"));
    assert!(corporate.stdout.starts_with("role: corporate"));
    let empty = prepare(scenario("empty-home"));
    let extra = observe(&empty, &["machine", "show", "--json"], &Side::Native);
    assert_eq!(extra.code, Some(64));
    let odd = prepare(scenario("odd-key-path"));
    assert_eq!(
        observe(&odd, &["data", "status"], &Side::Native).code,
        Some(64)
    );
    let python = observe(&odd, &["data", "status"], &Side::Python);
    assert!(python.stdout.contains("/backup.key"));
    assert!(!python.stdout.contains("//backup.key"));
}
