//! Synthetic rust-12 workload over the native component functions.

use crate::{
    cards, cli, error, explain, field, prd, project, read_yaml, replay, text, tour, typed,
    write_yaml, Result,
};
use careerbrew_projects::projects::{self, AddOptions};
use chrono::{DateTime, Duration, FixedOffset};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CASES: &[&str] = &[
    "source-staging",
    "concept-validation",
    "learning-schedule",
    "replay",
    "tours",
    "explainers-cli",
];
pub const DUPLICATE_CONTROL: &str = "duplicate-write-control";
const GIT_CONFIG: &[&str] = &[
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "user.name=Synthetic",
    "-c",
    "user.email=synthetic@example.com",
];

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_LLM", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-10-26T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("TZ", "UTC");
        std::env::set_var("GIT_AUTHOR_DATE", "2026-10-26T09:00:00+00:00");
        std::env::set_var("GIT_COMMITTER_DATE", "2026-10-26T09:00:00+00:00");
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profile/examples");
        for relative in [
            "day/calendars.yaml",
            "jobs/criteria.yaml",
            "jobs/sources.yaml",
            "mail.yaml",
            "profile.yaml",
            "sensitive-terms.txt",
            "settings.yaml",
            "work/sources.yaml",
        ] {
            let text = std::fs::read_to_string(examples.join(relative))?;
            crate::write_text(&root.join(relative), &text)?;
        }
        Ok(Self { base, root })
    }
}

fn t0() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-10-26T09:00:00+00:00").expect("fixed time")
}

fn error_of<T>(result: Result<T>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn output(result: Result<cli::Invocation>) -> Value {
    cli::value(cli::caught(result))
}

fn json_output(result: Result<cli::Invocation>) -> Result<Value> {
    let output = cli::caught(result);
    let parsed = serde_json::from_str(&output.stdout).unwrap_or(Value::Null);
    Ok(json!({"exit": output.exit, "json": parsed}))
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(GIT_CONFIG)
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(error("RuntimeError", "synthetic git command failed"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn commit(repo: &Path, message: &str, files: &[(&str, &str)]) -> Result<String> {
    for (relative, text) in files {
        let path = repo.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)?;
    }
    git(repo, &["add", "-A"])?;
    git(repo, &["commit", "-q", "-m", message])?;
    git(repo, &["rev-parse", "HEAD"])
}

fn learning_repo(base: &Path) -> Result<PathBuf> {
    let repo = base.join("toyrepo");
    std::fs::create_dir_all(repo.join("src"))?;
    std::fs::create_dir_all(repo.join("docs"))?;
    std::fs::write(repo.join("README.md"), "# Toy\nA queue and a worker.\n")?;
    std::fs::write(
        repo.join("docs/design.md"),
        "# Design\nLEARNCANARY local only.\n",
    )?;
    std::fs::write(repo.join("src/queue.py"), "class Queue: ...\n")?;
    let status = Command::new("git")
        .args(GIT_CONFIG)
        .args(["init", "-q", "-b", "main"])
        .arg(&repo)
        .status()?;
    if !status.success() {
        return Err(error("RuntimeError", "synthetic git init failed"));
    }
    commit(&repo, "chore: start", &[])?;
    Ok(repo)
}

fn q(bloom: &str) -> Value {
    json!({"bloom": bloom, "prompt": "Why?", "answer": "Because."})
}

fn concepts_fixture() -> Vec<Value> {
    vec![
        json!({"id": "queue", "section": "core", "title": "The queue", "cites": ["repo:toyrepo/src/queue.py"], "questions": [q("understand"), q("apply")]}),
        json!({"id": "worker", "section": "core", "title": "The worker", "prereqs": ["queue"], "cites": ["note:01-core.md"], "questions": [q("understand"), {"bloom": "analyze", "prompt": "Why?", "answer": "Because."}]}),
        json!({"id": "deploy", "section": "ops", "title": "Deploys", "cites": ["doc:runbook"], "questions": [q("apply")]}),
    ]
}

fn stage(ws: &Workspace) -> Result<PathBuf> {
    let repo = learning_repo(&ws.base)?;
    let doc = ws.base.join("runbook.md");
    std::fs::write(&doc, "Deploy with make ship. LEARNCANARY\n")?;
    project::start(
        &ws.root,
        "Toy",
        &[
            ("repo".into(), repo.to_string_lossy().into_owned()),
            ("doc".into(), format!("runbook:{}", doc.display())),
        ],
        true,
    )?;
    project::note(
        &ws.root,
        "Toy",
        "01-core.md",
        Some("# Core\nQueue then worker. LEARNCANARY\n"),
    )?;
    Ok(repo)
}

fn source_staging(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "source-staging")?;
    if std::env::var("CAREERBREW_RUST12_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(ws.root.join("planted-extra.txt"), "synthetic")?;
    }
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    let repo = learning_repo(&ws.base)?;
    let before = git(&repo, &["status", "--porcelain", "--ignored"])?;
    let doc = ws.base.join("runbook.md");
    std::fs::write(&doc, "Deploy with make ship. LEARNCANARY\n")?;
    let first = project::start(
        &ws.root,
        "Toy",
        &[
            ("repo".into(), repo.to_string_lossy().into_owned()),
            ("doc".into(), format!("runbook:{}", doc.display())),
        ],
        true,
    )?;
    let stored = project::folder(&ws.root, "Toy").join("inputs/doc/runbook.md");
    let raw = std::fs::read(&stored)?;
    let repo_unchanged = before == git(&repo, &["status", "--porcelain", "--ignored"])?;
    let repeat = project::start(
        &ws.root,
        "Toy",
        &[("repo".into(), repo.to_string_lossy().into_owned())],
        false,
    )?;
    std::fs::write(repo.join("README.md"), "# Toy\nNow with retries.\n")?;
    let changed = project::start(
        &ws.root,
        "Toy",
        &[("repo".into(), repo.to_string_lossy().into_owned())],
        false,
    )?;
    project::note(
        &ws.root,
        "Toy",
        "01-core.md",
        Some("# Core\nLEARNCANARY note\n"),
    )?;
    let note_ok = project::note(&ws.root, "Toy", "01-core.md", None)?.ends_with("note\n");
    let sources = first["sources"].as_array().expect("sources");
    let changed_sources = changed["sources"].as_array().expect("sources");
    Ok(json!({
        "first": {"keys": sources.iter().map(|source| text(field(source, "key"))).collect::<Vec<_>>(), "docs": sources.iter().map(|source| field(source, "docs").cloned().unwrap_or(Value::Null)).collect::<Vec<_>>(), "freshness": first["freshness"]},
        "privacy": {"consumed": !doc.exists(), "sealed": raw.starts_with(b"CBV1\n"), "plain_absent": !raw.windows(b"LEARNCANARY".len()).any(|part| part == b"LEARNCANARY"), "repo_unchanged": repo_unchanged},
        "repeat": repeat["freshness"],
        "changed": {"freshness": changed["freshness"], "stale": changed["stale"], "stable_hash": changed_sources[0]["hash"] == sources[0]["hash"]},
        "note": {"read": note_ok, "bad_path": error_of(project::note(&ws.root, "Toy", "../bad.md", Some("x"))), "missing_project": error_of(project::note(&ws.root, "Missing", "x.md", None))},
        "projects": project::projects(&ws.root)?,
        "errors": [error_of(project::start(&ws.root, "Empty", &[], false)), error_of(project::start(&ws.root, "Toy", &[("bad".into(), "x".into())], false)), error_of(project::start(&ws.root, "Toy", &[("doc".into(), "missing".into())], false))],
        "cli": output(cli::learn_start(&ws.root, "Toy", &[("repo".into(), repo.to_string_lossy().into_owned())], false)),
    }))
}

fn bad_concepts() -> Vec<Value> {
    vec![
        json!({"id": "a", "section": "s", "title": "A", "prereqs": ["b"], "cites": ["repo:toyrepo/missing.py"], "questions": [q("remember")]}),
        json!({"id": "b", "section": "s", "title": "B", "prereqs": ["a"], "cites": ["note:01-core.md"], "questions": [{"bloom": "apply", "prompt": "Why?", "answer": "Because.", "options": ["x", "y", "z"], "misconceptions": ["one"]}]}),
    ]
}

fn write_input_yaml(path: &Path, rows: &[Value]) -> Result<()> {
    let value = careerbrew_setup::value::to_canon(&json!({"concepts": rows}));
    let text = careerbrew_vault::json_yaml::yaml_write(&value)
        .map_err(|_| error("ValueError", "cannot write synthetic YAML"))?;
    std::fs::write(path, text)?;
    Ok(())
}

fn concept_validation(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "concept-validation")?;
    stage(&ws)?;
    let bad = bad_concepts();
    let problems = project::check_concepts(&ws.root, "Toy", &bad)?;
    let refused = error_of(project::import_concepts(&ws.root, "Toy", &bad));
    let mut escape = concepts_fixture()[0].clone();
    escape["cites"] = json!(["repo:toyrepo/../outside.md"]);
    let outside = project::check_concepts(&ws.root, "Toy", &[escape])?;
    let imported = project::import_concepts(&ws.root, "Toy", &concepts_fixture())?;
    project::answer(
        &ws.root,
        "Toy",
        "queue",
        "correct",
        "high",
        "understand",
        "teach",
        "open",
        None,
        Some(t0()),
    )?;
    project::answer(
        &ws.root,
        "Toy",
        "queue",
        "correct",
        "high",
        "apply",
        "quiz",
        "open",
        None,
        Some(t0()),
    )?;
    let reduced = project::import_concepts(&ws.root, "Toy", &[concepts_fixture()[2].clone()])?;
    let status = project::status(&ws.root, "Toy", Some(t0() + Duration::days(2)))?;
    let next_after_remove = text(field(
        &project::next_step(&ws.root, "Toy", Some(t0() + Duration::days(2)))?,
        "concept",
    ));
    let learn_ids = cards::all_cards(&ws.root, &project::folder(&ws.root, "Toy"))?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let interview_id = format!("mine-{}", &crate::digest("Interview?")[..10]);
    write_yaml(
        &ws.root,
        "tutor/decks/mine.yaml",
        &json!({"cards": [{"id": interview_id, "question": "Interview?", "answer": "Public card.", "topic": "algo", "tags": ["algo"]}]}),
    )?;
    let interview = read_yaml(&ws.root, "tutor/decks/mine.yaml", json!({}))?;
    let interview_separate = !interview["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|card| text(field(card, "id")) == "deploy");
    let bad_file = ws.base.join("bad.yaml");
    let good_file = ws.base.join("good.yaml");
    write_input_yaml(&bad_file, &bad)?;
    write_input_yaml(&good_file, &concepts_fixture())?;
    Ok(json!({
        "bad": problems, "refused": refused, "outside": outside, "imported": imported, "reduced": reduced,
        "removed": {"status_due": status["due_retests"], "next": next_after_remove, "retests": project::model(&ws.root, "Toy")?["retests"].clone()},
        "decks": {"learn_only": learn_ids <= BTreeSet::from(["deploy".to_owned()]), "interview_separate": interview_separate},
        "cli": {"bad": output(cli::learn_concepts(&ws.root, "Toy", &bad_file, true, false)), "good": output(cli::learn_concepts(&ws.root, "Toy", &good_file, false, false)), "list": output(cli::learn_list(&ws.root))},
    }))
}

fn step_summary(step: &Value) -> Value {
    ["mode", "concept", "section", "level"]
        .iter()
        .map(|key| {
            (
                key.to_string(),
                field(step, key).cloned().unwrap_or(Value::Null),
            )
        })
        .collect::<Map<_, _>>()
        .into()
}

fn learning_schedule(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "learning-schedule")?;
    stage(&ws)?;
    project::import_concepts(&ws.root, "Toy", &concepts_fixture())?;
    let first = project::next_step(&ws.root, "Toy", Some(t0()))?;
    let pretest = project::answer(
        &ws.root,
        "Toy",
        "queue",
        "dontknow",
        "low",
        "understand",
        "pretest",
        "open",
        None,
        Some(t0()),
    )?;
    let second = project::next_step(&ws.root, "Toy", Some(t0()))?;
    project::answer(
        &ws.root,
        "Toy",
        "deploy",
        "wrong",
        "low",
        "apply",
        "pretest",
        "open",
        None,
        Some(t0()),
    )?;
    let third = project::next_step(&ws.root, "Toy", Some(t0()))?;
    let evidence = vec![
        project::answer(
            &ws.root,
            "Toy",
            "queue",
            "correct",
            "high",
            "apply",
            "pretest",
            "open",
            None,
            Some(t0()),
        )?["mastered"]
            .clone(),
        project::answer(
            &ws.root,
            "Toy",
            "queue",
            "correct",
            "low",
            "apply",
            "quiz",
            "open",
            None,
            Some(t0()),
        )?["mastered"]
            .clone(),
        project::answer(
            &ws.root,
            "Toy",
            "queue",
            "correct",
            "high",
            "understand",
            "quiz",
            "open",
            None,
            Some(t0()),
        )?["mastered"]
            .clone(),
    ];
    let mastered = project::answer(
        &ws.root,
        "Toy",
        "queue",
        "correct",
        "medium",
        "apply",
        "quiz",
        "open",
        None,
        Some(t0()),
    )?;
    let early = project::status(&ws.root, "Toy", Some(t0() + Duration::hours(1)))?;
    let day1 = t0() + Duration::days(1) + Duration::minutes(1);
    let due = project::next_step(&ws.root, "Toy", Some(day1))?;
    let missed = project::answer(
        &ws.root,
        "Toy",
        "queue",
        "wrong",
        "high",
        "apply",
        "retest",
        "open",
        None,
        Some(day1),
    )?;
    let after_miss = project::status(&ws.root, "Toy", Some(day1))?;
    project::answer(
        &ws.root,
        "Toy",
        "queue",
        "correct",
        "high",
        "understand",
        "quiz",
        "open",
        None,
        Some(day1),
    )?;
    project::answer(
        &ws.root,
        "Toy",
        "queue",
        "correct",
        "high",
        "apply",
        "quiz",
        "open",
        None,
        Some(day1),
    )?;
    let remastered = project::next_step(
        &ws.root,
        "Toy",
        Some(day1 + Duration::days(1) + Duration::minutes(1)),
    )?;
    let day8 = day1 + Duration::days(7) + Duration::minutes(2);
    project::answer(
        &ws.root,
        "Toy",
        "queue",
        "wrong",
        "high",
        "apply",
        "retest",
        "open",
        None,
        Some(day8),
    )?;
    let final_status = project::status(&ws.root, "Toy", Some(day8))?;
    let mut leech_row = Value::Null;
    for n in 0..8 {
        let time = t0() + Duration::days(n * 2);
        project::answer(
            &ws.root,
            "Toy",
            "worker",
            "correct",
            "high",
            "apply",
            "quiz",
            "open",
            None,
            Some(time),
        )?;
        leech_row = project::answer(
            &ws.root,
            "Toy",
            "worker",
            "wrong",
            "high",
            "apply",
            "review",
            "open",
            None,
            Some(time + Duration::days(1)),
        )?;
    }
    let suspended = cards::is_suspended(&ws.root, &project::folder(&ws.root, "Toy"), "worker")?;
    let queue = cards::review_queue(&ws.root, &project::folder(&ws.root, "Toy"), day8)?
        .iter()
        .map(|card| text(field(card, "id")))
        .collect::<Vec<_>>();
    let final_retests = project::model(&ws.root, "Toy")?["retests"].clone();
    let step_summaries = [first, second, third, due, remastered]
        .iter()
        .map(step_summary)
        .collect::<Vec<_>>();
    Ok(json!({
        "steps": step_summaries,
        "pretest": pretest["mastered"], "evidence": evidence,
        "mastered": {"value": mastered["mastered"], "retests": final_retests},
        "early": {"due_retests": early["due_retests"], "scheduled_share": early["scheduled_share"]},
        "missed": {"mastered": missed["mastered"], "section_mastered": after_miss["sections"]["core"]["mastered"], "due_retests": after_miss["due_retests"]},
        "final": {"retest_retained_7d": final_status["retest_retained_7d"], "brier": final_status["brier"], "answers": final_status["answers"], "taught": final_status["taught"]},
        "leech": {"flag": leech_row["leech"], "suspended": suspended, "reviews": project::log(&ws.root, "Toy")?.iter().filter(|row| text(field(row, "concept")) == "worker" && text(field(row, "mode")) == "review").count()},
        "queue": queue,
        "cli": {"answer": output(cli::learn_answer(&ws.root, "Toy", "deploy", "wrong", "high", "apply", "quiz", "open", None)), "status": json_output(cli::learn_status(&ws.root, "Toy", true))?, "next": output(cli::learn_next(&ws.root, "Toy"))},
    }))
}

fn replay_repo(base: &Path) -> Result<PathBuf> {
    let repo = base.join("toyrepo");
    std::fs::create_dir_all(&repo)?;
    let status = Command::new("git")
        .args(GIT_CONFIG)
        .args(["init", "-q", "-b", "main"])
        .arg(&repo)
        .status()?;
    if !status.success() {
        return Err(error("RuntimeError", "synthetic git init failed"));
    }
    commit(
        &repo,
        "chore: start",
        &[
            ("README.md", "# Toy\n"),
            ("src/toy/__init__.py", "def add(a, b):\n    return a + b\n"),
        ],
    )?;
    commit(
        &repo,
        "feat: double a number (#7)",
        &[
            (
                "src/toy/__init__.py",
                "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return add(n, n)\n",
            ),
            (
                "tests/test_double.py",
                "from toy import double\n\n\ndef test_double():\n    assert double(4) == 8\n",
            ),
        ],
    )?;
    commit(
        &repo,
        "docs: say hello (#8)",
        &[("README.md", "# Toy\nHello.\n")],
    )?;
    commit(
        &repo,
        "a direct push, not a PR",
        &[("README.md", "# Toy\nHi.\n")],
    )?;
    Ok(repo)
}

fn replay_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "replay")?;
    let repo = replay_repo(&ws.base)?;
    project::start(
        &ws.root,
        "Toy",
        &[("repo".into(), repo.to_string_lossy().into_owned())],
        false,
    )?;
    let rows = replay::candidates(&repo, 50)?;
    let before = git(&repo, &["status", "--porcelain", "--ignored"])?;
    let refs = std::fs::read_dir(repo.join(".git"))?
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    let python = std::env::var("CAREERBREW_RUST12_PYTHON").unwrap_or_else(|_| "python3".into());
    let task = replay::start(&ws.root, "Toy", 7, None, Some(&python))?;
    let work = PathBuf::from(text(field(&task, "dir")));
    let parent_has_no_double =
        !std::fs::read_to_string(work.join("src/toy/__init__.py"))?.contains("double");
    let initial = replay::check(&ws.root, "Toy", 7)?;
    std::fs::write(
        work.join("src/toy/__init__.py"),
        "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n",
    )?;
    let passed = replay::check(&ws.root, "Toy", 7)?;
    let measured = replay::measure(&repo, 50, Some(&python))?;
    let after_refs = std::fs::read_dir(repo.join(".git"))?
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    let cli_check = cli::caught(cli::replay_check(&ws.root, "Toy", 7));
    Ok(json!({
        "candidates": rows,
        "task": {"pr": task["pr"], "title": task["title"], "tests": task["tests"], "inside_data": work.starts_with(&ws.root), "parent_has_no_double": parent_has_no_double},
        "checks": [{"passed": initial["passed"], "tests": initial["tests"]}, {"passed": passed["passed"], "tests": passed["tests"]}],
        "repo_untouched": before == git(&repo, &["status", "--porcelain", "--ignored"])? && refs == after_refs,
        "no_tests": error_of(replay::start(&ws.root, "Toy", 8, None, Some(&python))),
        "measure": measured,
        "cli": {"list": output(cli::replay_list(&ws.root, "Toy", None, 20)), "check": {"exit": cli_check.exit, "passed": cli_check.stdout.contains("PR #7: pass")}, "bad": output(cli::replay_start(&ws.root, "Toy", 8, None, Some(&python)))},
    }))
}

fn tours(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "tours")?;
    let repo = replay_repo(&ws.base)?;
    project::start(
        &ws.root,
        "Toy",
        &[("repo".into(), repo.to_string_lossy().into_owned())],
        false,
    )?;
    project::import_concepts(
        &ws.root,
        "Toy",
        &[
            json!({"id": "double", "section": "core", "title": "Doubling", "cites": ["repo:toyrepo/src/toy/__init__.py"], "questions": [{"bloom": "apply", "prompt": "double(3)?", "answer": "6"}]}),
        ],
    )?;
    let valid = json!({"repo": "toyrepo", "title": "How doubling works", "steps": [{"file": "src/toy/__init__.py", "line": 5, "description": "double reuses add.", "concept": "double"}, {"file": "README.md", "line": 1, "description": "The project name."}]});
    let bad = json!({"repo": "toyrepo", "title": "x", "steps": [{"file": "missing.py", "line": 1, "description": "d"}, {"file": "README.md", "line": 99, "description": ""}, {"file": "README.md", "line": 1, "description": "d", "concept": "nope"}]});
    let problems = tour::check(&ws.root, "Toy", &bad)?;
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    let added = tour::add(&ws.root, "Toy", "doubling", &valid)?;
    let raw = std::fs::read(project::folder(&ws.root, "Toy").join("tours/doubling.json"))?;
    let (out, unreviewed) = tour::export(&ws.root, "Toy", "doubling")?;
    let unreviewed_note = serde_json::from_str::<Value>(&std::fs::read_to_string(&out)?)?["steps"]
        [0]["description"]
        .as_str()
        .unwrap_or("")
        .starts_with("**No maintainer");
    for level in ["understand", "apply"] {
        project::answer(
            &ws.root,
            "Toy",
            "double",
            "correct",
            "high",
            level,
            "quiz",
            "open",
            None,
            Some(t0()),
        )?;
    }
    let approved = tour::approve(&ws.root, "Toy", "doubling", "maintainer")?;
    let (out, faded) = tour::export(&ws.root, "Toy", "doubling")?;
    let faded_text = serde_json::from_str::<Value>(&std::fs::read_to_string(&out)?)?["steps"][0]
        ["description"]
        .clone();
    commit(
        &repo,
        "fix: double with a multiply (#9)",
        &[(
            "src/toy/__init__.py",
            "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n",
        )],
    )?;
    let (_, stale) = tour::export(&ws.root, "Toy", "doubling")?;
    let source = ws.base.join("tour.json");
    std::fs::write(&source, serde_json::to_string(&valid)?)?;
    Ok(json!({
        "bad": problems,
        "added": {"approved": added["approved"], "steps": added["steps"].as_array().map_or(0, Vec::len), "plain_absent": !raw.windows(b"The project name".len()).any(|part| part == b"The project name")},
        "unreviewed": {"info": unreviewed, "note": unreviewed_note},
        "approved": {"sha_len": text(field(&approved["approved"], "sha")).len(), "info": faded, "faded_text": faded_text},
        "stale": stale,
        "errors": [error_of(tour::add(&ws.root, "Toy", "Bad Name", &valid)), error_of(tour::load(&ws.root, "Toy", "missing"))],
        "cli": {"add": output(cli::tour_add(&ws.root, "Toy", "second", &source, false)), "approve": output(cli::tour_approve(&ws.root, "Toy", "second", "maintainer")), "export": output(cli::tour_export(&ws.root, "Toy", "second"))},
    }))
}

fn explainers(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "explainers-cli")?;
    let root = std::env::var_os("CAREERBREW_RUST12_REPO")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let skill = root.join("skills/careerbrew-project-explainer/references");
    let example = std::fs::read_to_string(skill.join("example-careerbrew.md"))?;
    let example_prd = std::fs::read_to_string(skill.join("example-prd.md"))?;
    let deep_text = example.replace("## Invariants and boundaries", "## How it runs\n\n- Start everything with `uv run careerbrew`: it starts the watcher and the web app [S1].\n\n## Invariants and boundaries");
    let mut short_text = String::new();
    for part in RegexSplit::new(&example) {
        if !part.starts_with("## ")
            || explain::SHORT.contains(
                &explain::base(part[3..].split('\n').next().unwrap_or("").trim()).as_str(),
            )
        {
            short_text.push_str(part);
        }
    }
    short_text = short_text.replace("| S2 | primary | docs/VISION.md@c334531 | c334531 |\n", "");
    let bad_source = example.replace(
        "| S5 | primary | src/careerbrew/work/ | 2026-10-21 |",
        "| S5 | lead | src/careerbrew/work/ | 2026-10-21 |\n| S6 | maybe | ??? | |",
    );
    projects::add(
        &ws.root,
        "billing",
        AddOptions {
            title: String::new(),
            group: String::new(),
            parent: String::new(),
            links: Vec::new(),
            kind: String::new(),
            event_on: String::new(),
        },
    )?;
    projects::add_source(&ws.root, "billing", "tracker", "PROJ-1")?;
    let scaffold = explain::scaffold(&ws.root, "billing", false, true)?;
    let prd_scaffold = prd::scaffold(&ws.root, "billing", Some(4.0))?;
    let explain_path = ws.base.join("explain.md");
    let prd_path = ws.base.join("prd.md");
    std::fs::write(&explain_path, &example)?;
    std::fs::write(&prd_path, &example_prd)?;
    let no_pushback = remove_pushback(&example_prd);
    let unit_checks = ["3 days", "2 sprints", "5 points", "40 hours", "2 weeks"]
        .iter()
        .map(|unit| prd::check(&example_prd.replace("| 0.5 dev weeks |", &format!("| {unit} |"))))
        .collect::<Vec<_>>();
    let cli_prd = output(cli::project_prd(&ws.root, "billing", None, Some(2.0)));
    if std::env::var("CAREERBREW_RUST12_PLANT_WRITE").as_deref() == Ok("missing") {
        std::fs::remove_file(ws.root.join("out/prd-billing.md"))?;
    }
    Ok(json!({
        "full": explain::check(&example, false, false), "short": explain::check(&short_text, true, false),
        "deep": {"pass": explain::check(&deep_text, false, true), "missing": explain::check(&example, false, true), "command": explain::check(&deep_text.replace("with `uv run careerbrew`", "with the start command"), false, true)},
        "source_rules": explain::check(&bad_source, false, false),
        "structure": {"order": explain::check(&example.replace("## Gotchas", "## Tmp").replace("## Read next", "## Gotchas").replace("## Tmp", "## Read next"), false, false), "citation": explain::check(&example.replace("flags) [S5].", "flags).\n  - see [S5]"), false, false), "acronym": explain::check(&example.replace("KEEP (met its target) state", "KEEP state"), false, false)},
        "prd": {"pass": prd::check(&example_prd), "headings": prd::check(&example_prd.replace("## Requirements", "## Asks").replace("## Decisions", "## Choices")), "decision": prd::check(&example_prd.replace("| decided | the owner | 2026-10-27 | [S1] |\n| Estimates", "| maybe | the owner | 2026-10-27 | [S1] |\n| Estimates")), "units": unit_checks, "slot": prd::check(&example_prd.replace("Plan slot: 1 dev week [S1]\n", "")), "pushback": prd::check(&no_pushback), "fits": prd::check(&no_pushback.replace("Plan slot: 1 dev week", "Plan slot: 3 dev weeks")), "open": prd::check(&example_prd.replace("Who: the owner", "ask around"))},
        "scaffolds": {"deep": scaffold.contains("## How it runs") && scaffold.contains("| S1 | primary | PROJ-1 | TODO |"), "deep_fails": !explain::check(&scaffold, false, true).is_empty(), "prd": prd_scaffold.contains("Plan slot: 4 dev weeks") && prd_scaffold.contains("| S1 | primary | PROJ-1 | TODO |"), "prd_fails": !prd::check(&prd_scaffold).is_empty()},
        "cli": {"explain": output(cli::project_explain(&ws.root, "billing", None, false, false)), "explain_check": output(cli::project_explain(&ws.root, "billing", Some(&explain_path), false, false)), "deep_check": output(cli::project_explain(&ws.root, "billing", Some(&explain_path), false, true)), "prd": cli_prd, "prd_check": output(cli::project_prd(&ws.root, "billing", Some(&prd_path), None)), "unknown": output(cli::project_explain(&ws.root, "missing", None, false, false))},
    }))
}

struct RegexSplit<'a> {
    text: &'a str,
    offset: usize,
}
impl<'a> RegexSplit<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, offset: 0 }
    }
}
impl<'a> Iterator for RegexSplit<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.text.len() {
            return None;
        }
        let remaining = &self.text[self.offset..];
        let next = remaining[1..]
            .find("\n## ")
            .map(|index| self.offset + index + 2);
        let end = next.unwrap_or(self.text.len());
        let part = &self.text[self.offset..end];
        self.offset = end;
        Some(part)
    }
}

fn remove_pushback(text: &str) -> String {
    let Some(start) = text.find("## Pushback") else {
        return text.to_owned();
    };
    let tail = &text[start..];
    let end = tail
        .find("\n## Open questions")
        .map(|end| start + end + 1)
        .unwrap_or(text.len());
    format!("{}{}", &text[..start], &text[end..])
}

fn run(name: &str, base: &Path) -> Result<Value> {
    match name {
        "source-staging" => source_staging(base),
        "concept-validation" => concept_validation(base),
        "learning-schedule" => learning_schedule(base),
        "replay" => replay_case(base),
        "tours" => tours(base),
        "explainers-cli" => explainers(base),
        _ => Err(error("ValueError", "unknown rust-12 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let root = crate::root()?;
    let base = root.join("rust12-workload");
    std::fs::create_dir_all(&base)?;
    if selected == Some(DUPLICATE_CONTROL) {
        return duplicate_control(&base);
    }
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_none_or(|selected| selected == *name) {
            let result = match run(name, &base) {
                Ok(value) => value,
                Err(error) => json!({"error": error.class, "message": error.message}),
            };
            cases.push(json!({"id": name, "result": result}));
        }
    }
    Ok(serde_json::to_string(&json!({"cases": typed(&json!({"cases": cases}))["cases"]}))? + "\n")
}

fn duplicate_control(base: &Path) -> Result<String> {
    let out = base.join("explainers-cli/data/out/prd-billing.md");
    let existing = std::fs::read_to_string(&out)?;
    crate::write_text(&out, &existing)?;
    let cases = vec![json!({"id": DUPLICATE_CONTROL, "result": {"ok": true}})];
    Ok(serde_json::to_string(&json!({"cases": typed(&json!({"cases": cases}))["cases"]}))? + "\n")
}
