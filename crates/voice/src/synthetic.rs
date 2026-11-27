//! Synthetic rust-23 workload: the same cases as `tests/oracles/rust23.py`.
//!
//! Fake transports, fake speech engines and a fixture server on 127.0.0.1
//! and a free OS port only. No real model, network host or owner file.

use crate::http::{closed_port, Fixture};
use crate::judge::{self, Agreement, Judge, Label, Verdict};
use crate::models::{self, route_report, Route, Transport, TransportError};
use crate::voice::{self, TutorPractice};
use crate::{clean_report, dumps, expand_user, pure_path, sha_full, typed, Error, Result};
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const CASES: &[&str] = &[
    "routes-config",
    "allowed-routes",
    "complete-routing",
    "judge-text",
    "judge-cache",
    "http-transports",
    "labels",
    "agreement-run",
    "voice-transcribe",
    "voice-answer",
];
const CANARY: &str = "RUST23CANARYTERM";
const TOKEN: &str = "rust23-synthetic-token";

const SYNTH_ROUTES: &str = "routes:
  - name: local
    provider: ollama
    base_url: http://localhost:11434/v1
    model: synth-local:1b
    private_data: true
  - name: lan
    provider: ollama
    base_url: http://192.0.2.5:11434/v1
    model: synth-lan
    private_data: true
  - name: loop6
    provider: openai-compatible
    base_url: http://[::1]:8080/v1
    model: 8
    private_data: yes
  - name: hosted
    provider: synthetic-hosted
    base_url: https://${RUST23_ACCOUNT}.example.invalid/$RUST23_MISSING/v1
    model: synth-hosted
    api_key_env: RUST23_TOKEN
    private_data: false
judge:
  route: local
  answer_route: local
";

fn plant(value: &str) -> bool {
    std::env::var("CAREERBREW_RUST23_PLANT_WRITE").as_deref() == Ok(value)
}

fn voice_fixtures() -> PathBuf {
    crate::repo_root().join("tests/fixtures/voice")
}

fn outcome<T: Into<Value>>(result: Result<T>) -> Value {
    match result {
        Ok(value) => json!({"ok": value.into()}),
        Err(error) => json!({"error": error.class, "message": error.message}),
    }
}

struct Workspace {
    base: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    config: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        let bin = base.join("bin");
        let config = base.join("config");
        std::fs::create_dir_all(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(&bin)?;
        std::fs::create_dir_all(&config)?;
        for key in [
            "CLOUDFLARE_ACCOUNT_ID",
            "EVALS_LLM_URL",
            "EVALS_LABELS",
            "EVALS_CACHE",
            "RUST23_TOKEN",
        ] {
            std::env::remove_var(key);
        }
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "on");
        std::env::set_var("CAREERBREW_ENCRYPT", "on");
        std::env::set_var("CAREERBREW_LLM", "on");
        std::env::set_var("CAREERBREW_NOW", "2026-10-19T13:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("PATH", &bin);
        std::env::set_var("NO_PROXY", "*");
        std::env::set_var("no_proxy", "*");
        std::env::set_var("TZ", "UTC");
        models::set_config_dir(Some(config.clone()));
        Ok(Self {
            base,
            root,
            bin,
            config,
        })
    }

    fn models_yaml(&self, body: &str) -> Result<()> {
        std::fs::write(self.config.join("models.yaml"), body)?;
        Ok(())
    }

    fn script(&self, name: &str, body: &str) -> Result<PathBuf> {
        let path = self.bin.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }

    fn base_text(&self) -> String {
        self.base.to_string_lossy().into_owned()
    }

    fn settings(&self, values: Value) -> Result<()> {
        careerbrew_tutor::write_yaml_abs(&self.root.join("settings.yaml"), &values)?;
        Ok(())
    }
}

fn events(ws: &Workspace) -> Result<Value> {
    let path = ws.root.join("logs/events.jsonl");
    if !path.exists() {
        return Ok(json!([]));
    }
    let body = std::fs::read_to_string(path)?;
    let mut rows = Vec::new();
    for line in careerbrew_policy::text::splitlines(&body) {
        let mut row: Value =
            serde_json::from_str(line).map_err(|_| Error::value("bad event row"))?;
        if let Some(map) = row.as_object_mut() {
            map.remove("ts");
        }
        rows.push(row);
    }
    Ok(Value::Array(rows))
}

// --- fake transports ------------------------------------------------------------

type Step = std::result::Result<Value, TransportError>;

#[derive(Clone, Default)]
struct Fake {
    state: Rc<RefCell<(VecDeque<Step>, Vec<Value>)>>,
}

impl Fake {
    fn new(script: Vec<Step>) -> Self {
        Self {
            state: Rc::new(RefCell::new((script.into(), Vec::new()))),
        }
    }

    fn seen(&self) -> Vec<Value> {
        self.state.borrow().1.clone()
    }
}

impl Transport for Fake {
    fn call(&mut self, payload: &Value, url: &str) -> std::result::Result<Value, TransportError> {
        let mut seen = payload.clone();
        let messages = payload.get("messages").cloned().unwrap_or(Value::Null);
        seen["messages"] = json!(crate::sha16(&dumps(&messages)));
        let mut state = self.state.borrow_mut();
        state.1.push(json!({"payload": seen, "url": url}));
        state
            .0
            .pop_front()
            .unwrap_or_else(|| Err(TransportError::Index("pop from empty list".into())))
    }
}

struct Scripted(fn(&str) -> String);

impl Transport for Scripted {
    fn call(&mut self, payload: &Value, _url: &str) -> std::result::Result<Value, TransportError> {
        let content = payload["messages"]
            .as_array()
            .and_then(|items| items.last())
            .and_then(|item| item["content"].as_str())
            .unwrap_or("")
            .to_owned();
        Ok(Value::String((self.0)(&content)))
    }
}

fn url_error(reason: &str) -> Step {
    Err(TransportError::Url {
        reason: reason.into(),
        timeout: false,
    })
}

fn completion(content: Value, finish: Option<&str>) -> Vec<u8> {
    let mut choice = json!({"message": {"content": content}});
    if let Some(finish) = finish {
        choice["finish_reason"] = json!(finish);
    }
    serde_json::to_vec(&json!({"choices": [choice]})).expect("JSON encodes")
}

fn seen_report(fixture: &Fixture) -> Value {
    Value::Array(
        fixture
            .seen()
            .into_iter()
            .map(|seen| {
                json!({
                    "path": seen.path,
                    "auth": seen.authorization.as_deref() == Some(&format!("Bearer {TOKEN}")),
                    "auth_present": seen.authorization.is_some(),
                    "content_type": seen.content_type,
                    "body": seen.body,
                })
            })
            .collect(),
    )
}

// --- cases ------------------------------------------------------------------------

fn routes_list() -> Result<Value> {
    Ok(Value::Array(
        models::routes()?.iter().map(route_report).collect(),
    ))
}

fn routes_config(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "routes-config")?;
    models::set_config_dir(None);
    let real_unset = routes_list()?;
    std::env::set_var("CLOUDFLARE_ACCOUNT_ID", "synthacct");
    let real_set = routes_list()?;
    std::env::remove_var("CLOUDFLARE_ACCOUNT_ID");
    models::set_config_dir(Some(ws.config.clone()));
    ws.models_yaml(SYNTH_ROUTES)?;
    std::env::set_var("RUST23_ACCOUNT", "acct-7");
    let synth = routes_list()?;
    std::env::remove_var("RUST23_ACCOUNT");
    ws.models_yaml("judge: {route: local}\n")?;
    let no_routes = routes_list()?;
    std::fs::remove_file(ws.config.join("models.yaml"))?;
    let missing_file = routes_list()?;
    ws.models_yaml("routes:\n  - provider: ollama\n")?;
    let nameless = outcome(routes_list());
    let mut hosts = Map::new();
    for url in [
        "http://LOCALHOST:1/v1",
        "http://127.0.0.1/v1",
        "http://[::1]/v1",
        "http://user@localhost:9/",
        "localhost:11434/v1",
        "",
        "http://localhost.example.invalid/v1",
        "https://127.0.0.2/v1",
    ] {
        let route = Route::new("h", "p", "m", true, url);
        hosts.insert(url.into(), json!(models::on_this_machine(&route)));
    }
    Ok(json!({
        "real_unset": real_unset, "real_set": real_set, "synthetic": synth,
        "no_routes": no_routes, "missing_file": missing_file, "nameless": nameless, "hosts": hosts,
    }))
}

fn allowed_names(sensitivity: &str) -> Value {
    outcome(
        models::allowed_routes(sensitivity)
            .map(|routes| routes.into_iter().map(|r| r.name).collect::<Vec<_>>()),
    )
}

fn allowed_report() -> Value {
    let mut out = Map::new();
    for s in ["public", "personal", "business", "bogus"] {
        out.insert(s.into(), allowed_names(s));
    }
    Value::Object(out)
}

fn allowed_routes(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "allowed-routes")?;
    ws.models_yaml(SYNTH_ROUTES)?;
    let personal = allowed_report();
    careerbrew_tutor::write_yaml_abs(
        &ws.root.join("plugins/rust23plug/careerbrew-plugin.yaml"),
        &json!({"name": "rust23plug", "version": "0.1.0", "policy": {"allowed_routes": ["local", "hosted", "lan"]}}),
    )?;
    careerbrew_tutor::write_json_abs(
        &ws.root.join("plugins.lock.json"),
        &json!({"plugins": {"rust23plug": {"origin": "plugin:rust23plug"}}}),
    )?;
    let narrowed = allowed_report();
    careerbrew_tutor::write_yaml_abs(
        &ws.root.join("machine.yaml"),
        &json!({"role": "corporate", "label": "synthetic"}),
    )?;
    let corporate = allowed_report();
    Ok(json!({"personal": personal, "plugin": narrowed, "corporate": corporate}))
}

fn complete_routing(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "complete-routing")?;
    ws.models_yaml(SYNTH_ROUTES)?;
    careerbrew_tutor::write_text_abs(&ws.root.join("sensitive-terms.txt"), &format!("{CANARY}\n"))?;
    let mut out = Map::new();
    let mut run = |label: &str, prompt: &str, sensitivity: &str, script: Vec<Step>| {
        let mut fake = Fake::new(script);
        let fallback_text = format!("fallback for {label}");
        let fallback = move || fallback_text.clone();
        let result = models::complete(prompt, sensitivity, &fallback, &mut fake)
            .map(|(text, name)| json!([text, name]));
        out.insert(
            label.into(),
            json!({"result": outcome(result), "seen": fake.seen()}),
        );
    };
    let ok = |text: &str| Ok(json!(text));
    std::env::set_var("CAREERBREW_LLM", "off");
    run("llm-off", "synthetic prompt", "public", vec![ok("never")]);
    std::env::set_var("CAREERBREW_LLM", "on");
    run(
        "first-answers",
        "synthetic prompt",
        "public",
        vec![ok("local answer")],
    );
    run(
        "unavailable-then-next",
        "synthetic prompt",
        "public",
        vec![
            url_error("refused"),
            Err(TransportError::Os("down".into())),
            Err(TransportError::Value("bad".into())),
            ok("hosted answer"),
        ],
    );
    run(
        "all-unavailable",
        "synthetic prompt",
        "public",
        vec![
            Err(TransportError::Key("choices".into())),
            Err(TransportError::Timeout("slow".into())),
            Err(TransportError::Http(503, "busy".into())),
            Err(TransportError::Os("none".into())),
        ],
    );
    let down = || Err(TransportError::Os("down".into()));
    let leaky = format!("about {CANARY} plans");
    run(
        "privacy-block",
        &leaky,
        "public",
        vec![down(), down(), down(), ok("never")],
    );
    run(
        "personal-private-only",
        &leaky,
        "personal",
        vec![url_error("no"), ok("lan answer"), ok("unused")],
    );
    run("bad-sensitivity", "x", "secret", vec![]);
    run(
        "uncaught-runtime",
        "synthetic prompt",
        "public",
        vec![Err(TransportError::Runtime("boom".into()))],
    );
    out.insert("events".into(), events(&ws)?);
    Ok(Value::Object(out))
}

fn case(value: Value) -> Value {
    value
}

fn verdict_pair(v: &Verdict) -> Value {
    json!([v.verdict, v.evidence])
}

fn judge_text(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "judge-text")?;
    ws.models_yaml(SYNTH_ROUTES)?;
    let replies = vec![
        r#"{"verdict": "PASS", "evidence": "names the   command"}"#.to_owned(),
        r#"<think>{"verdict": "FAIL"}</think>{"verdict": "fail", "evidence": "missing step"}"#
            .to_owned(),
        r#"Sure: {"verdict": "UNKNOWN"} trailing"#.to_owned(),
        r#"{"verdict": "MAYBE"} but PASS overall"#.to_owned(),
        r#"{"verdict": PASS}"#.to_owned(),
        "no verdict here".to_owned(),
        "the answer FAILs; verdict: FAIL.".to_owned(),
        r#"{"verdict": " pass ", "evidence": null}"#.to_owned(),
        r#"{"verdict": "PASS", "evidence": 12}"#.to_owned(),
        r#"{"verdict": "PASS", "evidence": true}"#.to_owned(),
        format!(
            r#"{{"verdict": "PASS", "evidence": "{}"}}"#,
            "long ".repeat(80)
        ),
        "[1, 2] PASS".to_owned(),
        r#"{"evidence": "only evidence"} UNKNOWN"#.to_owned(),
        "<think>PASS</think>".to_owned(),
        "PASSING grade".to_owned(),
        "verdict: Pass".to_owned(),
        r#"{"verdict": "PASS", "evidence": "aéb"}"#.to_owned(),
        format!("{} UNKNOWN {}", "x".repeat(10), "y\n".repeat(200)),
    ];
    let parsed = replies
        .iter()
        .map(|r| verdict_pair(&judge::parse_verdict(r)))
        .collect::<Vec<_>>();
    let v = Verdict::new;
    let case_verdicts = json!([
        judge::case_verdict(&[]),
        judge::case_verdict(&[v("PASS", ""), v("PASS", "")]),
        judge::case_verdict(&[v("PASS", ""), v("UNKNOWN", "")]),
        judge::case_verdict(&[v("UNKNOWN", ""), v("FAIL", "")]),
    ]);
    let cases = vec![
        case(
            json!({"id": "c1", "skill": "rust23-none", "input": "plan my week", "expect": ["names a command", "asks one question"], "should": "use"}),
        ),
        case(
            json!({"id": "c2", "skill": "rust23-none", "input": "tell a joke", "expect": "refuses politely", "should": "refrain"}),
        ),
        case(json!({"id": "c3", "cli": ["status"], "expect": "x"})),
        case(
            json!({"id": "c4", "skill": "rust23-none", "input": "number criteria", "expect": [1, 2.5, true]}),
        ),
        case(json!({"id": "c5", "skill": "rust23-none", "input": "no criteria"})),
    ];
    let mut criteria = Map::new();
    let mut digests = Map::new();
    for c in &cases {
        let id = c["id"].as_str().unwrap_or("").to_owned();
        criteria.insert(id.clone(), json!(judge::criteria(c)));
        digests.insert(id, json!(judge::expect_digest(c)));
    }
    let llm_ids = judge::llm_cases(&cases)
        .iter()
        .map(|c| c["id"].clone())
        .collect::<Vec<_>>();
    let judge_msgs = json!([
        judge::judge_messages(&cases[0], "names a command", "run careerbrew plan")?,
        judge::judge_messages(&cases[1], "refuses politely", "no")?,
    ]);
    let answer_msgs = judge::answer_messages(&cases[0])?;
    let real_skill = judge::skill_text("careerbrew-teacher")?;
    let mut skill_names = Map::new();
    for name in ["", ".", "..", "a/b", "a\\b", "a\0b", "rust23-none"] {
        skill_names.insert(crate::py_repr_str(name), outcome(judge::skill_text(name)));
    }
    let real_answer =
        judge::answer_messages(&json!({"skill": "careerbrew-teacher", "input": "teach me"}))?;
    let mut routes = Map::new();
    let route_of = |name: &str| judge::route(name).map(|r| route_report(&r));
    for name in ["local", "lan", "nope"] {
        routes.insert(name.into(), outcome(route_of(name)));
    }
    std::env::set_var("EVALS_LLM_URL", " http://127.0.0.1:9/v1 ");
    routes.insert("override-local".into(), outcome(route_of("hosted")));
    std::env::set_var("EVALS_LLM_URL", "https://judge.example.invalid/v1");
    routes.insert("override-remote".into(), outcome(route_of("local")));
    std::env::set_var("EVALS_LLM_URL", "   ");
    routes.insert("override-blank".into(), outcome(route_of("lan")));
    std::env::remove_var("EVALS_LLM_URL");
    let local = judge::route("local")?;
    let lan = judge::route("loop6")?;
    let payloads = json!([
        Judge::payload(&local, &answer_msgs, judge::ANSWER_TOKENS),
        Judge::payload(&lan, &judge_msgs[0], judge::JUDGE_TOKENS),
    ]);
    let row = |k: String, o: &str, j: &str| (k, o.to_owned(), j.to_owned());
    let rows20 = (0..20)
        .map(|i| row(format!("k{i}"), "pass", "pass"))
        .collect::<Vec<_>>();
    let with = |n: usize, extra: Vec<(String, String, String)>, stale: &[&str]| Agreement {
        rows: rows20[..n].iter().cloned().chain(extra).collect(),
        stale: stale.iter().map(|s| (*s).to_owned()).collect(),
    };
    let agreements = vec![
        ("empty", Agreement::default()),
        (
            "few",
            Agreement {
                rows: vec![
                    row("a".into(), "pass", "pass"),
                    row("b".into(), "fail", "pass"),
                ],
                stale: vec!["s1".into()],
            },
        ),
        (
            "keep",
            with(
                17,
                vec![
                    row("x1".into(), "pass", "fail"),
                    row("x2".into(), "fail", "pass"),
                    row("x3".into(), "fail", "unknown"),
                ],
                &[],
            ),
        ),
        (
            "pivot",
            with(
                13,
                (0..7)
                    .map(|i| row(format!("y{i}"), "pass", "fail"))
                    .collect(),
                &[],
            ),
        ),
        (
            "calibrate",
            with(
                15,
                (0..5)
                    .map(|i| row(format!("z{i}"), "pass", "fail"))
                    .collect(),
                &["s1", "s2"],
            ),
        ),
        (
            "two-thirds",
            with(
                14,
                (0..7)
                    .map(|i| row(format!("w{i}"), "pass", "fail"))
                    .collect(),
                &[],
            ),
        ),
    ];
    let jdg = Judge::new(
        local,
        lan,
        Box::new(Scripted(|_| String::new())),
        ws.base.join("cache"),
    );
    let mut report = Map::new();
    for (key, a) in agreements {
        report.insert(
            key.into(),
            json!({"n": a.n(), "agree": a.agree(), "trusted": a.trusted(), "decision": a.decision(), "lines": judge::agreement_lines(&a, &jdg)}),
        );
    }
    Ok(json!({
        "parsed": parsed,
        "case_verdicts": case_verdicts,
        "digest": [judge::digest(""), judge::digest("héllo"), judge::digest("abc")],
        "criteria": criteria,
        "expect_digests": digests,
        "llm_ids": llm_ids,
        "judge_messages": judge_msgs,
        "answer_messages": answer_msgs,
        "real_skill_sha": sha_full(real_skill.as_bytes()),
        "real_answer_sha": sha_full(dumps(&real_answer).as_bytes()),
        "skill_names": skill_names,
        "routes": routes,
        "payloads": payloads,
        "agreements": report,
    }))
}

fn cache_listing(folder: &Path) -> Result<Value> {
    if !folder.exists() {
        return Ok(json!([]));
    }
    let mut paths = std::fs::read_dir(folder)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
        out.push(json!([
            path.file_name().unwrap_or_default().to_string_lossy(),
            format!("0o{mode:o}"),
            std::fs::read_to_string(&path)?,
        ]));
    }
    Ok(Value::Array(out))
}

fn grades(verdicts: &[Verdict]) -> Value {
    Value::Array(verdicts.iter().map(verdict_pair).collect())
}

fn cache_grader(content: &str) -> String {
    if content.starts_with("You grade") {
        if content.contains("names") {
            return r#"{"verdict": "PASS", "evidence": "synthetic"}"#.into();
        }
        return r#"{"verdict": "FAIL", "evidence": "synthetic"}"#.into();
    }
    "<think>plan</think>  Run `careerbrew plan` and name it.  ".into()
}

fn judge_cache(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "judge-cache")?;
    ws.models_yaml(&SYNTH_ROUTES.replace("  answer_route: local\n", "  answer_route: loop6\n"))?;
    let cache = ws.base.join("cache");
    let mut j = Judge::from_config(Some(Box::new(Scripted(cache_grader))), Some(cache.clone()))?;
    let case = json!({"id": "c1", "skill": "rust23-none", "input": "plan my week", "expect": ["names a command", "asks one question"]});
    let before = j.cached_answer(&case)?;
    let ans = j.answer(&case)?;
    let first_grades = grades(&j.grade(&case, &ans)?);
    let calls_first = j.calls;
    let ans2 = j.answer(&case)?;
    let second_grades = grades(&j.grade(&case, &ans2)?);
    let calls_second = j.calls;
    let after = j.cached_answer(&case)?;
    let listing = cache_listing(&cache)?;
    let mut entries = std::fs::read_dir(&cache)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    std::fs::write(&entries[0], "{broken")?;
    j.grade(&case, &ans)?;
    j.answer(&case)?;
    let calls_repaired = j.calls;

    let error_cases: Vec<(&str, &str, Step)> = vec![
        ("http-404-ollama", "local", Err(TransportError::Http(404, "nf".into()))),
        ("http-404-other", "loop6", Err(TransportError::Http(404, "nf".into()))),
        ("http-500", "local", Err(TransportError::Http(500, "err".into()))),
        ("timeout", "local", Err(TransportError::Timeout("slow".into()))),
        (
            "url-timeout",
            "local",
            Err(TransportError::Url {
                reason: "slow".into(),
                timeout: true,
            }),
        ),
        ("url-refused", "local", url_error("refused")),
        ("os-error", "local", Err(TransportError::Os("no route".into()))),
        ("key-error", "local", Err(TransportError::Key("choices".into()))),
        ("index-error", "local", Err(TransportError::Index("0".into()))),
        ("type-error", "local", Err(TransportError::Type("x".into()))),
        ("value-error", "local", Err(TransportError::Value("x".into()))),
        ("non-text", "local", Ok(json!(5))),
        ("llm-error", "local", Err(TransportError::Llm("model completion is incomplete or has no completion reason; rerun with a complete response".into()))),
    ];
    let mut errors = Map::new();
    for (n, (label, name, step)) in error_cases.into_iter().enumerate() {
        let fake = Fake::new(vec![step]);
        let mut jj = Judge::new(
            judge::route(name)?,
            judge::route(name)?,
            Box::new(fake.clone()),
            ws.base.join("error-cache"),
        );
        let result =
            outcome(jj.answer(&json!({"skill": "rust23-none", "input": format!("error {n}")})));
        let seen = fake.seen();
        let keys = seen
            .first()
            .and_then(|s| s["payload"].as_object())
            .map(|map| map.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        errors.insert(
            label.into(),
            json!({"result": result, "calls": jj.calls, "seen_keys": keys, "url": seen.first().map(|s| s["url"].clone())}),
        );
    }
    let mut inside = Judge::new(
        judge::route("local")?,
        judge::route("local")?,
        Box::new(Fake::new(vec![Ok(json!("x"))])),
        ws.root.join("cache"),
    );
    let inside_result = outcome(inside.answer(&json!({"skill": "rust23-none", "input": "inside"})));
    let error_cache = ws.base.join("error-cache");
    let error_count = if error_cache.exists() {
        std::fs::read_dir(&error_cache)?.count()
    } else {
        0
    };
    Ok(json!({
        "routes": [j.answer_route.name, j.judge_route.name],
        "cached_before": before,
        "answer": ans,
        "grades": first_grades,
        "calls_first": calls_first,
        "answer_again": ans2,
        "grades_again": second_grades,
        "calls_second": calls_second,
        "cached_after": after,
        "listing": listing,
        "calls_repaired": calls_repaired,
        "errors": errors,
        "error_cache": error_count,
        "inside_data_root": inside_result,
    }))
}

fn http_transports(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "http-transports")?;
    let replies = vec![
        (200, completion(json!("HTTP answer one"), Some("stop"))),
        (200, completion(json!("cut off"), Some("length"))),
        (200, completion(json!("no reason"), None)),
        (200, br#"{"choices": ["x"]}"#.to_vec()),
        (200, b"not json".to_vec()),
        (200, b"{}".to_vec()),
        (404, br#"{"error": "missing"}"#.to_vec()),
        (500, b"{}".to_vec()),
        (200, completion(json!(5), Some("stop"))),
        (200, br#"{"choices": [{"finish_reason": "stop"}]}"#.to_vec()),
        (
            200,
            br#"{"choices": [{"message": {"content": "complete via http"}}]}"#.to_vec(),
        ),
        (503, b"{}".to_vec()),
    ];
    let mut fixture = Fixture::start(replies)?;
    let port = fixture.port.to_string();
    let run = || -> Result<(Vec<Value>, usize, Value, Value)> {
        ws.models_yaml(&SYNTH_ROUTES.replace(
            "    model: synth-local:1b\n",
            "    model: synth-local:1b\n    api_key_env: RUST23_TOKEN\n",
        ))?;
        std::env::set_var("RUST23_TOKEN", TOKEN);
        std::env::set_var("EVALS_LLM_URL", fixture.url());
        let mut j = Judge::from_config(None, Some(ws.base.join("cache")))?;
        let mut results = Vec::new();
        for n in 0..10 {
            results.push(outcome(j.answer(
                &json!({"skill": "rust23-none", "input": format!("http {n}")}),
            )));
        }
        std::env::remove_var("EVALS_LLM_URL");
        let cached = std::fs::read_dir(ws.base.join("cache"))?.count();
        ws.models_yaml(&format!(
            "routes:\n  - name: fixture\n    provider: ollama\n    base_url: {}/\n    model: synth\n    private_data: true\n",
            fixture.url()
        ))?;
        let fallback = || "fallback".to_owned();
        let complete = || {
            models::complete(
                "synthetic prompt",
                "personal",
                &fallback,
                &mut models::HttpTransport,
            )
            .map(|(text, name)| json!([text, name]))
        };
        let ok = outcome(complete());
        let down = outcome(complete());
        Ok((results, cached, ok, down))
    };
    let ran = run();
    let seen = seen_report(&fixture);
    fixture.close();
    let (mut results, cached, complete_ok, complete_down) = ran?;
    let closed = closed_port()?;
    std::env::set_var("EVALS_LLM_URL", format!("http://127.0.0.1:{closed}/v1"));
    ws.models_yaml(SYNTH_ROUTES)?;
    let mut refused = outcome(
        Judge::from_config(None, Some(ws.base.join("cache2")))
            .and_then(|mut j| j.answer(&json!({"skill": "rust23-none", "input": "refused"}))),
    );
    if let Some(message) = refused.get("message").and_then(Value::as_str) {
        let cut = message.replace(&closed.to_string(), "<port>");
        let cut = cut.split(" (").next().unwrap_or("").to_owned();
        refused["message"] = json!(cut);
    }
    std::env::remove_var("EVALS_LLM_URL");
    for row in &mut results {
        if let Some(message) = row.get("message").and_then(Value::as_str) {
            let replaced = message.replace(&port, "<port>");
            row["message"] = json!(replaced);
        }
    }
    Ok(json!({
        "results": results, "cached": cached, "complete_ok": complete_ok,
        "complete_down": complete_down, "seen": seen, "refused": refused, "events": events(&ws)?,
    }))
}

fn labels_pair(result: (Vec<Label>, Vec<String>)) -> Value {
    json!([result.0, result.1])
}

fn labels(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "labels")?;
    let file = ws.base.join("labels/labels.jsonl");
    std::env::set_var("EVALS_LABELS", &file);
    let cases = vec![
        json!({"id": "c1", "skill": "s", "input": "one", "expect": ["a", "b"]}),
        json!({"id": "c2", "skill": "s", "input": "two", "expect": "c"}),
        json!({"id": "c3", "cli": ["x"], "expect": "d"}),
        json!({"id": "c4", "skill": "s", "input": "four", "expect": ["e"]}),
    ];
    let missing = labels_pair(judge::read_labels(None, None)?);
    let added = json!([
        judge::add_label(&cases[0], "answer one é", "pass", None)?,
        judge::add_label(&cases[1], "answer two", "fail", None)?,
    ]);
    let replaced = judge::add_label(&cases[0], "answer one v2", "fail", None)?;
    let bad_label = outcome(judge::add_label(&cases[1], "x", "maybe", None));
    let text_after_add = std::fs::read_to_string(&file)?;
    let mode = std::fs::metadata(&file)?.permissions().mode() & 0o777;
    let first = text_after_add.lines().next().unwrap_or("").to_owned();
    let good: Value = serde_json::from_str(&first).map_err(|_| Error::value("bad label"))?;
    let with = |updates: Value| {
        let mut row = good.clone();
        for (key, value) in updates.as_object().into_iter().flatten() {
            row[key] = value.clone();
        }
        serde_json::to_string(&row).expect("JSON encodes")
    };
    let lines = [
        first.clone(),
        String::new(),
        "not json".into(),
        "[1, 2]".into(),
        serde_json::to_string(&json!({"case": "c2"})).expect("JSON encodes"),
        with(json!({"case": "c4", "label": "maybe"})),
        with(json!({"case": "c4", "answer": "edited"})),
        with(json!({"case": "c9"})),
        first.clone(),
        with(json!({"case": "c3"})),
    ];
    std::fs::write(&file, format!("{}\n", lines.join("\n")))?;
    let read_all = judge::read_labels(None, None)?;
    let read_skip = judge::read_labels(None, Some("c4"))?;
    let checked = judge::check_labels(&read_all.0, &cases);
    let blocked = outcome(judge::add_label(&cases[0], "x", "pass", None));
    std::fs::write(
        &file,
        format!(
            "{}\n",
            [&lines[0], &lines[5], &lines[6]]
                .map(String::as_str)
                .join("\n")
        ),
    )?;
    let fixed = judge::add_label(&cases[3], "answer four", "pass", None)?;
    let final_text = std::fs::read_to_string(&file)?;
    let final_rows = judge::read_labels(None, None)?.0;
    let keys = |rows: Vec<(String, Label)>| {
        let mut keys = rows.into_iter().map(|(key, _)| key).collect::<Vec<_>>();
        keys.sort();
        keys
    };
    let current = keys(judge::current_labels(&final_rows, &cases));
    let mut changed_first = cases[0].clone();
    changed_first["expect"] = json!(["a", "changed"]);
    let changed = vec![changed_first, cases[1].clone(), cases[3].clone()];
    let stale_current = keys(judge::current_labels(&final_rows, &changed));
    let next = judge::next_unlabeled(&final_rows, &changed).map(|c| c["id"].clone());
    let none_left = judge::next_unlabeled(&final_rows, &[cases[0].clone(), cases[3].clone()]);
    std::env::set_var("EVALS_LABELS", ws.root.join("labels.jsonl"));
    let inside = outcome(judge::read_labels(None, None).map(labels_pair));
    let inside_add = outcome(judge::add_label(&cases[0], "x", "pass", None));
    std::env::remove_var("EVALS_LABELS");
    Ok(json!({
        "missing": missing,
        "added": added,
        "replaced": replaced,
        "bad_label": bad_label,
        "text_after_add": text_after_add,
        "mode": format!("0o{mode:o}"),
        "read_all": labels_pair(read_all),
        "read_skip": labels_pair(read_skip),
        "checked": checked,
        "blocked": blocked,
        "fixed": fixed,
        "final_text": final_text,
        "current": current,
        "stale_current": stale_current,
        "next": next,
        "none_left": none_left,
        "inside": inside,
        "inside_add": inside_add,
    }))
}

fn agreement_grader(content: &str) -> String {
    if content.starts_with("You grade") {
        if content.contains("fails on purpose") {
            return r#"{"verdict": "FAIL", "evidence": "synthetic miss"}"#.into();
        }
        if content.contains("request 5") {
            return "UNKNOWN".into();
        }
        return r#"{"verdict": "PASS", "evidence": "synthetic hit"}"#.into();
    }
    format!("answer for {content}")
}

fn agreement_run(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "agreement-run")?;
    ws.models_yaml(SYNTH_ROUTES)?;
    let mut cases = (0..22)
        .map(|i| {
            json!({
                "id": format!("r{i:02}"), "skill": "rust23-none", "input": format!("request {i}"),
                "expect": ["names a command", if i % 3 != 0 { "keeps it short" } else { "fails on purpose" }],
                "suite": if i % 4 != 0 { "regression" } else { "capability" },
            })
        })
        .collect::<Vec<_>>();
    cases.push(json!({"id": "cli-case", "cli": ["status"], "expect": "x"}));
    std::env::set_var("EVALS_LABELS", ws.base.join("labels.jsonl"));
    let mut j = Judge::new(
        judge::route("local")?,
        judge::route("lan")?,
        Box::new(Scripted(agreement_grader)),
        ws.base.join("cache"),
    );
    let mut progress = Vec::new();
    let mut few = Vec::new();
    for (i, c) in cases.iter().enumerate().take(3) {
        let answer = j.answer(c)?;
        few.push(judge::add_label(
            c,
            &answer,
            if i % 3 != 0 { "pass" } else { "fail" },
            None,
        )?);
    }
    let labels = judge::read_labels(None, None)?.0;
    let untrusted = judge::run(&cases, &mut j, &labels, &mut |line| progress.push(line))?;
    for (i, c) in cases.iter().enumerate().take(22).skip(3) {
        let answer = j.answer(c)?;
        let label = if i % 3 != 0 && i != 5 { "pass" } else { "fail" };
        judge::add_label(c, &answer, label, None)?;
    }
    let labels = judge::read_labels(None, None)?.0;
    let mut stale_first = cases[1].clone();
    stale_first["expect"] = json!(["changed"]);
    let stale_cases = std::iter::once(stale_first)
        .chain(cases[2..].iter().cloned())
        .collect::<Vec<_>>();
    let trusted = judge::run(&stale_cases, &mut j, &labels, &mut |line| {
        progress.push(line)
    })?;
    let answer = j.answer(&cases[1])?;
    let show = judge::show_text(&cases[1], &answer, &j, &labels, &cases)?;
    let show_new = judge::show_text(&cases[1], "different answer", &j, &labels, &stale_cases)?;
    std::env::remove_var("EVALS_LABELS");
    Ok(json!({
        "few": few, "untrusted": [untrusted.0, untrusted.1], "trusted": [trusted.0, trusted.1],
        "progress": progress, "calls": j.calls, "show": show, "show_new": show_new,
    }))
}

fn voice_transcribe(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "voice-transcribe")?;
    let fixtures = voice_fixtures();
    let log = ws.base.join("engine.log");
    let log_text = log.to_string_lossy().into_owned();
    let audio = ws.base.join("answer.wav");
    std::fs::write(
        &audio,
        std::fs::read(fixtures.join("behavioral-answer.wav"))?,
    )?;
    let ogg = ws.base.join("answer.ogg");
    std::fs::write(&ogg, b"OggS synthetic")?;
    let wsb = ws.base_text();
    let mut out = Map::new();
    let mut put = |key: &str, value: Value| {
        out.insert(key.into(), value);
    };
    put("no-engine-cmd", json!(voice::stt_command()?));
    put("no-engine", outcome(voice::transcribe(&audio)));
    ws.settings(json!({"stt_model": "~/models/ggml base.bin"}))?;
    put("model-without-whisper", json!(voice::stt_command()?));
    ws.script(
        "whisper-cli",
        &format!("/bin/echo \"$@\" > \"{log_text}\"\n/bin/echo \"[00:00.000 --> 00:01.000]  whisper says hi\"\n"),
    )?;
    put("model-with-whisper", json!(voice::stt_command()?));
    put("whisper-run", outcome(voice::transcribe(&audio)));
    put(
        "whisper-args",
        json!(std::fs::read_to_string(&log)?.replace(&wsb, "<ws>")),
    );
    let engine = ws.script(
        "fake-engine",
        &format!(
            "/bin/echo \"$1\" >> \"{log_text}.paths\"\ncase \"$2\" in\n  fail) exit 3 ;;\n  empty) /bin/echo \"[BLANK_AUDIO] [music]\" ; exit 0 ;;\nesac\n/bin/echo \"[00:00] hello  [BLANK_AUDIO] world\"\n/bin/echo 'second  line'\n"
        ),
    )?;
    let engine = engine.to_string_lossy().into_owned();
    ws.settings(
        json!({"stt_command": format!("'{engine}' {{audio}} ok"), "stt_model": "/ignored.bin"}),
    )?;
    put(
        "command-setting",
        json!(voice::stt_command()?.replace(&wsb, "<ws>")),
    );
    put("wav-run", outcome(voice::transcribe(&audio)));
    put(
        "missing-audio",
        outcome(voice::transcribe(&ws.base.join("nope.wav"))),
    );
    ws.settings(json!({"stt_command": format!("{engine} {{audio}} fail")}))?;
    put("engine-fails", outcome(voice::transcribe(&audio)));
    ws.settings(json!({"stt_command": format!("{engine} {{audio}} empty")}))?;
    put("engine-empty", outcome(voice::transcribe(&audio)));
    ws.settings(json!({"stt_command": "rust23-missing-engine {audio}"}))?;
    put("engine-missing", outcome(voice::transcribe(&audio)));
    ws.settings(json!({"stt_command": format!("{engine} {{audio}} ok")}))?;
    put("ogg-no-ffmpeg", outcome(voice::transcribe(&ogg)));
    ws.script(
        "ffmpeg",
        &format!(
            "/bin/echo \"$@\" > \"{log_text}.ffmpeg\"\nfor last; do :; done\n/bin/cp \"{}\" \"$last\"\n",
            audio.to_string_lossy()
        ),
    )?;
    put("ogg-with-ffmpeg", outcome(voice::transcribe(&ogg)));
    put(
        "ffmpeg-args",
        json!(std::fs::read_to_string(format!("{log_text}.ffmpeg"))?.replace(&wsb, "<ws>")),
    );
    ws.script("ffmpeg", "exit 1\n")?;
    put("ogg-ffmpeg-fails", outcome(voice::transcribe(&ogg)));
    let fake_stt = fixtures.join("fake-stt.sh").to_string_lossy().into_owned();
    ws.settings(json!({"stt_command": format!("{fake_stt} {{audio}}")}))?;
    std::env::set_var(
        "PATH",
        format!("{}:/usr/bin:/bin", ws.bin.to_string_lossy()),
    );
    put(
        "fixture-engine",
        outcome(voice::transcribe(&fixtures.join("behavioral-answer.wav"))),
    );
    ws.settings(json!({"stt_command": "'unbalanced {audio}"}))?;
    put("unbalanced-quote", outcome(voice::transcribe(&audio)));
    let seen = std::fs::read_to_string(format!("{log_text}.paths"))?;
    let audio_text = audio.to_string_lossy().into_owned();
    let ogg_text = ogg.to_string_lossy().into_owned();
    let rows = careerbrew_policy::text::splitlines(&seen)
        .into_iter()
        .map(|p| {
            let path = Path::new(p);
            let temp = path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.to_string_lossy().starts_with("careerbrew-voice-"));
            json!({
                "name": path.file_name().unwrap_or_default().to_string_lossy(),
                "original": p == audio_text || p == ogg_text,
                "temp": temp,
                "retained": path.exists() && temp,
            })
        })
        .collect::<Vec<_>>();
    put("engine-paths", Value::Array(rows));
    let tmp = std::env::var("TMPDIR")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let left = if tmp.exists() {
        std::fs::read_dir(&tmp)?
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("careerbrew-voice-")
            })
            .count()
    } else {
        0
    };
    put("temp-dirs-left", json!(left));
    Ok(Value::Object(out))
}

fn voice_answer(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "voice-answer")?;
    careerbrew_tutor::write_text_abs(&ws.root.join("sensitive-terms.txt"), &format!("{CANARY}\n"))?;
    let fixtures = voice_fixtures();
    let practice = TutorPractice::open()?;
    let cue_rows = json!([
        voice::cues(
            &json!({"text": "Named a trade-off and the alternatives, about which there were options"})
        ),
        voice::cues(&json!({"text": "ignored", "cues": ["STAR", "p99", 5]})),
        voice::cues(&json!({"text": "Short words only here"})),
    ]);
    let said = json!({
        "boundary": voice::says("we cut latency by 40%", "40%"),
        "inside-word": voice::says("restarted", "star"),
        "word": voice::says("the star method", "star"),
        "symbol": voice::says("o(n^2) time", "o(n^2)"),
    });
    let typed_answer = "Clarify edge cases first. Brute force is O(n^2), two pointers is O(n).";
    let behavioral = std::fs::read_to_string(fixtures.join("behavioral-answer.txt"))?;
    let grade_values = json!({
        "arrays": voice::grade(&practice, "arrays-two-pointers-1", typed_answer)?,
        "behavioral": voice::grade(&practice, "beh-impact-1", &behavioral)?,
        "unknown": outcome(voice::grade(&practice, "rust23-unknown", "x")),
    });
    let fake_stt = fixtures.join("fake-stt.sh").to_string_lossy().into_owned();
    ws.settings(json!({"stt_command": format!("{fake_stt} {{audio}}")}))?;
    std::env::set_var(
        "PATH",
        format!("{}:/usr/bin:/bin", ws.bin.to_string_lossy()),
    );
    let wav = fixtures.join("behavioral-answer.wav");
    let mut typed_row = voice::answer(
        &practice,
        "arrays-two-pointers-1",
        Some(typed_answer),
        None,
        Some(42),
    )?;
    let mut spoken = voice::answer(&practice, "beh-impact-1", None, Some(&wav), None)?;
    let neither = outcome(voice::answer(
        &practice,
        "beh-impact-1",
        Some("   "),
        None,
        None,
    ));
    for row in [&mut typed_row, &mut spoken] {
        if let Some(map) = row["row"].as_object_mut() {
            map.remove("at");
        }
    }
    let wav_text = wav.to_string_lossy().into_owned();
    let leaky = format!("{CANARY} clarify edge cases");
    let mut printed = Map::new();
    let calls: [(&str, &str, Option<&str>, Option<&str>); 4] = [
        ("typed", "arrays-two-pointers-1", Some(typed_answer), None),
        ("audio", "beh-impact-1", None, Some(&wav_text)),
        ("unknown", "rust23-unknown", Some("x"), None),
        ("canary", "arrays-two-pointers-1", Some(&leaky), None),
    ];
    for (label, item, text, audio) in calls {
        let (code, stdout) = voice::drill_speak(&practice, item, text, audio, None)?;
        printed.insert(label.into(), json!({"code": code, "stdout": stdout}));
    }
    ws.settings(json!({}))?;
    let (code, stdout) =
        voice::drill_speak(&practice, "beh-impact-1", None, Some(&wav_text), None)?;
    printed.insert("no-engine".into(), json!({"code": code, "stdout": stdout}));
    Ok(json!({
        "cues": cue_rows, "said": said, "grades": grade_values, "typed": typed_row,
        "spoken": spoken, "neither": neither, "printed": printed, "events": events(&ws)?,
    }))
}

fn run_case(name: &str, base: &Path) -> Result<Value> {
    match name {
        "routes-config" => routes_config(base),
        "allowed-routes" => allowed_routes(base),
        "complete-routing" => complete_routing(base),
        "judge-text" => judge_text(base),
        "judge-cache" => judge_cache(base),
        "http-transports" => http_transports(base),
        "labels" => labels(base),
        "agreement-run" => agreement_run(base),
        "voice-transcribe" => voice_transcribe(base),
        "voice-answer" => voice_answer(base),
        _ => Err(Error::value("unknown case")),
    }
}

fn owned(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| pure_path(&value))
        .unwrap_or_default()
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let data = owned("CAREERBREW_HOME");
    let temp = owned("TMPDIR");
    if !temp.is_empty() {
        std::env::set_var("TMPDIR", &temp);
    }
    let base = expand_user(&data).join("rust23-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|wanted| wanted != *name) {
            continue;
        }
        let mut result = match run_case(name, &base) {
            Ok(value) => value,
            Err(error) => json!({"error": error.class, "message": error.message}),
        };
        clean_report(&mut result, &data, &temp);
        cases.push(json!({"id": name, "result": typed(&result)}));
    }
    if plant("extra") {
        careerbrew_tutor::write_json_abs(
            &base.join("planted-extra.json"),
            &json!({"synthetic": true}),
        )?;
    }
    if plant("missing") && selected.is_none_or(|case| case == "labels") {
        let file = base.join("labels/labels/labels.jsonl");
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }
    if plant("duplicate") {
        let file = base.join("routes-config/config/models.yaml");
        if file.exists() {
            let body = std::fs::read(&file)?;
            std::fs::write(&file, body)?;
        }
    }
    Ok(format!("{}\n", dumps(&json!({"cases": cases}))))
}
