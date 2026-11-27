//! Synthetic rust-19 workload. Mirrors `tests/oracles/rust19.py` case by case.

use crate::criteria;
use crate::linkedin::{self, Alert};
use crate::prep;
use crate::sources::{self, Posting};
use crate::want::{self, Weights};
use crate::{JobsError, Result};
use careerbrew_setup::machine;
use careerbrew_tutor::{compact, read_json, read_yaml, typed, write_yaml};
use serde_json::{json, Map, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "helpers",
    "ats-scan",
    "criteria-filter",
    "want-learn",
    "linkedin-alerts",
    "corporate-guard",
];
const GROUPS: &[&str] = &[
    "jobs",
    "interview",
    "interview-patterns",
    "prep",
    "drill",
    "cards",
    "research",
    "day",
];

struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-10-19T13:00:00+00:00");
        std::env::set_var("CAREERBREW_EXTRA_KNOWLEDGE", "0");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_HTTP_FIXTURES");
        Ok(Self { root })
    }
}

fn repo_root() -> PathBuf {
    let path = std::env::var_os("CAREERBREW_RUST19_REPO")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    path.canonicalize().unwrap_or(path)
}

fn error<T>(result: Result<T>) -> Value {
    match result {
        Ok(_) => json!(["accepted", ""]),
        Err(error) => json!([error.class, error.message]),
    }
}

fn posting(id: &str, company: &str, title: &str) -> Posting {
    Posting::new(
        id,
        company,
        title,
        "Remote",
        true,
        &format!("https://example.com/{id}"),
        "",
    )
}

fn with(mut p: Posting, edit: impl FnOnce(&mut Posting)) -> Posting {
    edit(&mut p);
    p
}

fn regions_json(regions: &sources::CustomRegions) -> Value {
    Value::Array(
        regions
            .iter()
            .map(|(name, words)| json!([name, words]))
            .collect(),
    )
}

fn helpers(base: &Path) -> Result<Value> {
    Workspace::new(base, "helpers")?;
    let repo = repo_root();
    let vocab = sources::skill_vocab(&repo)?;
    let texts = [
        "&lt;p&gt;Go &amp; Kubernetes&lt;/p&gt;  <b>Kafka</b>\n\tnow",
        "<div>a&nbsp;b &#39;c&#39; &quot;d&quot; &#x41;</div>",
        "",
    ];
    let salaries = [
        "$150k - $180k",
        "$210,000 - $260,000",
        "$90K to $1,200",
        "pay is $100k",
        "$1000k - $2000k",
        "$95k–$120k",
    ];
    let titles = [
        "Principal Engineer",
        "Staff SWE",
        "Team Lead",
        "Senior Dev",
        "Sr. Engineer",
        "Sr Engineer",
        "Junior Dev",
        "Intern",
        "Engineer",
        "SRE",
    ];
    let skill_texts = [
        "Python, golang and k8s on AWS",
        "Node.js plus TypeScript; ci/cd",
        "nothing here",
        "Rust-lang and C++ with ML",
        "Go-to market goal",
    ];
    let regions = [
        "Remote - US",
        "US only",
        "Anywhere in the World",
        "EMEA or US Pacific Time",
        "Asia Pacific time",
        "Remote, Central Time",
        "Australian Central Time",
        "Europe",
        "Examplia remote",
        "Examplian citizens",
        "Example City",
        "LATAM",
        "Worldwide",
        "Pacific timezone",
    ];
    let custom = criteria::custom_regions(
        &json!({"regions_ok": ["global", "Examplia"], "region_words": {"examplia": ["Examplian", "examplia "]}}),
    )?;
    let bad = [
        json!({"regions_ok": [""]}),
        json!({"regions_ok": 5}),
        json!({"region_words": []}),
        json!({"region_words": {"us": ["x"]}}),
        json!({"region_words": {"x": "y"}}),
        json!({"region_words": {"x": [""]}}),
        json!({"region_words": {"": ["y"]}}),
    ];
    let none = Vec::new();
    Ok(json!({
        "strip": texts.iter().map(|t| sources::strip_html(t)).collect::<Vec<_>>(),
        "salary": salaries.iter().map(|t| { let (a, b) = sources::parse_salary(t); json!([a, b]) }).collect::<Vec<_>>(),
        "seniority": titles.iter().map(|t| sources::seniority_of(t)).collect::<Vec<_>>(),
        "skills": skill_texts.iter().map(|t| sources::extract_skills(t, &vocab)).collect::<Vec<_>>(),
        "regions": regions.iter().map(|t| sources::region_of(t, &none)).collect::<Vec<_>>(),
        "custom": regions_json(&custom),
        "custom_regions": regions.iter().map(|t| sources::region_of(t, &custom)).collect::<Vec<_>>(),
        "region_errors": bad.iter().map(|c| error(criteria::custom_regions(c))).collect::<Vec<_>>(),
        "single_label": regions_json(&criteria::custom_regions(&json!({"regions_ok": "Examplia"}))?),
        "vocab_size": vocab.len(),
    }))
}

fn ats_scan(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "ats")?;
    let repo = repo_root();
    let ats = repo.join("tests/fixtures/ats");
    let url = |name: &str| format!("file://{}", ats.join(name).display());
    write_yaml(
        &ws.root,
        "jobs/sources.yaml",
        &json!({"sources": [
            {"kind": "greenhouse", "board": "northwind", "company": "Northwind", "url": url("greenhouse-northwind.json"), "tier": "dream"},
            {"kind": "lever", "board": "contoso", "company": "Contoso", "url": url("lever-contoso.json")},
            {"kind": "ashby", "board": "fabrikam", "url": url("ashby-fabrikam.json")},
        ]}),
    )?;
    write_yaml(
        &ws.root,
        "jobs/criteria.yaml",
        &json!({"regions_ok": ["global", "us"], "region_words": {"sampleland": ["sample town"]}}),
    )?;
    let scanned = sources::scan(&ws.root, &repo)?;
    let loaded = sources::load(&ws.root)?;
    let bad_kind = error(sources::normalize(
        &repo,
        "workday",
        "X",
        &json!({}),
        "user",
        None,
        &Vec::new(),
    ));
    let configured = sources::configured_sources(&ws.root)?
        .into_iter()
        .map(|mut row| {
            if let Some(map) = row.as_object_mut() {
                map.remove("url");
            }
            row
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "scanned": scanned.iter().map(Posting::to_json).collect::<Vec<_>>(),
        "loaded_ids": loaded.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        "configured": configured,
        "bad_kind": bad_kind,
    }))
}

fn filter_postings() -> Vec<Posting> {
    vec![
        with(posting("p1", "Acme", "Senior Software Engineer"), |p| {
            p.seniority = 3;
            p.salary_max = Some(200000);
            p.region = "global".into();
        }),
        with(posting("p2", "Acme", "Engineering Manager"), |p| {
            p.seniority = 2
        }),
        with(
            posting("p3", "Globex", "Software Engineering Intern"),
            |p| p.seniority = 0,
        ),
        with(posting("p4", "Initech", "Staff Developer"), |p| {
            p.seniority = 4;
            p.region = "europe".into();
        }),
        with(posting("p5", "Umbrella", "Senior Engineer"), |p| {
            p.seniority = 3;
            p.remote = false;
            p.location = "Example City".into();
        }),
        with(posting("p6", "Hooli", "Senior Engineer"), |p| {
            p.seniority = 3;
            p.remote = false;
            p.location = "Sample Town".into();
        }),
        with(posting("p7", "Vandelay", "Senior Engineer"), |p| {
            p.seniority = 3;
            p.salary_max = Some(90000);
        }),
        with(posting("p8", "Pied", "Product Designer"), |p| {
            p.seniority = 3
        }),
        with(posting("p9", "Massive", "Junior Developer"), |p| {
            p.seniority = 1
        }),
        with(posting("p10", "EXCLUDED CO", "Senior Engineer"), |p| {
            p.seniority = 3
        }),
    ]
}

fn merged(base: &Value, updates: Value) -> Value {
    let mut out = base.clone();
    for (key, value) in updates.as_object().into_iter().flatten() {
        out[key] = value.clone();
    }
    out
}

fn reasons(postings: &[Posting], c: &Value) -> Result<Value> {
    let mut map = Map::new();
    for p in postings {
        map.insert(p.id.clone(), json!(criteria::reject_reason(p, c)?));
    }
    Ok(Value::Object(map))
}

fn criteria_filter(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "criteria")?;
    let defaults = criteria::load(&ws.root)?;
    write_yaml(
        &ws.root,
        "jobs/criteria.yaml",
        &json!({"min_seniority": 2, "exclude_companies": ["excluded co"], "regions_ok": ["global", "americas"], "locations_any": ["example city"], "min_salary": 150000}),
    )?;
    let loaded = criteria::load(&ws.root)?;
    let postings = filter_postings();
    let reason_map = reasons(&postings, &loaded)?;
    let (kept, rejected) = criteria::apply(&postings, &loaded)?;
    let remote_required = merged(
        &loaded,
        json!({"remote": "required", "regions_ok": null, "min_salary": null}),
    );
    let no_titles = merged(&defaults, json!({"titles_any": [], "titles_none": []}));
    Ok(json!({
        "defaults": defaults,
        "loaded": loaded,
        "reasons": reason_map,
        "kept": kept.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
        "rejected": rejected,
        "remote_required": reasons(&postings, &remote_required)?,
        "no_titles": reasons(&postings, &no_titles)?,
        "bad_regions": error(criteria::reject_reason(&postings[0], &merged(&loaded, json!({"regions_ok": [3]})))),
    }))
}

fn want_postings() -> Vec<Posting> {
    let skills = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    vec![
        with(posting("w1", "Alpha", "Senior Python Engineer"), |p| {
            p.seniority = 3;
            p.salary_max = Some(220000);
            p.skills = skills(&["python", "aws", "kafka"]);
        }),
        with(posting("w2", "Beta", "Staff Go Engineer"), |p| {
            p.seniority = 4;
            p.salary_max = Some(180000);
            p.skills = skills(&["go", "kubernetes"]);
        }),
        with(posting("w3", "Gamma", "Engineer"), |p| {
            p.seniority = 2;
            p.location = "Remote Golang shop".into();
        }),
        with(posting("w4", "Delta", "Senior Frontend Engineer"), |p| {
            p.seniority = 3;
            p.salary_max = Some(150000);
            p.skills = skills(&["react", "typescript"]);
            p.remote = false;
        }),
        with(posting("w5", "Epsilon", "Principal Rust Engineer"), |p| {
            p.seniority = 5;
            p.salary_max = Some(260000);
            p.skills = skills(&["rust", "distributed systems"]);
        }),
        with(posting("w6", "Zeta", "Senior Data Engineer"), |p| {
            p.seniority = 3;
            p.skills = skills(&["sql", "python", "kafka"]);
        }),
        with(posting("w7", "Eta", "Senior Go Engineer at Google"), |p| {
            p.seniority = 3;
            p.salary_max = Some(200000);
            p.skills = skills(&["go"]);
        }),
    ]
}

fn scores(
    root: &Path,
    postings: &[Posting],
    w: &Weights,
    prof: &Value,
    crit: &Value,
) -> Result<Value> {
    let tags = want::tag_words(root)?;
    let mut map = Map::new();
    for p in postings {
        let (score, features) = want::score(p, w, prof, crit, &tags)?;
        map.insert(p.id.clone(), json!([score, want::features_json(&features)]));
    }
    Ok(Value::Object(map))
}

fn want_learn(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "want")?;
    if std::env::var("CAREERBREW_RUST19_EVENT_CONTROL").as_deref() == Ok("1") {
        std::env::set_var("CAREERBREW_EVENTS", "on");
    }
    let root = &ws.root;
    write_yaml(
        root,
        "profile.yaml",
        &json!({"skills": {"Python": 4, "AWS": 3, "Go": 2}, "want_to_learn": ["Rust", "Kubernetes"], "current_level": 3, "remote_preference": "remote", "target_salary": 180000}),
    )?;
    write_yaml(root, "jobs/weights.yaml", &json!({"comp": 4, "unknown": 9}))?;
    let crit = merged(
        &criteria::load(root)?,
        json!({"company_scores": {"Alpha": 0.9, "Delta": 0.2}}),
    );
    let postings = want_postings();
    let w = want::weights(root)?;
    let before = scores(root, &postings, &w, &want::profile(root)?, &crit)?;
    want::rate(root, "w1", 5, &["python", "kafka"])?;
    want::rate(root, "w2", 2, &["go", "Golang"])?;
    want::rate(root, "w3", 1, &["go", "remote"])?;
    want::rate(root, "w4", 2, &["remote"])?;
    want::rate(root, "w5", 4, &["kafka"])?;
    want::rate(root, "w2", 1, &["go"])?;
    if std::env::var("CAREERBREW_RUST19_PLANT_WRITE").as_deref() == Ok("event-duplicate") {
        careerbrew_tutor::emit(
            root,
            "jobs.rate",
            vec![("status", careerbrew_vault::canon::Canon::Str("ok".into()))],
        )?;
    }
    let bad_rating = error(want::rate(root, "w1", 6, &[]));
    let tags = want::tag_words(root)?;
    let factors = postings
        .iter()
        .map(|p| (p.id.clone(), json!(want::tag_factor(p, &tags))))
        .collect::<Map<_, _>>();
    let prof = want::profile(root)?;
    let after = scores(root, &postings, &w, &prof, &crit)?;
    let agree = want::agreement(root, &postings, &w, &prof, &crit)?;
    let few = want::learn_weights(root, &postings[..3], &prof, &crit, 2000, 0.05)?;
    let learned = want::learn_weights(root, &postings, &prof, &crit, 400, 0.05)?;
    want::rate(root, "w6", 4, &[])?;
    want::rate(root, "w7", 3, &["google"])?;
    let relearned = want::learn_weights(root, &postings, &prof, &crit, 300, 0.2)?;
    Ok(json!({
        "weights": want::weights_json(&w),
        "before": before,
        "after": after,
        "tags": [tags.0, tags.1],
        "factors": factors,
        "ratings": want::ratings(root)?,
        "agreement": agree,
        "few": few,
        "learned": learned,
        "relearned": relearned,
        "weights_after": want::weights_json(&want::weights(root)?),
        "bad_rating": bad_rating,
        "spearman": [
            want::spearman(&[1.0, 2.0], &[2.0, 1.0]),
            want::spearman(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]),
            want::spearman(&[3.0, 1.0, 2.0, 2.0], &[1.0, 2.0, 3.0, 3.0]),
        ],
        "rank": want::rank(&[3.0, 1.0, 3.0, 2.0, 1.0]),
    }))
}

fn linkedin_alerts(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "linkedin")?;
    let repo = repo_root();
    sources::save(&ws.root, &[posting("keep-1", "Keep", "Engineer")])?;
    let alert = |id: &str, subject: &str| Alert {
        id: id.into(),
        subject: subject.into(),
    };
    let msgs = [
        alert(
            "m1",
            "\"python engineer\": Acme Corp - Senior Python Engineer and more",
        ),
        alert("m2", "Globex – Remote Staff Engineer"),
        alert("m3", "no separator here"),
        alert("m4", "  Initech - Senior Golang Developer  "),
    ];
    let built = linkedin::postings_from_alerts(&msgs, &sources::skill_vocab(&repo)?);
    let added = linkedin::add_alert_postings(&ws.root, &repo, &msgs)?;
    let again = linkedin::add_alert_postings(&ws.root, &repo, &msgs)?;
    let mut ids = sources::load(&ws.root)?
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    ids.sort();
    Ok(json!({
        "built": built.iter().map(Posting::to_json).collect::<Vec<_>>(),
        "added": added,
        "again": again,
        "ids": ids,
    }))
}

fn guard(group: &str) -> Result<()> {
    match machine::guard_decision(group, &machine::role()?) {
        Some(refusal) => Err(JobsError::new("RoleError", refusal)),
        None => Ok(()),
    }
}

fn corporate_guard(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "corporate")?;
    write_yaml(
        &ws.root,
        "profile.yaml",
        &json!({"name": "RUST19CANARYPERSON", "skills": {"python": 3}}),
    )?;
    sources::save(&ws.root, &[posting("c1", "Corp", "Engineer")])?;
    prep::add_story(&ws.root, "Corp story", &["impact"])?;
    let role = machine::role()?;
    let personal = GROUPS
        .iter()
        .map(|g| ((*g).to_owned(), json!(machine::guard_decision(g, &role))))
        .collect::<Map<_, _>>();
    let refused = error(machine::set_role("corporate", "").map_err(JobsError::from));
    let postings = read_json(&ws.root, "jobs/postings.json", json!({}))?;
    let kept = json!({
        "role": machine::role()?,
        "postings": postings.as_object().map_or(0, Map::len),
        "stories": prep::stories(&ws.root)?.len(),
    });
    let clean = Workspace::new(base, "corporate-clean")?;
    write_yaml(
        &clean.root,
        "profile.yaml",
        &json!({"name": "Synthetic", "skills": {"python": 3}}),
    )?;
    machine::set_role("corporate", "")?;
    let corporate = GROUPS
        .iter()
        .map(|g| ((*g).to_owned(), error(guard(g))))
        .collect::<Map<_, _>>();
    Ok(json!({
        "personal": personal,
        "refused": refused,
        "kept": kept,
        "corporate": corporate,
        "role": machine::role()?,
        "jobs_dir": clean.root.join("jobs").exists(),
    }))
}

fn call(cases: &mut Vec<Value>, name: &str, result: Result<Value>) {
    let value = match result {
        Ok(value) => value,
        Err(error) => json!({"error": error.class, "message": error.message}),
    };
    cases.push(json!({"id": name, "result": typed(&value)}));
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = careerbrew_tutor::root()?.join("rust19-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|selected| selected != *name) {
            continue;
        }
        let result = match *name {
            "helpers" => helpers(&base),
            "ats-scan" => ats_scan(&base),
            "criteria-filter" => criteria_filter(&base),
            "want-learn" => want_learn(&base),
            "linkedin-alerts" => linkedin_alerts(&base),
            "corporate-guard" => corporate_guard(&base),
            _ => Err(JobsError::value("unknown synthetic case")),
        };
        call(&mut cases, name, result);
    }
    let plant = std::env::var("CAREERBREW_RUST19_PLANT_WRITE").ok();
    if selected == Some("duplicate-write-control") {
        std::env::set_var("CAREERBREW_HOME", base.join("want/data"));
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("want/keys/backup.key"));
        let root = base.join("want/data");
        let current = read_yaml(&root, "jobs/weights.yaml", json!({}))?;
        write_yaml(&root, "jobs/weights.yaml", &current)?;
    }
    if plant.as_deref() == Some("extra") {
        careerbrew_tutor::write_json(
            &careerbrew_tutor::root()?,
            "planted-extra.json",
            &json!({"synthetic": true}),
        )?;
    }
    if plant.as_deref() == Some("missing") && selected == Some("ats-scan") {
        std::fs::remove_file(base.join("ats/data/jobs/postings.json"))?;
    }
    Ok(format!("{}\n", compact(&json!({"cases": cases}))?))
}
