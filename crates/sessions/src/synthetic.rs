//! Synthetic rust-10 workload over the native component functions.

use crate::drafts::Backend as _;
use crate::{agents, cli, drafts, error, field, prior_selection, sessions, text, Result};
use careerbrew_projects::projects::{self, AddOptions};
use chrono::NaiveDate;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CASES: &[&str] = &[
    "session-lifecycle",
    "claims-and-drafts",
    "agent-activity",
    "reply-fit",
    "prior-selection",
    "cli-handlers",
];
const TARGET: &str = "C0SYN/1700000000.000100";
pub const DUPLICATE_CONTROL: &str = "duplicate-write-control";

struct Workspace {
    base: PathBuf,
    root: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str, project_names: &[&str]) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var("TZ", "UTC");
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
            crate::write_text(&root, relative, &text)?;
        }
        for project in project_names {
            projects::add(&root, project, AddOptions::default())?;
        }
        Ok(Self { base, root })
    }
}

fn at(value: &str) {
    std::env::set_var("CAREERBREW_NOW", value);
}

fn error_of(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn session_lifecycle(base: &Path) -> Result<Value> {
    let ws = Workspace::new(
        base,
        "session-lifecycle",
        &["billing", "ledger", "search", "resume"],
    )?;
    let root = &ws.root;
    if std::env::var("CAREERBREW_RUST10_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(root.join("planted-extra.txt"), "synthetic")?;
    }
    crate::set_setting(root, "work_session_cap", 2)?;
    let first = sessions::open_session(root, "billing", "sess-a1")?;
    let second = sessions::open_session(root, "ledger", "sess-b2")?;
    let first_id = text(field(&first, "record_id"));
    let identity_shape = first_id.len() == 32
        && first_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let reused = sessions::open_session(root, "billing", "")?;
    let duplicate = error_of(sessions::open_session(root, "billing", "sess-other").map(|_| ()));
    let duplicate_id = error_of(sessions::open_session(root, "search", "sess-a1").map(|_| ()));
    let unknown = error_of(sessions::open_session(root, "missing", "sess-x").map(|_| ()));
    let cap_error = sessions::open_session(root, "search", "sess-search").expect_err("cap");
    let cap = cli::invocation(2, format!("error: {} (breaks US-107)\n", cap_error.message));
    let contract = [
        "runs only the work process",
        "full scope",
        "one at a time, in order",
        "shipped and verified",
        "foreseeable approvals",
        "each scope item marked done or not done",
        "work claim take",
        "work session end --all",
    ]
    .into_iter()
    .map(|needle| {
        (
            needle.to_owned(),
            json!(sessions::CONTRACT.contains(needle)),
        )
    })
    .collect::<serde_json::Map<_, _>>();
    let missing = sessions::end(root, "", true)?;
    let request = sessions::request_wrap(root, "wrap-1")?;
    let repeat_request = sessions::request_wrap(root, "wrap-1")?;
    let state = sessions::load(root)?;
    let fresh_before =
        sessions::fresh_handoff(sessions::find(&state, "sess-a1", true).expect("session"));
    sessions::write_handoff(root, "sess-a1", "SYNTHCANARY fresh after wrap", "")?;
    let state = sessions::load(root)?;
    let fresh_after =
        sessions::fresh_handoff(sessions::find(&state, "sess-a1", true).expect("session"));
    sessions::write_handoff(root, "sess-b2", "ledger complete", "")?;
    let ended = sessions::end(root, "", true)?;
    let claims_after_end = sessions::claims(root)?;
    at("2026-11-03T09:00:00+00:00");
    let resumed = sessions::open_session(root, "billing", "sess-next")?;
    let older = sessions::open_session(root, "ledger", "sess-ledger-next")?;
    at("2026-11-04T09:00:00+00:00");
    let no_older_brief = sessions::open_session(root, "billing", "sess-later")?;
    std::fs::create_dir_all(root.join("work/agents"))?;
    let states = sessions::session_states(root, true)?
        .into_iter()
        .map(|row| text(field(&row, "state")))
        .collect::<Vec<_>>();
    Ok(json!({
        "identity": {"shape": identity_shape, "distinct": first["record_id"] != second["record_id"]},
        "opened": [text(field(&first, "id")), text(field(&second, "id"))],
        "reused": {"id": text(field(&reused, "id")), "reused": reused["reused"]},
        "errors": {"duplicate": duplicate, "duplicate_id": duplicate_id, "unknown": unknown},
        "cap": cap,
        "contract": contract,
        "handoff": {"missing": missing["missing"].as_array().map_or(0, Vec::len), "wrap": request, "repeat_wrap": repeat_request, "fresh_before": fresh_before, "fresh_after": fresh_after},
        "ended": {"count": ended["ended"].as_array().map_or(0, Vec::len), "claims": claims_after_end, "states": states},
        "resume": {"same_project": text(field(&resumed, "brief")) == "SYNTHCANARY fresh after wrap", "other_project": text(field(&older, "brief")) == "ledger complete", "older_day_cleared": text(field(&no_older_brief, "brief")).is_empty()},
    }))
}

struct KeepsOld {
    local: drafts::LocalBackend,
}

impl drafts::Backend for KeepsOld {
    fn root(&self) -> &Path {
        self.local.root()
    }
    fn clear(&self, _target: &str) -> Result<()> {
        Ok(())
    }
    fn save(&self, target: &str, text: &str) -> Result<()> {
        self.local.save(target, text)
    }
    fn read(&self, target: &str) -> Result<Option<String>> {
        self.local.read(target)
    }
}

struct Trims {
    local: drafts::LocalBackend,
}

impl drafts::Backend for Trims {
    fn root(&self) -> &Path {
        self.local.root()
    }
    fn clear(&self, target: &str) -> Result<()> {
        self.local.clear(target)
    }
    fn save(&self, target: &str, text: &str) -> Result<()> {
        self.local
            .save(target, &text.chars().take(10).collect::<String>())
    }
    fn read(&self, target: &str) -> Result<Option<String>> {
        self.local.read(target)
    }
}

pub fn interleaved_transactions(root: &Path, force_cross_session_write: bool) -> Result<Value> {
    let backend = drafts::LocalBackend::new(root);
    let mut operations = 0;
    let mut contentions = 0;
    let mut draft_saves = 0;
    let mut cross_session = 0;
    let mut stale = 0;
    let mut conflicting = 0;
    for n in 0..250 {
        let target = format!("C{}/1.0", n % 4);
        let resource = drafts::uses_entry(&target)?;
        sessions::take(root, &resource, "sess-a", None)?;
        operations += 1;
        if sessions::holder(root, &resource)?
            .and_then(|row| {
                field(&row, "session")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            != Some("sess-a")
        {
            stale += 1;
        }
        let before = backend.read(&target)?;
        let contested = if force_cross_session_write && n == 0 {
            backend.save(&target, "SYNTHCANARY forced cross-session")
        } else {
            drafts::replace(
                root,
                &target,
                &format!("SYNTHCANARY contested {n}"),
                false,
                "sess-b",
            )
            .map(|_| ())
        };
        operations += 1;
        match contested {
            Ok(()) => cross_session += 1,
            Err(error) if error.class == "DraftClaimed" => contentions += 1,
            Err(error) => return Err(error),
        }
        if backend.read(&target)? != before {
            conflicting += 1;
        }
        let result = drafts::replace(
            root,
            &target,
            &format!("SYNTHCANARY holder {n}"),
            false,
            "sess-a",
        )?;
        operations += 1;
        if result["same"] == true {
            draft_saves += 1;
        } else {
            conflicting += 1;
        }
        if sessions::holder(root, &resource)?
            .and_then(|row| {
                field(&row, "session")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            != Some("sess-a")
        {
            stale += 1;
        }
        sessions::release(root, &resource, "sess-a")?;
        operations += 1;
        if sessions::holder(root, &resource)?.is_some() {
            stale += 1;
        }
    }
    Ok(json!({
        "operations": operations, "contentions": contentions, "draft_saves": draft_saves,
        "cross_session_writes": cross_session, "stale_claims": stale,
        "conflicting_target_saves": conflicting,
    }))
}

fn claims_and_drafts(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "claims-and-drafts", &["billing", "ledger"])?;
    let root = &ws.root;
    sessions::open_session(root, "billing", "sess-a")?;
    sessions::open_session(root, "ledger", "sess-b")?;
    let canonical = sessions::resource("pr:Example/Tools#17")?;
    let taken = sessions::take(root, &canonical, "sess-a", None)?;
    let held = error_of(sessions::take(root, "pr:example/tools#17", "sess-b", None).map(|_| ()));
    let renewed = sessions::take(root, "pr:EXAMPLE/TOOLS#17", "sess-a", None)?["renewed"] == true;
    let released = sessions::release(root, "pr:example/tools#17", "sess-a")?;
    let invalid = [
        "pr:tools#17",
        "pr:example/tools#0",
        "pr:example/tools#01",
        "pr:./tools#1",
        " pr:example/tools#17",
        "thread with spaces",
    ]
    .into_iter()
    .map(|value| error_of(sessions::take(root, value, "sess-a", None).map(|_| ())))
    .collect::<Vec<_>>();
    sessions::take(root, "browser:sso", "sess-a", Some(30))?;
    at("2026-11-02T09:29:00+00:00");
    let before_expiry = sessions::holder(root, "browser:sso")?.is_some();
    at("2026-11-02T09:31:00+00:00");
    let expired = sessions::holder(root, "browser:sso")?.is_none();
    at("2026-11-02T09:00:00+00:00");
    let saved = drafts::replace(root, TARGET, "SYNTHCANARY approved\r\n", false, "sess-a")?;
    let other =
        error_of(drafts::replace(root, TARGET, "SYNTHCANARY other", false, "sess-b").map(|_| ()));
    let without_session =
        error_of(drafts::replace(root, TARGET, "SYNTHCANARY other", false, "").map(|_| ()));
    let kept_text = drafts::Backend::read(&drafts::LocalBackend::new(root), TARGET)?;
    sessions::release(root, &drafts::uses_entry(TARGET)?, "sess-a")?;
    let after_release =
        drafts::replace(root, TARGET, "SYNTHCANARY second", false, "sess-b")?["same"] == true;
    let kept = drafts::replace(root, TARGET, "ignored", true, "sess-b")?;
    let local = drafts::LocalBackend::new(root);
    drafts::Backend::save(&local, "C1/3.0", "SYNTHCANARY old")?;
    let keeps_old = KeepsOld {
        local: drafts::LocalBackend::new(root),
    };
    let not_cleared = error_of(
        drafts::replace_at(
            root,
            "C1/3.0",
            "SYNTHCANARY new",
            &keeps_old,
            Duration::from_secs(10),
            false,
            "",
        )
        .map(|_| ()),
    );
    let trims = Trims {
        local: drafts::LocalBackend::new(root),
    };
    let mismatch = error_of(
        drafts::replace_at(
            root,
            "C1/2.0",
            "This reply is longer than ten characters.",
            &trims,
            Duration::from_secs(10),
            false,
            "",
        )
        .map(|_| ()),
    );
    let empty = error_of(drafts::replace(root, "C1/7.0", " \n", false, "").map(|_| ()));
    let bad_targets = ["", "../etc", "a b", &"x".repeat(200)]
        .into_iter()
        .map(|value| error_of(drafts::check_target(value).map(|_| ())))
        .collect::<Vec<_>>();
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    drafts::replace(root, "C9/secure.1", "SYNTHCANARY private reply", false, "")?;
    let raw = std::fs::read(drafts::LocalBackend::new(root).path("C9/secure.1"))?;
    Ok(json!({
        "claim": {"canonical": canonical, "taken": text(field(&taken, "session")), "held": held, "renewed": renewed, "released": released, "invalid": invalid},
        "expiry": {"before": before_expiry, "after": expired, "zero": error_of(sessions::take(root, "browser:sso", "sess-a", Some(0)).map(|_| ()))},
        "save": {"same": saved["same"], "other": other, "without_session": without_session, "original_kept": kept_text.as_deref() == Some("SYNTHCANARY approved\r\n"), "after_release": after_release, "if_empty": kept["kept"]},
        "failures": {"not_cleared": not_cleared, "mismatch": mismatch, "empty": empty, "bad_targets": bad_targets},
        "privacy": {"sealed": raw.starts_with(b"CBV1\n"), "plain_text_absent": !raw.windows(b"SYNTHCANARY".len()).any(|part| part == b"SYNTHCANARY"), "digest_name": !drafts::LocalBackend::new(root).path("C9/secure.1").file_name().unwrap_or_default().to_string_lossy().contains("C9")},
        "compare": [drafts::compare("a  \n", Some("a\n\n")), drafts::compare("a\nb", Some("a\nc"))],
        "interleavings": interleaved_transactions(root, false)?,
    }))
}

fn agent_activity(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "agent-activity", &[])?;
    let root = &ws.root;
    agents::record(
        root,
        "review-bot",
        "SYNTHCANARY Reviewing\nPR 7",
        "running",
        "https://github.example.com/org/shop/pull/7",
        "billing",
        "",
    )?;
    at("2026-11-02T09:05:00+00:00");
    agents::record(
        root,
        "fix-bot",
        "Fixing the flaky test",
        "running",
        "",
        "ledger",
        "",
    )?;
    at("2026-11-02T09:10:00+00:00");
    agents::record(
        root,
        "review-bot",
        "Left 3 comments",
        "done",
        "",
        "billing",
        "",
    )?;
    agents::record(root, "review-bot", "PR 8", "running", "", "billing", "pr-8")?;
    let view = agents::view_at(root, crate::now())?;
    at("2026-11-02T10:11:00+00:00");
    let quiet = agents::runs(root)?
        .first()
        .is_some_and(|row| text(field(row, "status")) == "quiet");
    agents::record(root, "review-bot", "Done again", "done", "", "", "pr-8")?;
    agents::record(
        root,
        "review-bot",
        "Started again",
        "running",
        "",
        "billing",
        "pr-8",
    )?;
    let rows = agents::runs(root)?;
    let restarted = rows
        .iter()
        .find(|row| text(field(row, "run")) == "pr-8")
        .expect("restart");
    let invalid = [
        ("a b", "x", "running", "", "", ""),
        ("bot", " ", "running", "", "", ""),
        ("bot", &"x".repeat(201), "running", "", "", ""),
        ("bot", "x", "paused", "", "", ""),
        ("bot", "x", "running", "javascript:alert(1)", "", ""),
        ("bot", "x", "running", "", &"p".repeat(81), ""),
    ]
    .into_iter()
    .map(|(agent, title, status, url, project, run)| {
        error_of(agents::record(root, agent, title, status, url, project, run).map(|_| ()))
    })
    .collect::<Vec<_>>();
    at("2026-11-10T09:00:00+00:00");
    let expired = agents::runs(root)?;
    agents::record(root, "new-bot", "now", "running", "", "", "")?;
    let retained = agents::load(root)?
        .iter()
        .map(|row| text(field(row, "agent")))
        .collect::<Vec<_>>();
    let review = view["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|group| text(field(group, "project")) == "billing")
        .expect("billing")["runs"]
        .clone();
    Ok(json!({
        "counts": view["counts"], "groups": view["groups"].as_array().into_iter().flatten().map(|group| text(field(group, "project"))).collect::<Vec<_>>(),
        "review": review, "quiet": quiet,
        "restart": {"events": restarted["events"], "started_is_last": restarted["started_at"] == restarted["at"]},
        "invalid": invalid, "expired": expired, "retained": retained,
    }))
}

fn thread(texts: &[&str], mine_last: bool) -> Result<Vec<Value>> {
    let rows = texts.iter().enumerate().map(|(n, text)| {
        json!({"ts": format!("1700000{n:03}.000100"), "text": text, "mine": mine_last && n + 1 == texts.len()})
    }).collect::<Vec<_>>();
    drafts::messages(&json!({"messages": rows}))
}

fn reply_fit(base: &Path) -> Result<Value> {
    Workspace::new(base, "reply-fit", &[])?;
    let started = drafts::fit(
        &thread(
            &["Here is the PR.", "Deploy started, will update when done."],
            false,
        )?,
        Some("Could you rename SYNTHCANARY before we ship?"),
        None,
        None,
    )?;
    let approve = drafts::fit(
        &thread(&["Rolling out to production now."], false)?,
        Some("LGTM, thanks!"),
        None,
        None,
    )?;
    let major = drafts::fit(
        &thread(&["Release kicked off."], false)?,
        Some("nit: rename it"),
        None,
        Some("the migration drops the users table"),
    )?;
    let planned = [
        "We will deploy tomorrow.",
        "Can we release this?",
        "The release will start after lunch.",
        "We are about to start the deploy.",
        "Did the deploy start?",
        "The fix is not deployed yet.",
        "Hasn't been released.",
    ]
    .into_iter()
    .map(|text| {
        Ok(json!([
            text,
            drafts::fit(
                &thread(&[text], false)?,
                Some("Could you add a test first?"),
                None,
                None
            )?["kind"]
                .clone()
        ]))
    })
    .collect::<Result<Vec<_>>>()?;
    let rollback = drafts::fit(
        &thread(&["Deploy started.", "Rolled back."], false)?,
        Some("Can you add a test?"),
        None,
        None,
    )?["kind"]
        .clone();
    let restarted = drafts::fit(
        &thread(
            &[
                "Deploy started.",
                "Rolled back.",
                "Deploying again with the fix.",
            ],
            false,
        )?,
        Some("Can you add a test?"),
        None,
        None,
    )?["kind"]
        .clone();
    let emoji = [
        "thanks", "Thanks!", "ty", "ok", "Done.", "awesome", "+1", ":tada:",
    ]
    .into_iter()
    .map(|text| {
        Ok(json!([
            text,
            drafts::fit(
                &thread(&["Merged your fix.", text], false)?,
                Some("You're welcome"),
                None,
                None
            )?["emoji"]
                .clone()
        ]))
    })
    .collect::<Result<Vec<_>>>()?;
    let stale_thread = thread(
        &[
            "Can you look at the PR?",
            "Never mind, I merged it.",
            "Deploy started.",
        ],
        false,
    )?;
    let seen = stale_thread[0]["ts"].clone();
    let stale = drafts::fit(
        &stale_thread,
        Some("Please split the PR in two."),
        Some(&seen),
        None,
    )?;
    let plain = drafts::fit(
        &thread(&["Can you look at the PR?", "Also the doc."], false)?,
        Some("Looking now."),
        Some(&json!("1700000000.000100")),
        None,
    )?;
    let ordered = drafts::fit(&drafts::messages(&json!({"messages": [{"ts": "2.0", "text": "Deploy started."}, {"ts": "1.0", "text": "Can you add a test?"}]}))?, None, None, None)?["kind"].clone();
    let mine = drafts::fit(
        &thread(&["Can you add a test?", "Added it."], true)?,
        Some("ok"),
        None,
        None,
    )?["kind"]
        .clone();
    let long_error = error_of(
        drafts::fit(
            &thread(&["Release kicked off."], false)?,
            None,
            None,
            Some(&"x".repeat(201)),
        )
        .map(|_| ()),
    );
    let small_draft = drafts::fit(
        &thread(&["Here is the doc."], false)?,
        Some("thanks!"),
        None,
        None,
    )?["kind"]
        .clone();
    let empty_thread = error_of(drafts::messages(&json!({"messages": []})).map(|_| ()));
    Ok(json!({
        "started": {"kind": started["kind"], "deploy_started": started["deploy_started"], "text": started["text"], "newer": started["newer"], "stale": started["stale"]},
        "approve": {"kind": approve["kind"], "keep_draft": approve["keep_draft"], "text": approve["text"]},
        "major": {"kind": major["kind"], "text": major["text"], "long_error": long_error},
        "planned": planned, "rollback": rollback, "restarted": restarted, "emoji": emoji,
        "small_draft": small_draft, "long_not_small": drafts::small_ack("thanks for the detailed review of the doc"),
        "stale": {"kind": stale["kind"], "newer": stale["newer"], "stale": stale["stale"], "newest_ts": stale["newest_ts"]},
        "plain_stale": {"kind": plain["kind"], "keep": plain["keep_draft"]}, "ordered": ordered, "mine": mine,
        "empty_thread": empty_thread,
    }))
}

fn prior_row(day: &str, record: Value, project: Value, extra: Value) -> Value {
    let mut row = json!({"day": day, "record_id": record, "project_incarnation": project});
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            row[key] = value.clone();
        }
    }
    row
}

fn prior_selection_case(base: &Path) -> Result<Value> {
    Workspace::new(base, "prior-selection", &[])?;
    let (a, b, p, q) = (
        "a".repeat(32),
        "b".repeat(32),
        "1".repeat(32),
        "2".repeat(32),
    );
    let today = NaiveDate::from_ymd_opt(2026, 11, 7).expect("today");
    let valid = prior_row("2026-11-06", json!(a), json!(p), json!({}));
    let inputs = vec![
        (today, vec![valid.clone()], vec![p.clone()]),
        (
            today,
            vec![prior_row("2026-11-07", json!(a), json!(p), json!({}))],
            vec![p.clone()],
        ),
        (
            today,
            vec![prior_row("2026-11-05", json!(a), json!(p), json!({}))],
            vec![p.clone()],
        ),
        (
            today,
            vec![prior_row(
                "2026-11-06",
                json!(a),
                json!(p),
                json!({"ended": "yes", "handoff": "SYNTHCANARY"}),
            )],
            vec![p.clone()],
        ),
        (
            today,
            vec![prior_row(
                "2026-11-06",
                json!(a),
                json!(p),
                json!({"status": "done", "pending": true}),
            )],
            vec![p.clone()],
        ),
        (today, vec![json!({"day": "2026-11-06"})], vec![p.clone()]),
        (
            today,
            vec![prior_row("2026-11-06", json!(a), json!(q), json!({}))],
            vec![p.clone()],
        ),
        (
            today,
            vec![
                valid.clone(),
                prior_row("2026-11-06", json!(a), json!(q), json!({})),
            ],
            vec![p.clone()],
        ),
        (
            today,
            vec![prior_row("2026-02-29", json!(a), json!(p), json!({}))],
            vec![p.clone()],
        ),
        (
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(),
            vec![prior_row("2024-02-29", json!(a), json!(p), json!({}))],
            vec![p.clone()],
        ),
        (
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap(),
            vec![prior_row("2026-12-31", json!(a), json!(p), json!({}))],
            vec![p.clone()],
        ),
        (today, vec![valid.clone()], vec![p.clone(), p.clone()]),
        (
            today,
            vec![prior_row(
                "2026-11-06",
                json!("A".repeat(32)),
                json!(p),
                json!({}),
            )],
            vec![p.clone()],
        ),
        (
            today,
            vec![prior_row("2026-11-06", json!(a), json!("short"), json!({}))],
            vec![p.clone()],
        ),
        (today, vec![Value::Null], vec![p.clone()]),
        (
            today,
            vec![
                valid.clone(),
                prior_row("2026-11-07", json!(a), json!(p), json!({})),
            ],
            vec![p.clone()],
        ),
    ];
    let results = inputs
        .into_iter()
        .map(|(day, rows, projects)| prior_selection::select(day, &rows, &projects))
        .collect::<Result<Vec<_>>>()?;
    let errors = vec![
        "ValueError".to_owned(),
        "ValueError".to_owned(),
        error_of(prior_selection::select(NaiveDate::MIN, &[], &[]).map(|_| ())),
        error_of(prior_selection::select(today, &[], &[p.clone(), "bad".into()]).map(|_| ())),
        error_of(prior_selection::select(today, &vec![Value::Null; 10001], &[]).map(|_| ())),
    ];
    let first = vec![
        prior_row(
            "2026-11-06",
            json!(a),
            json!(p),
            json!({"status": "waiting", "extra": {"note": ["SYNTHCANARY"]}}),
        ),
        prior_row("2026-11-07", json!(b), json!(p), json!({})),
    ];
    let repeated = prior_selection::select(today, &first, std::slice::from_ref(&p))?
        == prior_selection::select(today, &first, std::slice::from_ref(&p))?;
    let reversed = first.iter().rev().cloned().collect::<Vec<_>>();
    let reversed = prior_selection::select(today, &reversed, &[p])?;
    Ok(json!({"results": results, "errors": errors, "repeat": repeated, "reversed": reversed}))
}

fn cli_handlers(base: &Path) -> Result<Value> {
    let ws = Workspace::new(base, "cli-handlers", &["billing", "ledger"])?;
    let root = &ws.root;
    sessions::open_session(root, "billing", "sess-a")?;
    sessions::open_session(root, "ledger", "sess-b")?;
    let agent_empty = cli::invocation(
        cli::work_agent_show(root, false).0,
        cli::work_agent_show(root, false).1,
    );
    let (code, output) = cli::work_agent_event(
        root,
        "review-bot",
        "SYNTHCANARY Reviewing PR 7",
        "running",
        "https://github.example.com/org/shop/pull/7",
        "billing",
        "",
        "",
    );
    let agent_event = cli::invocation(code, output);
    let (code, output) = cli::work_agent_show(root, false);
    let agent_show = cli::invocation(code, output);
    let (code, output) = cli::work_agent_show(root, true);
    let agent_json = cli::invocation(code, output);
    let (code, output) =
        cli::work_agent_event(root, "bot", "x", "running", "javascript:alert", "", "", "");
    let agent_bad = cli::invocation(code, output);
    let (code, output) = cli::work_agent_event(
        root,
        "billing-bot",
        "SYNTHCANARY session step",
        "running",
        "",
        "",
        "",
        "sess-a",
    );
    let session_event = cli::invocation(code, output);
    let (code, output) = cli::work_agent_event(
        root,
        "billing-bot",
        "x",
        "running",
        "",
        "ledger",
        "",
        "sess-a",
    );
    let session_conflict = cli::invocation(code, output);
    let (code, output) = cli::work_claim(root, "take", "pr:Example/Tools#17", "sess-a");
    let claim = cli::invocation(code, output);
    let (code, output) = cli::work_claim(root, "take", "pr:example/tools#17", "sess-b");
    let claim_held = cli::invocation(code, output);
    let (code, output) = cli::work_claim_list(root, false);
    let claim_list = cli::invocation(code, output);
    let (code, output) = cli::work_claim_list(root, true);
    let claim_json = cli::invocation(code, output);
    let (code, output) = cli::work_claim(root, "release", "pr:EXAMPLE/TOOLS#17", "sess-a");
    let claim_release = cli::invocation(code, output);
    let (code, output) = cli::work_claim(root, "release", "pr:example/tools#17", "sess-a");
    let claim_release_twice = cli::invocation(code, output);
    let thread = ws.base.join("thread.json");
    std::fs::write(
        &thread,
        serde_json::to_string(
            &json!({"messages": [{"ts": "1", "text": "SYNTHCANARY asked for a review"}, {"ts": "2", "text": "Deploy started."}]}),
        )?,
    )?;
    let change = ws.base.join("change.txt");
    std::fs::write(&change, "Could you change SYNTHCANARY first?")?;
    let (code, output) = cli::draft_fit(root, &thread, Some(&change), Some("1"), None, true);
    let fit_json = cli::invocation(code, output);
    let (code, output) = cli::draft_fit(root, &thread, Some(&change), None, None, false);
    let fit_text = cli::invocation(code, output);
    let (code, output) = cli::draft_replace(root, TARGET, &change, "", false);
    let replaced = cli::invocation(code, output);
    let (code, output) = cli::draft_replace(root, TARGET, &change, "", true);
    let replaced_json = cli::invocation(code, output);
    let (code, output) = cli::draft_show(root, TARGET);
    let shown = cli::invocation(code, output);
    let (code, output) = cli::draft_show(root, "C9/missing.1");
    let missing = cli::invocation(code, output);
    let sent = ws.base.join("sent.txt");
    std::fs::write(&sent, "Looks good.\n")?;
    let back = ws.base.join("back.txt");
    std::fs::write(&back, "Looks good.  \n\n")?;
    let (code, output) = cli::draft_check(&sent, &back);
    let check_same = cli::invocation(code, output);
    std::fs::write(&back, "Please change SYNTHCANARY.")?;
    let (code, output) = cli::draft_check(&sent, &back);
    let check_bad = cli::invocation(code, output);
    let (code, output) =
        cli::draft_fit(root, &ws.base.join("missing.json"), None, None, None, false);
    let fit_missing = cli::invocation(code, output);
    if std::env::var("CAREERBREW_RUST10_PLANT_WRITE").as_deref() == Ok("missing") {
        std::fs::remove_file(drafts::LocalBackend::new(root).path(TARGET))?;
    }
    Ok(json!({
        "agent": {"empty": agent_empty, "event": agent_event, "show": agent_show, "json": agent_json, "bad": agent_bad, "session_event": session_event, "session_conflict": session_conflict},
        "claim": {"take": claim, "held": claim_held, "list": claim_list, "json": claim_json, "release": claim_release, "release_twice": claim_release_twice},
        "draft": {"fit_json": fit_json, "fit_text": fit_text, "replace": replaced, "replace_json": replaced_json, "show": shown, "missing": missing, "check_same": check_same, "check_bad": check_bad, "fit_missing": fit_missing},
    }))
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "session-lifecycle" => session_lifecycle(base),
        "claims-and-drafts" => claims_and_drafts(base),
        "agent-activity" => agent_activity(base),
        "reply-fit" => reply_fit(base),
        "prior-selection" => prior_selection_case(base),
        "cli-handlers" => cli_handlers(base),
        _ => Err(error("ValueError", "unknown rust-10 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust10-workload");
    std::fs::create_dir_all(&base)?;
    if selected == Some(DUPLICATE_CONTROL) {
        if std::env::var("CAREERBREW_RUST10_PLANT_WRITE").as_deref() != Ok("duplicate") {
            return Err(error(
                "ValueError",
                "duplicate control requires its registered plant",
            ));
        }
        let root = base.join("cli-handlers/data");
        let backend = drafts::LocalBackend::new(&root);
        if !backend.path(TARGET).is_file() {
            return Err(error(
                "ValueError",
                "duplicate control needs the expected saved draft",
            ));
        }
        let content = backend
            .read(TARGET)
            .map_err(|_| error("ControlRead", "duplicate control read failed"))?
            .ok_or_else(|| error("ValueError", "duplicate control needs draft content"))?;
        crate::context()?
            .store()
            .write_text(&backend.path(TARGET), &content)
            .map_err(|failure| {
                let class = match failure {
                    careerbrew_vault::atomic::StoreError::NoKey => "ControlWriteNoKey",
                    careerbrew_vault::atomic::StoreError::Vault => "ControlWriteVault",
                    careerbrew_vault::atomic::StoreError::Utf8 => "ControlWriteUtf8",
                    careerbrew_vault::atomic::StoreError::Codec(_) => "ControlWriteCodec",
                    careerbrew_vault::atomic::StoreError::Io(_) => "ControlWriteIo",
                };
                error(class, "duplicate control write failed")
            })?;
        let report = crate::typed(
            &json!({"cases": [{"id": DUPLICATE_CONTROL, "result": {"observed": true}}]}),
        );
        return Ok(serde_json::to_string(&report)? + "\n");
    }
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            cases.push(json!({"id": case, "result": run(case, &base)?}));
        }
    }
    let report = crate::typed(&json!({"cases": cases}));
    Ok(serde_json::to_string(&report)? + "\n")
}
