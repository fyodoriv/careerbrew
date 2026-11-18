//! Controls for the rust-03 pilot binary: byte parity with the frozen
//! actions reference, the argparse error text, the exit-64 refusals, the
//! event row, and the Python-compatible JSON writer.
use careerbrew_cli::registry::{parse, Json};
use careerbrew_cli::render::{py_dumps, py_string};
use std::path::Path;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_careerbrew-native");
const REFERENCE: &str =
    include_str!("../../../xtask/tests/fixtures/capture/actions-reference-telemetry-v4.json");
const ROOT_USAGE_HEAD: &str =
    "usage: careerbrew [-h] [--no-watch] [--no-web] [--no-refresh] [--version]\n";

/// Runs the binary in a fresh home with events off and `COLUMNS=80`.
fn native(home: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .env("CAREERBREW_HOME", home)
        .env("CAREERBREW_EVENTS", "off")
        .env("COLUMNS", "80")
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path.strip_prefix(dir).unwrap().display().to_string());
        }
    }
    out
}

#[test]
fn actions_json_is_byte_equal_to_the_frozen_v4_reference() {
    let home = tempfile::tempdir().unwrap();
    let result = native(home.path(), &["actions", "--json"]);
    assert_eq!(result.status.code(), Some(0));
    assert!(result.stderr.is_empty());
    assert_eq!(text(&result.stdout), REFERENCE);
    let rows = parse(REFERENCE).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 240);
    // Events off: nothing is written.
    assert!(files(home.path()).is_empty());
}

#[test]
fn a_name_selects_the_action_and_its_children() {
    let home = tempfile::tempdir().unwrap();
    let all = parse(REFERENCE).unwrap();
    for name in ["day", "config.set", "nosuch"] {
        let result = native(home.path(), &["actions", name, "--json"]);
        let rows = parse(text(&result.stdout)).unwrap();
        let expected: Vec<&Json> = all
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                let own = row.get("name").unwrap().as_str().unwrap();
                own == name || own.starts_with(&format!("{name}."))
            })
            .collect();
        assert_eq!(rows.as_array().unwrap().len(), expected.len(), "{name}");
    }
    let text_result = native(home.path(), &["actions", "nosuch"]);
    assert_eq!(
        text(&text_result.stdout),
        "0 action(s); the web app runs them in Actions; details: careerbrew actions --json\n"
    );
}

#[test]
fn the_text_listing_ends_with_the_count_line() {
    let home = tempfile::tempdir().unwrap();
    let result = native(home.path(), &["actions"]);
    assert_eq!(result.status.code(), Some(0));
    let out = text(&result.stdout);
    assert_eq!(out.lines().count(), 241);
    assert!(out.ends_with(
        "\n240 action(s); the web app runs them in Actions; details: careerbrew actions --json\n"
    ));
}

#[test]
fn an_unknown_flag_prints_argparses_usage_error() {
    let home = tempfile::tempdir().unwrap();
    for (args, unknown) in [
        (&["actions", "--bogus"][..], "--bogus"),
        (&["actions", "a", "b"][..], "b"),
        (&["actions", "-x", "a", "b", "--json"][..], "-x b"),
    ] {
        let result = native(home.path(), args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(result.stdout.is_empty());
        let err = text(&result.stderr);
        assert!(err.starts_with(ROOT_USAGE_HEAD), "{err}");
        assert!(
            err.ends_with(&format!(
                "                  ...\ncareerbrew: error: unrecognized arguments: {unknown}\n"
            )),
            "{err}"
        );
    }
}

#[test]
fn help_prints_the_stored_page_and_wins_over_unknown_arguments() {
    let home = tempfile::tempdir().unwrap();
    let root = native(home.path(), &["--help"]);
    assert_eq!(root.status.code(), Some(0));
    assert!(text(&root.stdout).starts_with(ROOT_USAGE_HEAD));
    assert_eq!(native(home.path(), &["-h"]).stdout, root.stdout);
    let page = native(home.path(), &["actions", "--help"]);
    assert!(text(&page.stdout).starts_with("usage: careerbrew actions [-h] [--json] [name]\n"));
    assert_eq!(
        native(home.path(), &["actions", "--bogus", "-h"]).stdout,
        page.stdout
    );
    let leaf = native(home.path(), &["config", "set", "--help"]);
    assert_eq!(leaf.status.code(), Some(0));
    assert!(text(&leaf.stdout).starts_with("usage: careerbrew config set"));
}

#[test]
fn argv_outside_the_pilot_exits_64() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        &[][..],
        &["config", "show"][..],
        &["--no-watch", "actions"][..],
        &["actions", "--"][..],
        &["actions", "--json=1"][..],
        &["actions", "-hx"][..],
        &["nosuch", "--help"][..],
    ] {
        let result = native(home.path(), args);
        assert_eq!(result.status.code(), Some(64), "{args:?}");
        assert!(result.stdout.is_empty());
        assert!(text(&result.stderr).starts_with("native: unsupported "));
    }
    assert!(files(home.path()).is_empty());
}

#[test]
fn another_help_width_goes_to_python() {
    let home = tempfile::tempdir().unwrap();
    for args in [&["--help"][..], &["actions", "--bogus"][..]] {
        let result = Command::new(BIN)
            .args(args)
            .env_clear()
            .env("CAREERBREW_HOME", home.path())
            .env("CAREERBREW_EVENTS", "off")
            .env("COLUMNS", "120")
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(64), "{args:?}");
    }
}

#[test]
fn the_text_listing_goes_to_python_when_redaction_has_inputs() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("profile.yaml"), "").unwrap();
    assert_eq!(native(home.path(), &["actions"]).status.code(), Some(64));
    // --json is printed without redaction in Python too.
    assert_eq!(
        native(home.path(), &["actions", "--json"]).status.code(),
        Some(0)
    );
}

#[test]
fn events_on_appends_one_content_free_row() {
    let home = tempfile::tempdir().unwrap();
    let result = Command::new(BIN)
        .args(["actions", "--json"])
        .env_clear()
        .env("CAREERBREW_HOME", home.path())
        .env("COLUMNS", "80")
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(files(home.path()), ["logs/events.jsonl"]);
    let log = std::fs::read_to_string(home.path().join("logs/events.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    let row = parse(log.trim_end()).unwrap();
    let Json::Object(pairs) = &row else {
        panic!("{log}")
    };
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["command", "event", "ms", "status", "ts", "v", "via"]);
    assert_eq!(row.get("command").unwrap().as_str(), Some("actions"));
    assert_eq!(row.get("event").unwrap().as_str(), Some("cli.run"));
    assert_eq!(row.get("status").unwrap().as_str(), Some("ok"));
    assert_eq!(row.get("via").unwrap().as_str(), Some("agent"));
    // An argparse error or a help page writes nothing, as in Python.
    let quiet = tempfile::tempdir().unwrap();
    for args in [&["actions", "--bogus"][..], &["--help"][..]] {
        Command::new(BIN)
            .args(args)
            .env_clear()
            .env("CAREERBREW_HOME", quiet.path())
            .env("COLUMNS", "80")
            .output()
            .unwrap();
    }
    assert!(files(quiet.path()).is_empty());
}

#[test]
fn the_event_stamp_is_utc_seconds() {
    use careerbrew_cli::events::stamp;
    assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
    assert_eq!(stamp(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(stamp(1_763_506_799), "2025-11-18T22:59:59Z");
}

/// Known `json.dumps(s)` outputs from CPython 3.12 (`ensure_ascii=True`).
#[test]
fn the_string_writer_matches_python_samples() {
    for (value, python) in [
        ("plain", r#""plain""#),
        ("q\"b\\s/", r#""q\"b\\s/""#),
        ("\n\r\t\u{8}\u{c}", r#""\n\r\t\b\f""#),
        ("\u{0}\u{1f}\u{7f}", r#""\u0000\u001f\u007f""#),
        ("caf\u{e9} \u{2014}", r#""café —""#),
        ("\u{1f600}", r#""😀""#),
        ("\u{ffff}\u{10000}", r#""￿𐀀""#),
    ] {
        let mut out = String::new();
        py_string(value, &mut out);
        assert_eq!(out, python, "{value:?}");
    }
}

/// A small deterministic generator (xorshift64*): no extra dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn char(&mut self) -> char {
        let pools: [(u32, u32); 5] = [
            (0, 0x80),
            (0x80, 0x800),
            (0x800, 0xd800),
            (0xe000, 0x10000),
            (0x10000, 0x110000),
        ];
        let (low, high) = pools[(self.next() % 5) as usize];
        char::from_u32(low + (self.next() % u64::from(high - low)) as u32).unwrap_or('x')
    }

    fn string(&mut self) -> String {
        (0..self.next() % 12).map(|_| self.char()).collect()
    }

    fn value(&mut self, depth: u32) -> Json {
        match (self.next() % if depth > 2 { 4 } else { 6 }, self.next() % 3) {
            (0, _) => Json::Null,
            (1, n) => Json::Bool(n == 0),
            (2, n) => Json::Number(["0", "-1.5", "1700000000.0001"][n as usize].to_owned()),
            (3, _) => Json::Str(self.string()),
            (4, _) => Json::Array(
                (0..self.next() % 4)
                    .map(|_| self.value(depth + 1))
                    .collect(),
            ),
            _ => Json::Object(
                (0..self.next() % 4)
                    .map(|_| (self.string(), self.value(depth + 1)))
                    .collect(),
            ),
        }
    }
}

/// Property: for any value, the writer's output is ASCII, parses back to
/// the same value (order and number text kept), and is stable.
#[test]
fn the_json_writer_round_trips_random_values() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..2000 {
        let value = rng.value(0);
        let dumped = py_dumps(&value);
        assert!(dumped.is_ascii(), "{dumped}");
        let back = parse(&dumped).unwrap();
        assert_eq!(back, value);
        assert_eq!(py_dumps(&back), dumped);
    }
}

/// Property on the real data: parsing and re-writing the committed registry
/// rows reproduces Python's own `json.dumps(rows, indent=1)` bytes.
#[test]
fn the_writer_reproduces_the_reference_bytes() {
    let rows = parse(REFERENCE).unwrap();
    assert_eq!(format!("{}\n", py_dumps(&rows)), REFERENCE);
}
