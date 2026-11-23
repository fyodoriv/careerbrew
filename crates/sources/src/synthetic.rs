//! Synthetic rust-08 workload. Every side effect is scoped below the
//! invocation-owned CAREERBREW_HOME; reports contain metadata only.

use crate::{calendar, cli, connectors, http, mail, typed, Result, SourceError};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const CASES: &[&str] = &[
    "http-policy",
    "connector-evidence",
    "calendar-recurrence",
    "mail-readers",
    "source-isolation",
    "cli-connect",
];
const SECRET_MARKERS: &[&str] = &[
    "synthetic-not-reported",
    "synthetic-jira-not-reported",
    "synthetic-interactive-not-reported",
    "synthetic-refresh-not-reported",
    "synthetic-access-token",
    "synthetic-refresh-token",
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
        std::fs::set_permissions(&root, Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-10-19T10:00:00+00:00");
        std::env::set_var("CB_TEST_TOKEN", "synthetic-not-reported");
        std::env::set_var("CB_JIRA", "synthetic-jira-not-reported");
        std::env::set_var("TZ", "UTC");
        std::env::remove_var("CAREERBREW_TEST_OWNER");
        std::env::remove_var("CB_MISSING");
        Ok(Self { base, root })
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default())
        .join("tests/fixtures/http")
}

fn fixture_calendar() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/calendar/work.ics")
}

fn fetcher(root: &Path) -> impl FnMut(&str) -> Result<Value> + '_ {
    move |url| http::get_json(root, url, None)
}

fn http_policy(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "http-policy")?;
    if std::env::var("CAREERBREW_RUST08_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(workspace.root.join("planted-extra.txt"), "synthetic")?;
    }
    let allowed = vec!["example.com".to_owned(), "example.org".to_owned()];
    let hosts = [
        "example.com",
        "api.example.com",
        "badexample.com",
        "other.invalid",
        "localhost",
        "127.0.0.1",
    ];
    let host_rows = hosts
        .iter()
        .map(|host| json!([host, http::host_allowed(host, Some(&allowed))]))
        .collect::<Vec<_>>();
    let denied = http::check_host("https://other.invalid/private", Some(&allowed))
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted");
    let redirects = [
        http::redirect(
            "https://api.example.com/a",
            "https://api.example.com/b",
            &["Authorization".into()],
            Some(&allowed),
        ),
        http::redirect(
            "https://api.example.com/a",
            "https://api.example.org/b",
            &["Authorization".into()],
            Some(&allowed),
        ),
        http::redirect(
            "https://api.example.com/a",
            "http://api.example.com/b",
            &[],
            Some(&allowed),
        ),
        http::redirect(
            "https://api.example.com/a",
            "https://api.example.org/b",
            &["User-Agent".into()],
            Some(&allowed),
        ),
        http::redirect(
            "https://api.example.com/a",
            "https://other.invalid/b",
            &[],
            Some(&allowed),
        ),
    ]
    .iter()
    .map(|result| match result {
        Ok(url) => json!(url),
        Err(error) => json!(error.class),
    })
    .collect::<Vec<_>>();
    let root = fixture_root();
    let user = http::get_json(&root, "https://api.github.com/user", None)?;
    let raw = http::get_bytes(
        &root,
        "https://weworkremotely.com/categories/remote-programming-jobs.rss",
        None,
    )?;
    let raw = String::from_utf8_lossy(&raw);
    let missing = http::get_json(&root, "https://absent.example.com/missing", None)
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted");
    let status = http::get_json(
        &root,
        "https://bad.atlassian.example.com/rest/api/3/myself",
        None,
    )
    .err()
    .map(|error| error.class)
    .unwrap_or("accepted");
    let origins = [
        "https://example.com/a",
        "http://example.com:80/a",
        "https://example.com:444/a",
    ]
    .iter()
    .map(|raw| {
        let url = http::parse_url(raw).expect("fixed URL");
        let scheme = url.scheme;
        let host = url.host;
        let port = url.port;
        json!([scheme, host, port])
    })
    .collect::<Vec<_>>();
    Ok(json!({
        "hosts": host_rows,
        "denied": denied,
        "origins": origins,
        "redirects": redirects,
        "fixture_user": crate::text(crate::field(&user, "login")),
        "raw_xml": raw.starts_with("<?xml") && raw.contains("<rss"),
        "missing": missing,
        "status": status,
        "limits": {"jira_pages": connectors::JIRA_MAX_PAGES, "pagerduty_pages": connectors::PD_MAX_PAGES, "reply_pages": connectors::REPLY_PAGES},
    }))
}

fn item_projection(items: Vec<Value>) -> Vec<Value> {
    let keys = [
        "id",
        "kind",
        "title",
        "at",
        "url",
        "tags",
        "origin",
        "draft",
        "state",
        "merged_at",
    ];
    items
        .into_iter()
        .map(|item| {
            let mut out = Map::new();
            for key in keys {
                if let Some(value) = crate::field(&item, key) {
                    out.insert(key.into(), value.clone());
                }
            }
            Value::Object(out)
        })
        .collect()
}

fn issue(index: usize) -> Value {
    json!({"key": format!("PROJ-{index}"), "fields": {"summary": if index == 100 { "done last".into() } else { format!("done {index}") }, "resolutiondate": "2026-10-01T00:00:00.000+0000", "issuetype": {"name": "Task"}}})
}

fn connector_evidence(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "connector-evidence")?;
    let root = fixture_root();
    let configs = json!({
        "github": {"kind": "github", "token": "env:CB_TEST_TOKEN", "user": "me"},
        "jira": {"kind": "jira", "token": "env:CB_TEST_TOKEN", "base_url": "https://jira.example.com", "email": "me@example.com"},
        "slack": {"kind": "slack", "token": "env:CB_TEST_TOKEN"},
        "gdocs": {"kind": "gdocs", "token": "env:CB_TEST_TOKEN"},
        "zoom": {"kind": "zoom", "token": "env:CB_TEST_TOKEN"},
        "outlook": {"kind": "outlook", "token": "env:CB_TEST_TOKEN"},
    });
    let mut evidence = Map::new();
    for kind in ["github", "jira", "slack", "gdocs", "zoom", "outlook"] {
        let mut fetch = fetcher(&root);
        evidence.insert(
            kind.into(),
            json!(item_projection(connectors::run(
                &configs[kind],
                "user",
                &mut fetch,
                false
            )?)),
        );
    }
    // The Python personal-mark provider materializes its feed folder on the
    // first Slack recognition read, even when no mark exists.
    std::fs::create_dir_all(workspace.root.join("work/feed"))?;
    let personal_slack = connectors::run(&configs["slack"], "user", &mut fetcher(&root), true)?;
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let seen_by_fetch = Rc::clone(&seen);
    let mut paged = move |url: &str| {
        if url.ends_with("/myself") {
            return Ok(json!({"accountId": "synthetic"}));
        }
        seen_by_fetch.borrow_mut().push(url.to_owned());
        if !url.contains("nextPageToken=") {
            Ok(
                json!({"issues": (0..100).map(issue).collect::<Vec<_>>(), "nextPageToken": "tok2", "isLast": false}),
            )
        } else {
            Ok(json!({"issues": [issue(100)], "isLast": true}))
        }
    };
    let pages = connectors::jira(&configs["jira"], "user", &mut paged)?;
    let seen = seen.borrow();
    let rejected = connectors::jira(&json!({"kind": "jira", "token": "env:CB_TEST_TOKEN", "base_url": "https://bad.atlassian.example.com", "email": "me@example.com"}), "user", &mut fetcher(&root)).err().map(|error| error.class).unwrap_or("accepted");
    let pd_calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let pd_calls_by_fetch = Rc::clone(&pd_calls);
    let mut pd = move |url: &str| {
        let mut calls = pd_calls_by_fetch.borrow_mut();
        calls.push(url.to_owned());
        if calls.len() == 1 {
            Ok(
                json!({"log_entries": [{"id": "L1", "type": "acknowledge_log_entry", "created_at": "2026-10-18T10:00:00Z", "agent": {"id": "U1"}, "incident": {"id": "I1", "summary": "Synthetic outage", "html_url": "https://pagerduty.example.com/incidents/I1"}}], "more": true}),
            )
        } else {
            Ok(
                json!({"log_entries": [{"id": "L2", "type": "resolve_log_entry", "created_at": "2026-10-18T11:00:00Z", "agent": {"id": "U1"}, "incident": {"id": "I1", "summary": "Synthetic outage", "html_url": "https://pagerduty.example.com/incidents/I1"}}], "more": false}),
            )
        }
    };
    let actions = connectors::pd_actions(&mut pd, "U1")?;
    let pd_calls = pd_calls.borrow();
    Ok(json!({
        "evidence": evidence,
        "personal_slack": personal_slack,
        "jira_pages": {"count": pages.len(), "first": crate::text(crate::field(&pages[0], "id")), "last": crate::text(crate::field(&pages[pages.len() - 1], "id")), "calls": seen.len(), "cursor": seen[1].contains("nextPageToken=tok2")},
        "jira_rejected": rejected,
        "pagerduty": {"items": actions, "calls": pd_calls.len(), "second_offset": pd_calls[1].contains("offset=1")},
        "token_page": connectors::token_page("github", "https://github.example.com/api/v3"),
        "status_missing": connectors::status(&json!({"kind": "jira", "token": "env:CB_MISSING", "base_url": "https://jira.example.com"}))?,
        "status_ready": connectors::status(&json!({"kind": "github", "token": "env:CB_TEST_TOKEN", "user": "me"}))?,
    }))
}

fn calendar_recurrence(base: &Path) -> Result<Value> {
    Workspace::new(base, "calendar")?;
    let utc = calendar::zone("UTC")?;
    let work = std::fs::read_to_string(fixture_calendar())?;
    let work_events = calendar::parse_ics(
        &work,
        NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
        utc,
        &["me@example.com".into()],
    )?;
    let synthetic = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series@synthetic.example\r\nDTSTART;TZID=America/New_York:20261019T090000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE;TZID=America/New_York:20261020T090000\r\nSUMMARY:Private recurring review\r\nCLASS:PRIVATE\r\nORGANIZER:mailto:me@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let mut days = Vec::new();
    for day in [19, 20, 21] {
        days.push(calendar::parse_ics(
            synthetic,
            NaiveDate::from_ymd_opt(2026, 10, day).unwrap(),
            calendar::zone("America/New_York")?,
            &["me@example.com".into()],
        )?);
    }
    let graph = calendar::graph_events(
        &json!({"kind": "graph", "token": "env:CB_TEST_TOKEN"}),
        NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
        utc,
        &mut fetcher(&fixture_root()),
    )?;
    Ok(json!({
        "work": work_events.iter().map(calendar::Event::to_json).collect::<Vec<_>>(),
        "series_lengths": days.iter().map(Vec::len).collect::<Vec<_>>(),
        "series": days.iter().map(|events| events.iter().map(calendar::Event::to_json).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "graph": graph.iter().map(calendar::Event::to_json).collect::<Vec<_>>(),
    }))
}

struct FakeImap {
    calls: Rc<RefCell<Vec<String>>>,
}

impl mail::ImapConnection for FakeImap {
    fn starttls(&mut self, check_hostname: bool, verify_mode: u8) -> Result<()> {
        self.calls.borrow_mut().push(format!(
            "starttls:{}:{verify_mode}",
            if check_hostname { "True" } else { "False" }
        ));
        Ok(())
    }
    fn login(&mut self, _user: &str, _password: &str) -> Result<()> {
        self.calls.borrow_mut().push("login".into());
        Ok(())
    }
    fn examine(&mut self, mailbox: &str) -> Result<()> {
        self.calls
            .borrow_mut()
            .push(format!("select:{mailbox}:True"));
        Ok(())
    }
    fn search_since(&mut self, day: &str) -> Result<Vec<String>> {
        self.calls.borrow_mut().push(format!("(SINCE {day})"));
        Ok(vec!["1".into()])
    }
    fn fetch_headers(&mut self, id: &str) -> Result<String> {
        self.calls.borrow_mut().push(format!(
            "fetch:{id}:(BODY.PEEK[HEADER.FIELDS (MESSAGE-ID FROM SUBJECT DATE)])"
        ));
        Ok("Message-ID: <imap@synthetic.example>\r\nFrom: Recruiter <Recruiter@Example.com>\r\nSubject: =?utf-8?q?Technical_screen?=\r\nDate: Mon, 19 Oct 2026 09:00:00 +0000\r\n\r\n".into())
    }
    fn logout(&mut self) {
        self.calls.borrow_mut().push("logout".into());
    }
}

fn write_yaml(path: &Path, value: &Value) -> Result<()> {
    let context = crate::context()?;
    context
        .store()
        .write_yaml(path, &careerbrew_setup::value::to_canon(value))?;
    Ok(())
}

fn read_yaml(path: &Path) -> Result<Value> {
    let context = crate::context()?;
    Ok(careerbrew_setup::value::from_canon(
        &context
            .store()
            .read_yaml(path, careerbrew_setup::value::to_canon(&json!({})), true)?,
    ))
}

fn mail_readers(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "mail")?;
    let root = fixture_root();
    let graph = mail::graph(
        &json!({"name": "graph", "kind": "graph", "token": "env:CB_TEST_TOKEN"}),
        &mut fetcher(&root),
    )?;
    let jmap = mail::jmap(
        &json!({"name": "jmap", "kind": "jmap", "token": "env:CB_TEST_TOKEN"}),
        &mut fetcher(&root),
    )?;
    let calls = Rc::new(RefCell::new(vec!["connect:127.0.0.1:1143".to_string()]));
    let calls_by_factory = Rc::clone(&calls);
    let mut factory = move |host: &str, port: u16| {
        assert_eq!((host, port), ("127.0.0.1", 1143));
        Ok(Box::new(FakeImap {
            calls: Rc::clone(&calls_by_factory),
        }) as Box<dyn mail::ImapConnection>)
    };
    let imap = mail::imap(
        &json!({"name": "imap", "kind": "imap", "host": "127.0.0.1", "port": 1143, "user": "env:CB_TEST_TOKEN", "password": "env:CB_TEST_TOKEN"}),
        None,
        &mut factory,
    )?;
    let all = [graph.clone(), jmap.clone(), imap.clone()].concat();
    let classes = mail::classify(&all);
    write_yaml(
        &workspace.root.join("mail.yaml"),
        &json!({"accounts": [{"name": "personal", "kind": "graph", "machine": "personal"}, {"name": "work", "kind": "graph", "machine": "corporate"}]}),
    )?;
    let stored = read_yaml(&workspace.root.join("mail.yaml"))?;
    let corporate_accounts = crate::field(&stored, "accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|account| {
            matches!(
                crate::text(crate::field(account, "machine")).as_str(),
                "corporate" | "any"
            )
        })
        .map(|account| crate::text(crate::field(account, "name")))
        .collect::<Vec<_>>();
    let allowed = vec!["other.example".to_owned()];
    let wrong_host = mail::imap(&json!({"host": "mail.example.com", "user": "env:CB_TEST_TOKEN", "password": "env:CB_TEST_TOKEN"}), Some(&allowed), &mut factory).err().map(|error| error.class).unwrap_or("accepted");
    Ok(json!({
        "graph": graph.iter().map(mail::Message::to_json).collect::<Vec<_>>(),
        "jmap": jmap.iter().map(mail::Message::to_json).collect::<Vec<_>>(),
        "imap": imap.iter().map(mail::Message::to_json).collect::<Vec<_>>(),
        "imap_calls": calls.borrow().clone(),
        "classes": classes,
        "corporate_accounts": corporate_accounts,
        "wrong_host": wrong_host,
    }))
}

fn source_isolation(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "isolation")?;
    write_yaml(
        &workspace.root.join("day/calendars.yaml"),
        &json!({"calendars": [{"kind": "graph", "token": "env:CB_TEST_TOKEN"}, {"kind": "ics", "path": workspace.base.join("missing.ics")}]}),
    )?;
    let calendars = read_yaml(&workspace.root.join("day/calendars.yaml"))?;
    let mut events = Vec::new();
    let mut calendar_errors = Vec::new();
    for cfg in crate::field(&calendars, "calendars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let result = match crate::text(crate::field(cfg, "kind")).as_str() {
            "graph" => calendar::graph_events(
                cfg,
                NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
                calendar::zone("UTC")?,
                &mut fetcher(&fixture_root()),
            ),
            "ics" => calendar::ics_text(cfg, &fixture_root()).and_then(|text| {
                calendar::parse_ics(
                    &text,
                    NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(),
                    calendar::zone("UTC")?,
                    &[],
                )
            }),
            _ => Err(SourceError::new("KeyError", "unknown calendar")),
        };
        match result {
            Ok(mut found) => events.append(&mut found),
            Err(error) => calendar_errors.push(format!(
                "{}: {}",
                crate::text(crate::field(cfg, "kind")),
                error.class
            )),
        }
    }
    events.sort_by_key(|event| event.start);
    write_yaml(
        &workspace.root.join("mail.yaml"),
        &json!({"accounts": [{"name": "good", "kind": "graph", "token": "env:CB_TEST_TOKEN"}, {"name": "bad", "kind": "jmap", "token": "env:CB_TEST_TOKEN", "session_url": "https://absent.example.com/session"}]}),
    )?;
    let accounts = read_yaml(&workspace.root.join("mail.yaml"))?;
    let mut messages = Vec::new();
    let mut mail_errors = Vec::new();
    for account in crate::field(&accounts, "accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let result = match crate::text(crate::field(account, "kind")).as_str() {
            "graph" => mail::graph(account, &mut fetcher(&fixture_root())),
            "jmap" => mail::jmap(account, &mut fetcher(&fixture_root())),
            _ => Err(SourceError::new("KeyError", "unknown mail account")),
        };
        match result {
            Ok(mut found) => messages.append(&mut found),
            Err(error) => mail_errors.push(format!(
                "{}: {}",
                crate::text(crate::field(account, "name")),
                error.class
            )),
        }
    }
    let literal_ics = calendar::ics_text(
        &json!({"url": "https://calendar.example.com/private.ics"}),
        &fixture_root(),
    )
    .err()
    .map(|error| error.class)
    .unwrap_or("accepted");
    let literal_token = connectors::run(
        &json!({"kind": "github", "token": "literal-not-reported", "user": "me"}),
        "user",
        &mut fetcher(&fixture_root()),
        false,
    )
    .err()
    .map(|error| error.class)
    .unwrap_or("accepted");
    Ok(json!({
        "calendar_titles": events.iter().map(|event| event.title.clone()).collect::<Vec<_>>(),
        "calendar_errors": calendar_errors,
        "mail_ids": messages.iter().map(|message| message.id.clone()).collect::<Vec<_>>(),
        "mail_errors": mail_errors,
        "literal_ics": literal_ics,
        "literal_token": literal_token,
    }))
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn invoke(
    workspace: &Workspace,
    values: &[&str],
    control: cli::ConnectControl,
    keychain: &mut HashMap<String, String>,
) -> Result<Value> {
    let root = fixture_root();
    let mut store = |service: &str, value: &str| {
        keychain.insert(service.to_owned(), value.to_owned());
    };
    let arguments = args(values);
    let (stdout, exit) = cli::connect_using(&arguments[1..], &root, control, &mut store);
    let path = workspace.root.join("work/connectors.yaml");
    let saved = if path.exists() {
        crate::field(&read_yaml(&path)?, "connectors")
            .cloned()
            .unwrap_or(json!([]))
    } else {
        json!([])
    };
    Ok(json!({"exit": exit, "stdout": stdout, "saved": saved}))
}

fn cli_connect(base: &Path) -> Result<Value> {
    if std::env::var("CAREERBREW_RUST08_PLANT_WRITE").as_deref() == Ok("duplicate") {
        let path = base.join("isolation/data/mail.yaml");
        let bytes = std::fs::read(&path)?;
        std::fs::write(path, bytes)?;
    }
    let mut out = Map::new();
    let mut keychain = HashMap::from([(
        "careerbrew.outlook.refresh".to_owned(),
        "synthetic-refresh-not-reported-1".to_owned(),
    )]);
    let scoped = Workspace::new(base, "cli-scoped")?;
    out.insert(
        "scoped".into(),
        invoke(
            &scoped,
            &[
                "connect",
                "jira",
                "--ref",
                "env:CB_JIRA",
                "--base-url",
                "https://scoped.atlassian.example.com",
                "--email",
                "me@example.com",
            ],
            cli::ConnectControl::default(),
            &mut keychain,
        )?,
    );
    let bad = Workspace::new(base, "cli-bad")?;
    out.insert(
        "bad".into(),
        invoke(
            &bad,
            &[
                "connect",
                "jira",
                "--ref",
                "env:CB_JIRA",
                "--base-url",
                "https://bad.atlassian.example.com",
                "--email",
                "me@example.com",
            ],
            cli::ConnectControl::default(),
            &mut keychain,
        )?,
    );
    let missing = Workspace::new(base, "cli-missing")?;
    out.insert(
        "missing".into(),
        invoke(
            &missing,
            &[
                "connect",
                "jira",
                "--ref",
                "env:CB_JIRA",
                "--base-url",
                "https://jira.example.com",
            ],
            cli::ConnectControl::default(),
            &mut keychain,
        )?,
    );
    let interactive = Workspace::new(base, "cli-interactive")?;
    out.insert(
        "interactive".into(),
        invoke(
            &interactive,
            &["connect", "jira"],
            cli::ConnectControl {
                interactive: true,
                ..cli::ConnectControl::default()
            },
            &mut keychain,
        )?,
    );
    let denied = Workspace::new(base, "cli-denied")?;
    out.insert(
        "denied".into(),
        invoke(
            &denied,
            &[
                "connect",
                "jira",
                "--ref",
                "env:CB_JIRA",
                "--base-url",
                "https://jira.example.com",
                "--email",
                "me@example.com",
            ],
            cli::ConnectControl {
                allowed: false,
                ..cli::ConnectControl::default()
            },
            &mut keychain,
        )?,
    );
    let signed = Workspace::new(base, "cli-outlook")?;
    let outlook = invoke(
        &signed,
        &["connect", "outlook", "--client-id", "synthetic-client"],
        cli::ConnectControl {
            owner_terminal: true,
            ..cli::ConnectControl::default()
        },
        &mut keychain,
    )?;
    let rotated = keychain
        .get("careerbrew.outlook.refresh")
        .is_some_and(|value| value == "synthetic-refresh-token-2");
    let mut outlook = outlook;
    outlook["rotated"] = json!(rotated);
    out.insert("outlook".into(), outlook);
    let declined = Workspace::new(base, "cli-declined")?;
    out.insert(
        "declined".into(),
        invoke(
            &declined,
            &[
                "connect",
                "outlook",
                "--client-id",
                "synthetic-client",
                "--tenant",
                "synthetic-declined-tenant",
            ],
            cli::ConnectControl {
                owner_terminal: true,
                ..cli::ConnectControl::default()
            },
            &mut keychain,
        )?,
    );
    let no_client = Workspace::new(base, "cli-no-client")?;
    let mut no_client_answer = invoke(
        &no_client,
        &["connect", "outlook"],
        cli::ConnectControl::default(),
        &mut keychain,
    )?;
    no_client_answer
        .as_object_mut()
        .expect("CLI result")
        .remove("saved");
    out.insert("no_client".into(), no_client_answer);
    let manual = Workspace::new(base, "cli-manual")?;
    out.insert(
        "manual".into(),
        invoke(
            &manual,
            &["connect", "outlook", "--ref", "env:CB_TEST_TOKEN"],
            cli::ConnectControl::default(),
            &mut keychain,
        )?,
    );
    let piped = Workspace::new(base, "cli-piped")?;
    out.insert(
        "piped".into(),
        invoke(
            &piped,
            &["connect", "outlook", "--client-id", "synthetic-client"],
            cli::ConnectControl::default(),
            &mut keychain,
        )?,
    );
    Ok(Value::Object(out))
}

fn run(name: &str, base: &Path) -> Value {
    let result = match name {
        "http-policy" => http_policy(base),
        "connector-evidence" => connector_evidence(base),
        "calendar-recurrence" => calendar_recurrence(base),
        "mail-readers" => mail_readers(base),
        "source-isolation" => source_isolation(base),
        "cli-connect" => cli_connect(base),
        _ => Err(SourceError::value("unknown component case")),
    };
    json!({"id": name, "result": match result { Ok(value) => typed(value), Err(error) => json!({"error": error.class}) }})
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let root = std::env::var_os("CAREERBREW_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| SourceError::value("CAREERBREW_HOME is required"))?;
    let base = root.join("rust08-workload");
    std::fs::create_dir_all(&base)?;
    let cases = CASES
        .iter()
        .filter(|name| selected.is_none_or(|selected| selected == **name))
        .map(|name| run(name, &base))
        .collect::<Vec<_>>();
    let text = serde_json::to_string(&json!({"cases": cases}))? + "\n";
    if SECRET_MARKERS.iter().any(|marker| text.contains(marker)) {
        return Err(SourceError::new(
            "PrivacyError",
            "synthetic credential reached the component report",
        ));
    }
    Ok(text)
}
