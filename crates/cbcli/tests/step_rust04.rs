//! rust-04 native handlers (`src/steps/rust04.rs`): parity with the Python
//! CLI in fresh synthetic homes for "draft.route", "draft.judge" and
//! "privacy.scan" (the `--file` draft form), plus the cases the native
//! binary must refuse and negative controls.
mod common;

use common::*;

const NOW: &str = "2026-11-20T09:00:00";

/// Two synthetic support channels and one open thread.
const DIRECTORY: &str = r#"{"channels": [
  {"id": "C100", "owns": ["database", "deploy access"], "members": ["U1", "alex"]},
  {"id": "C200", "owns": ["network"], "members": ["U2"]}
],
"threads": [{"channel": "C100", "url": "t-0001", "text": "Need database access for the synthetic team"}]}
"#;

/// The same channels with no `threads` key (`setdefault` adds it).
const NO_THREADS: &str =
    r#"{"channels": [{"id": "C100", "owns": ["database"], "members": ["U1"]}]}"#;

/// A thread whose url is not a string: `--json` prints it as JSON.
const ODD_URL: &str = r#"{"channels": [{"id": "C100", "owns": ["database"]}], "threads": [{"channel": "C100", "url": 42, "text": "database"}]}"#;

const FILES: &[(&str, &str)] = &[
    ("inputs/directory.json", DIRECTORY),
    ("inputs/no-threads.json", NO_THREADS),
    ("inputs/odd-url.json", ODD_URL),
    ("inputs/not-a-list.json", r#"{"channels": {"id": "C100"}}"#),
    (
        "inputs/duplicate-id.json",
        r#"{"channels": [{"id": "C1"}, {"id": "C1"}]}"#,
    ),
    (
        "inputs/bad-members.json",
        r#"{"channels": [{"id": "C1", "members": ["U1", 2]}]}"#,
    ),
    ("inputs/a-list.json", "[1, 2]"),
    // Python reads these differently or not at all: refused.
    ("inputs/invalid.json", "{\"channels\": [\n"),
    (
        "inputs/float.json",
        r#"{"channels": [], "threads": [{"url": 1.5}]}"#,
    ),
    (
        "inputs/twice.json",
        r#"{"channels": [], "channels": [{"id": "C1"}]}"#,
    ),
    (
        "inputs/nan.json",
        r#"{"channels": [], "threads": [{"url": NaN}]}"#,
    ),
    (
        "inputs/ask-link.txt",
        "I need access to the database, permission denied\n",
    ),
    (
        "inputs/ask-open.txt",
        "Request: grant network access please\n",
    ),
    ("inputs/ask-none.txt", "Lunch at noon?\n"),
    (
        "inputs/ask-unknown.txt",
        "I need access to the billing role\n",
    ),
    (
        "inputs/ask-cr.txt",
        "I need access to the database\r\npermission denied\r\n",
    ),
    (
        "inputs/draft-ok.txt",
        "Hi <@U1>, requesting database access\n",
    ),
    ("inputs/draft-name.txt", "cc @sam we need database access\n"),
    (
        "inputs/draft-member.txt",
        "cc @alex we need\tdatabase access\n",
    ),
    ("inputs/draft-off.txt", "Weekly synthetic notes\n"),
];

fn path(home: &Home, name: &str) -> String {
    format!("{}/inputs/{name}", home.data.display())
}

fn home() -> Home {
    prepare_setup(&Setup {
        id: "draft-inputs",
        cli: &[],
        files: FILES,
    })
}

fn same(home: &Home, args: &[String], must_serve: bool, label: &str) -> bool {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_same(home, &args, must_serve, label)
}

fn route(home: &Home, ask: &str, directory: &str, extra: &[&str]) -> Vec<String> {
    let mut argv = vec![
        "draft".to_owned(),
        "route".to_owned(),
        "--ask".to_owned(),
        path(home, ask),
        "--directory".to_owned(),
        path(home, directory),
    ];
    argv.extend(extra.iter().map(|s| (*s).to_owned()));
    argv
}

fn judge(home: &Home, draft: &str, channel: &str, directory: &str, extra: &[&str]) -> Vec<String> {
    let mut argv = vec![
        "draft".to_owned(),
        "judge".to_owned(),
        "--draft".to_owned(),
        path(home, draft),
        "--channel".to_owned(),
        channel.to_owned(),
        "--directory".to_owned(),
        path(home, directory),
    ];
    argv.extend(extra.iter().map(|s| (*s).to_owned()));
    argv
}

#[test]
fn draft_route_matches_python() {
    strict(NOW);
    let home = home();
    for ask in [
        "ask-link.txt",
        "ask-open.txt",
        "ask-none.txt",
        "ask-unknown.txt",
        "missing.txt",
    ] {
        for extra in [
            &[][..],
            &["--json"],
            &["--teammate", "U1"],
            &["--teammate", "U9", "--json"],
        ] {
            let args = route(&home, ask, "directory.json", extra);
            same(&home, &args, true, "draft.route");
        }
    }
    // Directory shapes: `setdefault`, a JSON url, every `AccessError`, and
    // a missing file.
    for directory in [
        "no-threads.json",
        "odd-url.json",
        "not-a-list.json",
        "duplicate-id.json",
        "bad-members.json",
        "a-list.json",
        "missing.json",
    ] {
        let args = route(&home, "ask-link.txt", directory, &["--json"]);
        same(&home, &args, true, "draft.route");
    }
    let args = route(&home, "ask-link.txt", "no-threads.json", &[]);
    same(&home, &args, true, "draft.route");
}

#[test]
fn draft_judge_matches_python() {
    strict(NOW);
    let home = home();
    let cases: [(&str, &str, &[&str]); 9] = [
        ("draft-ok.txt", "C100", &[]),
        ("draft-ok.txt", "C200", &[]),
        ("draft-ok.txt", "C999", &[]),
        ("draft-ok.txt", "C100", &["--new-thread"]),
        ("draft-name.txt", "C100", &[]),
        ("draft-member.txt", "C100", &[]),
        ("draft-off.txt", "C100", &["--new-thread"]),
        ("missing.txt", "C100", &[]),
        ("draft-member.txt", "C200", &["--new-thread"]),
    ];
    for (draft, channel, extra) in cases {
        let args = judge(&home, draft, channel, "directory.json", extra);
        same(&home, &args, true, "draft.judge");
    }
    for directory in ["no-threads.json", "duplicate-id.json", "missing.json"] {
        let args = judge(&home, "draft-ok.txt", "C100", directory, &["--new-thread"]);
        same(&home, &args, true, "draft.judge");
    }
}

#[test]
fn draft_commands_refuse_what_they_cannot_match() {
    strict(NOW);
    let home = home();
    let redaction = prepare_setup(&Setup {
        id: "redaction-input",
        cli: &[],
        files: &[
            ("inputs/directory.json", DIRECTORY),
            (
                "inputs/ask-link.txt",
                "I need access to the database, permission denied\n",
            ),
            ("profile.yaml", "name: Synthetic Person\n"),
        ],
    });
    let mut cases = Vec::new();
    for directory in ["invalid.json", "float.json", "twice.json", "nan.json"] {
        cases.push(route(&home, "ask-link.txt", directory, &["--json"]));
        cases.push(judge(&home, "draft-ok.txt", "C100", directory, &[]));
    }
    // Universal newlines would rewrite the carriage returns.
    cases.push(route(&home, "ask-cr.txt", "directory.json", &[]));
    // A path `expanduser` would change.
    let mut tilde = route(&home, "ask-link.txt", "directory.json", &[]);
    tilde[3] = "~/synthetic-ask.txt".to_owned();
    cases.push(tilde);
    // A missing required flag: argparse's usage error.
    cases.push(vec![
        "draft".into(),
        "route".into(),
        "--ask".into(),
        path(&home, "ask-link.txt"),
    ]);
    for args in &cases {
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        let direct = observe(&home, &argv, &Side::Native);
        assert_eq!(direct.code, Some(64), "{args:?} must refuse");
        same(&home, args, false, "draft.route");
    }
    let args = route(&redaction, "ask-link.txt", "directory.json", &[]);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(observe(&redaction, &argv, &Side::Native).code, Some(64));
    same(&redaction, &args, false, "draft.route");
    let args = judge(&redaction, "ask-link.txt", "C100", "directory.json", &[]);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(observe(&redaction, &argv, &Side::Native).code, Some(64));
    same(&redaction, &args, false, "draft.judge");
}

// ---------- "privacy.scan" --file ----------

/// Writes a draft outside the data root, in the live home and in the copy
/// every run restores, and returns its path.
fn outside(home: &Home, name: &str, bytes: &[u8]) -> String {
    let snap = home
        .home
        .parent()
        .unwrap()
        .with_file_name("snap")
        .join("home");
    for dir in [&home.home, &snap] {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    format!("{}/{name}", home.home.display())
}

fn scan_home() -> (Home, Vec<String>) {
    let home = prepare_setup(&Setup {
        id: "scan-drafts",
        cli: &[],
        files: &[("inputs/inside.md", "Synthetic notes\n")],
    });
    let root = home.data.display().to_string();
    let resolved = home.data.canonicalize().unwrap().display().to_string();
    let drafts: Vec<(&str, Vec<u8>)> = vec![
        ("clean.md", b"Synthetic notes for the team\n".to_vec()),
        ("private.md", b"see private/work/notes.md\n".to_vec()),
        (
            "system.md",
            b"/private/tmp/x and (/private/var/y) and /private/etc/z\n".to_vec(),
        ),
        ("glued.md", b"x/private/tmp/y\n".to_vec()),
        ("word.md", b"aprivate/x and private/ alone\n".to_vec()),
        ("twice.md", b"private/private/x\n".to_vec()),
        ("crlf.md", b"line one\r\n(private/tmp\r\nend\r".to_vec()),
        ("root.md", format!("data at {root}/day\n").into_bytes()),
        ("resolved.md", format!("data at {resolved}\n").into_bytes()),
        ("binary.md", vec![0x66, 0xff, 0xfe, 0x0a]),
        ("empty.md", Vec::new()),
    ];
    let paths = drafts
        .iter()
        .map(|(name, bytes)| outside(&home, name, bytes))
        .collect();
    (home, paths)
}

fn scan(files: &[&str], extra: &[&str]) -> Vec<String> {
    let mut argv = vec!["privacy".to_owned(), "scan".to_owned()];
    for file in files {
        argv.push("--file".to_owned());
        argv.push((*file).to_owned());
    }
    argv.extend(extra.iter().map(|s| (*s).to_owned()));
    argv
}

#[test]
fn privacy_scan_files_matches_python() {
    strict(NOW);
    let (home, drafts) = scan_home();
    let inside = format!("{}/inputs/inside.md", home.data.display());
    for draft in &drafts {
        same(&home, &scan(&[draft], &[]), true, "privacy.scan");
    }
    same(&home, &scan(&[&inside], &[]), true, "privacy.scan");
    let all: Vec<&str> = drafts.iter().map(String::as_str).collect();
    same(&home, &scan(&all, &[]), true, "privacy.scan");
    same(
        &home,
        &scan(&[&drafts[0], &drafts[0], &inside], &["--show"]),
        true,
        "privacy.scan",
    );
    same(
        &home,
        &scan(&[&drafts[1]], &["--show"]),
        true,
        "privacy.scan",
    );
    // A relative path, read from the repo root both sides run in.
    same(&home, &scan(&["LICENSE"], &[]), true, "privacy.scan");
}

#[test]
fn privacy_scan_refuses_what_it_cannot_match() {
    strict(NOW);
    let (home, drafts) = scan_home();
    let accent = outside(&home, "accent.md", "caf\u{e9} notes\n".as_bytes());
    let wide = outside(&home, "wide.md", "\u{ff50}rivate/x\n".as_bytes());
    let missing = format!("{}/missing.md", home.home.display());
    let doubled = drafts[0].replacen('/', "//", 1);
    let cases = vec![
        scan(&[&missing], &[]),
        scan(&[&drafts[0], &missing], &[]),
        scan(&[&accent], &[]),
        scan(&[&wide], &[]),
        scan(&["~/clean.md"], &[]),
        scan(&[&doubled], &[]),
        scan(&[&drafts[0]], &["--staged"]),
        scan(&[&drafts[0]], &["--repo", "."]),
        // The repo scan runs `git`.
        scan(&[], &[]),
        scan(&[], &["--staged"]),
    ];
    for args in &cases {
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(
            observe(&home, &argv, &Side::Native).code,
            Some(64),
            "{args:?} must refuse"
        );
    }
    // Compare Python and the console script on the forms without a git
    // walk of the repo.
    for args in &cases[..8] {
        same(&home, args, false, "privacy.scan");
    }
    let redaction = prepare_setup(&Setup {
        id: "scan-redaction",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
    });
    let draft = outside(&redaction, "clean.md", b"Synthetic notes\n");
    let args = scan(&[&draft], &[]);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(observe(&redaction, &argv, &Side::Native).code, Some(64));
    same(&redaction, &args, false, "privacy.scan");
}

/// Negative control for the scan: a clean draft and a leaking one differ
/// in output, exit code and event status.
#[test]
fn the_scan_comparison_detects_a_planted_difference() {
    strict(NOW);
    let (home, drafts) = scan_home();
    let clean = scan(&[&drafts[0]], &[]);
    let leak = scan(&[&drafts[1]], &[]);
    let clean: Vec<&str> = clean.iter().map(String::as_str).collect();
    let leak: Vec<&str> = leak.iter().map(String::as_str).collect();
    let a = observe(&home, &clean, &Side::Native);
    let b = observe(&home, &leak, &Side::Python);
    assert_eq!((a.code, b.code), (Some(0), Some(1)));
    assert_ne!(a.stdout, b.stdout);
    assert_ne!(a.events, b.events);
    assert!(b.stdout.contains("mentions-private-path"));
}

/// Negative control: the comparison sees a real difference. Two asks route
/// to different channels, and a failing judge exits 1 where a passing one
/// exits 0.
#[test]
fn the_comparison_detects_a_planted_difference() {
    strict(NOW);
    let home = home();
    let link = route(&home, "ask-link.txt", "directory.json", &[]);
    let open = route(&home, "ask-open.txt", "directory.json", &[]);
    let link: Vec<&str> = link.iter().map(String::as_str).collect();
    let open: Vec<&str> = open.iter().map(String::as_str).collect();
    let a = observe(&home, &link, &Side::Native);
    let b = observe(&home, &open, &Side::Python);
    assert_ne!(a.stdout, b.stdout);
    assert!(a
        .stdout
        .starts_with("access route: link the open thread t-0001 in C100"));
    let pass = judge(&home, "draft-ok.txt", "C100", "directory.json", &[]);
    let fail = judge(&home, "draft-ok.txt", "C200", "directory.json", &[]);
    let pass: Vec<&str> = pass.iter().map(String::as_str).collect();
    let fail: Vec<&str> = fail.iter().map(String::as_str).collect();
    let passed = observe(&home, &pass, &Side::Native);
    let failed = observe(&home, &fail, &Side::Python);
    assert_eq!((passed.code, failed.code), (Some(0), Some(1)));
    assert_ne!(passed.events, failed.events);
}
