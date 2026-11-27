//! Synthetic rust-18 differential workload: the same cases, fixtures and
//! write order as `tests/oracles/rust18.py`.

use crate::when::{self, At};
use crate::{evidence, float, int, ladder_grade, linkedin, promo, typed, Error, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "evidence-tags",
    "promo-assess",
    "ladder-grade",
    "linkedin-block",
];
const CANARY: &str = "SYNTHCANARY-rust18-term";
const PLANT: &str = "CAREERBREW_RUST18_PLANT_WRITE";

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str, settings: Option<Value>) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        if root.exists() {
            return Err(Error::new("FileExistsError", "workspace exists"));
        }
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_LLM", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-11-03T11:00:00+00:00");
        std::env::set_var("CAREERBREW_RUST13_WRITE_TRACE", base.join("writes.log"));
        std::env::set_var("TZ", "UTC");
        if let Some(settings) = settings {
            crate::write_yaml(&root.join("settings.yaml"), &settings)?;
        }
        Ok(Self { root })
    }

    fn evidence(&self, rows: &[Value]) -> Result<()> {
        crate::write_jsonl(&evidence::path()?, rows)
    }
}

fn shown<T>(result: Result<T>) -> String {
    match result {
        Ok(_) => "accepted".into(),
        Err(error) => error.shown(),
    }
}

fn strings(values: &[String]) -> Value {
    json!(values)
}

fn plant() -> String {
    std::env::var(PLANT).unwrap_or_default()
}

// ---------- evidence ----------

const TITLES: &[&str] = &[
    "feat(api): ship the order export",
    "fix: flaky checkout test",
    "perf: cut p99 latency of search",
    "Refactor(core): split the parser",
    "docs: explain the migration",
    "ci: cache the build",
    "chore: bump deps",
    "Led the incident review for the outage",
    "Mentored two new hires on design",
    "RFC for the queue",
    "plain title with no prefix",
    "",
];

fn evidence_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "evidence-tags", None)?;
    let tags = TITLES
        .iter()
        .map(|t| strings(&evidence::tags_for(t)))
        .collect::<Vec<_>>();
    let owned = ["ownership".to_owned(), "custom".to_owned()];
    let at = "2026-11-02T15:00:00+00:00";
    let first = evidence::note(
        "Led the incident review",
        &owned,
        "cut the time to fix by 40%",
        Some(at),
        &[],
    )?;
    let second = evidence::note(
        "Mentored a new hire on design",
        &[],
        "",
        None,
        &["https://example.com/doc/1".to_owned()],
    )?;
    let again = evidence::note(
        "Led the incident review",
        &owned,
        "cut the time to fix by 40%",
        Some(at),
        &[],
    )?;
    let loaded = evidence::load()?;
    let ids: BTreeSet<String> = [crate::text_of(first.get("id")), "note:missing".into()].into();
    let gone = evidence::forget(&ids)?;
    let after = evidence::load()?
        .iter()
        .map(|row| row["id"].clone())
        .collect::<Vec<_>>();
    if plant() == "extra" {
        std::fs::write(ws.root.join("planted-extra.txt"), "synthetic")?;
    }
    Ok(json!({
        "tags": tags,
        "first": typed(&first),
        "second": typed(&second),
        "again_id": again["id"],
        "loaded": int(loaded.len()),
        "forgot": int(gone),
        "after": after,
    }))
}

// ---------- promo ----------

fn promo_evidence() -> Vec<Value> {
    vec![
        json!({"id": "pr:example/shop#12", "kind": "pr", "repo": "example/shop", "title": "feat: ship the order export", "url": "https://github.com/example/shop/pull/12", "at": "2026-10-20T10:00:00Z", "tags": ["delivery"]}),
        json!({"id": "pr:example/shop#14", "kind": "pr", "repo": "example/shop", "title": "Launch the billing page", "url": "https://github.com/example/shop/pull/14", "at": "2026-10-22T10:00:00+00:00", "tags": []}),
        json!({"id": "git:shop:abc123def456", "kind": "commit", "repo": "shop", "title": "fix: flaky checkout test", "at": "2026-09-01T10:00:00+00:00", "tags": ["quality"]}),
        json!({"id": "git:shop:bbb123def456", "kind": "commit", "repo": "shop", "title": "perf: cut search latency 40%", "at": "2025-01-01T10:00:00+00:00", "tags": ["quality", "impact"]}),
        json!({"id": "note:1", "kind": "note", "title": "Wrote the RFC for the job queue", "impact": "unblocked two teams", "at": "2026-10-01T10:00:00+00:00", "tags": ["design"]}),
        json!({"id": "note:2", "kind": "note", "title": "Mentored a new hire", "at": "2026-08-01T10:00:00+00:00", "tags": ["leadership"]}),
        json!({"id": "note:3", "kind": "note", "title": "Graded line about the design review", "at": "2026-10-30T18:00:00+00:00", "tags": ["dim:collaboration"]}),
        json!({"id": "note:4", "kind": "note", "title": "Migrated the billing jobs", "at": "not a time", "tags": []}),
        json!({"id": "note:5", "kind": "note", "title": "Shipping notes for mentoring", "tags": []}),
    ]
}

fn bad_ladders() -> Vec<Value> {
    vec![
        Value::Null,
        json!({"id": "x", "levels": ["a"]}),
        json!({"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}}]}),
        json!({"id": "x", "levels": ["a", "b"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {"a": 1}}]}),
        json!({"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {"a": 1}, "examples": ["no"]}]}),
    ]
}

fn pinned(text: &str) -> At {
    let (naive, offset) = when::parse(text).expect("fixture time");
    when::fixed(naive, offset.expect("fixture offset"))
}

fn promo_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "promo-assess", None)?;
    ws.evidence(&promo_evidence())?;
    crate::write_yaml(
        &ws.root.join("goals.yaml"),
        &json!({"long_term": [{"text": "Grow into technical leadership and mentoring"}], "queue": []}),
    )?;
    let at = pinned("2026-11-03T11:00:00+00:00");
    let assessed = promo::assess("generic-swe", "senior", &at)?;
    let staff = promo::assess("generic-swe", "staff", &at)?;
    let gaps = promo::gaps(&assessed)
        .iter()
        .map(|row| json!(row.id))
        .collect::<Vec<_>>();
    let titles = [
        "Migrating jobs",
        "remigrate",
        "ship it",
        "shipped",
        "the feat-flag",
        "Fix: x",
    ];
    let keywords = ["migrat*", "ship", "feat", "fix*"]
        .iter()
        .map(|keyword| {
            let comp = json!({"id": "c", "signals": {"keywords": [keyword]}});
            titles
                .iter()
                .map(|title| json!(promo::matches(&json!({"title": title, "tags": []}), &comp)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut errors = bad_ladders()
        .iter()
        .map(|data| shown(promo::validate_ladder(data)))
        .collect::<Vec<_>>();
    errors.push(shown(promo::assess("missing", "senior", &at)));
    errors.push(shown(promo::assess("generic-swe", "principal", &at)));
    let path = promo::write_doc(&assessed)?;
    let doc = std::fs::read_to_string(&path)?;
    let relative = path
        .strip_prefix(&ws.root)
        .map_err(|_| Error::value("doc escaped its root"))?
        .to_string_lossy()
        .into_owned();
    match plant().as_str() {
        "missing" => std::fs::remove_file(ws.root.join("goals.yaml"))?,
        "duplicate" => crate::write_jsonl(&evidence::path()?, &promo_evidence())?,
        _ => {}
    }
    Ok(json!({
        "assessed": typed(&assessed.view()),
        "staff_overall": float(staff.overall),
        "staff_warnings": staff.warnings,
        "gaps": gaps,
        "keywords": keywords,
        "errors": errors,
        "doc_path": relative,
        "doc": doc,
    }))
}

// ---------- ladder grade ----------

const LINES: &[&str] = &[
    "Shipped the order export to all stores",
    "Fixed the flaky checkout test and added a regression test",
    "Reviewed four pull requests",
    "",
    "   ",
    "Wrote the design doc for the queue",
    "Mentored a new hire",
    "Cut search latency",
];

fn ladder_case(base: &Path) -> Result<Value> {
    Workspace::new(
        base,
        "ladder-grade",
        Some(
            json!({"ladder": "generic-swe", "target_level": "senior", "timezone": "America/New_York"}),
        ),
    )?;
    let levels = ladder_grade::levels("generic-swe", "senior")?;
    let dims = LINES
        .iter()
        .map(|line| ladder_grade::dims_of(line, None).map(Value::Array))
        .collect::<Result<Vec<_>>>()?;
    let unknown = ladder_grade::dims_of("Shipped", Some("missing"))?;
    let graded = ladder_grade::grade(LINES, "generic-swe", "senior")?;
    let added = ladder_grade::record(&graded, "2026-11-02")?;
    let again = ladder_grade::record(&graded, "2026-11-02")?;
    let errors = vec![
        shown(ladder_grade::levels("missing", "senior")),
        shown(ladder_grade::levels("generic-swe", "principal")),
    ];
    let stored = evidence::load()?
        .iter()
        .map(|row| {
            let mut out = Map::new();
            for key in ["id", "title", "at", "tags"] {
                out.insert(key.into(), row.get(key).cloned().unwrap_or(Value::Null));
            }
            Value::Object(out)
        })
        .collect::<Vec<_>>();
    let (ladder, level) = ladder_grade::target()?;
    Ok(json!({
        "target": [ladder, level],
        "levels": typed(&Value::Array(levels)),
        "dims": dims,
        "unknown": unknown,
        "graded": typed(&graded),
        "added": int(added),
        "again": int(again),
        "errors": errors,
        "stored": typed(&Value::Array(stored)),
    }))
}

// ---------- LinkedIn ----------

fn linkedin_evidence() -> Vec<Value> {
    vec![
        json!({"id": "n1", "kind": "note", "title": "Led the billing migration", "impact": "cut cost by $40k a year", "at": "2026-10-01T10:00:00+00:00", "tags": ["impact"]}),
        json!({"id": "n2", "kind": "pr", "repo": "example/shopfront", "title": "feat(api): add the order export to shopfront", "at": "2026-10-02T10:00:00+00:00", "tags": ["ownership"]}),
        json!({"id": "n3", "kind": "commit", "repo": "shopfront", "title": "PROJ-12: fix flaky checkout test", "at": "2026-10-03T10:00:00+00:00", "tags": ["quality"]}),
        json!({"id": "n4", "kind": "commit", "title": format!("perf: improve search latency 40% for {CANARY}"), "at": "2026-10-04T10:00:00+00:00", "tags": ["quality", "impact"]}),
        json!({"id": "n5", "kind": "slack", "title": "Thanks for the big launch", "at": "2026-10-05T10:00:00+00:00", "tags": ["design"]}),
        json!({"id": "n6", "kind": "note", "title": "Designed the event schema", "at": "2026-10-06T10:00:00+00:00", "tags": ["design"]}),
        json!({"id": "n7", "kind": "note", "title": "fix: x", "at": "2026-10-07T10:00:00+00:00", "tags": []}),
        json!({"id": "n8", "kind": "note", "title": "Designed the event schema", "at": "2026-10-08T10:00:00+00:00", "tags": ["design"]}),
        json!({"id": "n9", "kind": "commit", "title": "docs: see https://example.com/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p", "at": "2026-10-09T10:00:00+00:00", "tags": ["quality"]}),
        json!({"id": "n10", "kind": "commit", "title": "ci: automate the release in 3 days", "at": "2025-01-01T10:00:00+00:00", "tags": ["ownership"]}),
    ]
}

fn block_view(block: &linkedin::Block, root: &Path) -> Result<Value> {
    let path = block
        .path
        .strip_prefix(root)
        .map_err(|_| Error::value("block escaped its root"))?;
    Ok(json!({
        "path": path.to_string_lossy(),
        "text": block.text,
        "bullets": int(block.bullets),
        "chars": int(crate::py::len(&block.text)),
        "public_metrics": block.public_metrics,
        "suspects": block.suspects,
    }))
}

fn linkedin_case(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "linkedin-block", None)?;
    ws.evidence(&linkedin_evidence())?;
    crate::write_text(
        &ws.root.join("sensitive-terms.txt"),
        &format!("# synthetic terms\n{CANARY}\nab\n"),
    )?;
    let terms = linkedin::sensitive_terms()?;
    let repos: BTreeSet<String> = ["shopfront".to_owned(), "api".to_owned()].into();
    let mut cleaned = Vec::new();
    for title in [
        "feat: add the shopfront export 2x faster",
        "PROJ-9: Migrate the api to v2 in 3 days",
        "fix: ",
        "refactor(core): reworked caching",
        "docs: ship notes",
        "cut $1.5m cost",
    ] {
        for metrics in [true, false] {
            cleaned.push(json!(linkedin::clean(title, &repos, &[], metrics)?));
        }
    }
    let ranks = linkedin_evidence()
        .iter()
        .map(|item| {
            let (score, zero, at) = linkedin::rank(item);
            json!([{"int": score.to_string()}, {"int": zero.to_string()}, at])
        })
        .collect::<Vec<_>>();
    let built = linkedin::build("Senior engineer", 4, None)?;
    let since = linkedin::build("", 6, Some("2026-10-05"))?;
    let text = std::fs::read_to_string(linkedin::path()?)?;
    let blocked = shown(linkedin::build("Engineer at Proj-1 PROJ-77", 2, None));
    Ok(json!({
        "terms_count": int(terms.len()),
        "cleaned": cleaned,
        "ranks": ranks,
        "built": block_view(&built, &ws.root)?,
        "since": block_view(&since, &ws.root)?,
        "file": text,
        "blocked": blocked,
    }))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "evidence-tags" => evidence_case(base),
        "promo-assess" => promo_case(base),
        "ladder-grade" => ladder_case(base),
        "linkedin-block" => linkedin_case(base),
        _ => Err(Error::value("unknown rust-18 case")),
    }
}

/// `--all` or `--case <id>`: the oracle's compact report.
pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust18-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            cases.push(json!({"id": case, "result": run(case, &base)?}));
        }
    }
    let report = serde_json::to_string(&json!({ "cases": cases }))
        .map_err(|_| Error::value("report is not JSON"))?;
    Ok(report + "\n")
}
