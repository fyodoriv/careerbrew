//! Synthetic rust-11 workload over the native component transactions.

use crate::{
    cli, field, memory_sync, project_import, read_jsonl, research, text, write_jsonl, Result,
};
use careerbrew_projects::projects;
use serde_json::{json, Map, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const CASES: &[&str] = &[
    "source-identity",
    "project-import",
    "memory-in",
    "memory-out",
    "memory-removal",
    "research-cli",
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
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::fs::create_dir_all(base.join("home"))?;
        std::env::set_var("HOME", base.join("home"));
        std::env::set_var("CAREERBREW_HOME", &root);
        std::env::set_var("CAREERBREW_KEY_FILE", base.join("keys/backup.key"));
        std::env::set_var("CAREERBREW_EVENTS", "off");
        std::env::set_var("CAREERBREW_ENCRYPT", "off");
        std::env::set_var("CAREERBREW_NOW", "2026-10-21T16:00:00+00:00");
        std::env::set_var("CAREERBREW_TEST_OWNER", "1");
        std::env::set_var(
            "CAREERBREW_HTTP_FIXTURES",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http"),
        );
        std::env::set_var("CB_T", "synthetic");
        std::env::set_var("TZ", "UTC");
        Ok(Self { base, root })
    }
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(path)
}

fn error(result: Result<()>) -> String {
    result
        .err()
        .map(|error| error.class)
        .unwrap_or("accepted")
        .to_owned()
}

fn source_identity(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "source-identity")?;
    let root = &workspace.root;
    let refs = [
        "t001",
        "#C0123",
        "#billing-eng",
        "PROJ-7",
        "https://tracker.example.com/browse/PROJ-9",
        "https://github.com/acme/billing/pull/41/files?review=1#comment",
        "https://github.example.com/acme/billing/issues/7?x=1",
        "https://github.com/acme/billing/tree/main/src",
        "https://docs.google.com/document/d/doc-prd-1/edit",
        "https://example.slack.com/archives/C0123/p123",
        "https://example.com/post",
        "acme/billing",
        "https://github.com/acme/billing/pull/٤١",
    ];
    let detected = refs
        .iter()
        .map(|reference| research::detect(reference))
        .collect::<Result<Vec<_>>>()?;
    let items = refs
        .iter()
        .map(|reference| research::github_item(reference))
        .collect::<Vec<_>>();
    projects::add(root, "billing", Default::default())?;
    projects::add_source(root, "billing", "tracker", "PROJ-1")?;
    projects::add(root, "search", Default::default())?;
    projects::add_source(root, "search", "repo", "acme/search")?;
    let closest = research::match_project(root, "tracker", "PROJ-9", "")?;
    projects::add_source(root, "search", "tracker", "PROJ-2")?;
    let tied = error(research::match_project(root, "tracker", "PROJ-9", "").map(|_| ()));
    let after = [
        "2026-10-05T06:00:00.000-0400",
        "2026-10-05T04:00:00.000-0400",
        "",
        "bad",
        "2026-10-05T09:00:00",
        "a+😀",
    ]
    .into_iter()
    .map(|stamp| research::after(stamp, Some("2026-10-05T09:00:00+00:00")))
    .collect::<Vec<_>>();
    Ok(json!({
        "detected": detected,
        "items": items,
        "closest": closest,
        "tied": tied,
        "bad": error(research::detect("not a source").map(|_| ())),
        "after": after,
        "unicode_digest": crate::digest("café ٢"),
    }))
}

fn project_view(root: &Path, name: &str) -> Result<Value> {
    let every = projects::load(root)?;
    let row = &every[name];
    let sources = row["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| {
            json!([
                text(field(source, "kind")),
                text(field(source, "ref")),
                !text(field(source, "checked_at")).is_empty(),
                text(field(source, "status"))
            ])
        })
        .collect::<Vec<_>>();
    Ok(
        json!({"title": text(field(row, "title")), "group": text(field(row, "group")), "parent": text(field(row, "parent")), "links": row.get("links").cloned().unwrap_or_else(|| json!([])), "sources": sources}),
    )
}

fn project_import_case(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "project-import")?;
    if std::env::var("CAREERBREW_RUST11_PLANT_WRITE").as_deref() == Ok("extra") {
        std::fs::write(workspace.root.join("planted-extra.txt"), "synthetic")?;
    }
    let root = &workspace.root;
    let sheet = fixture("projects/team-goals.csv");
    let sheet = sheet.to_string_lossy().into_owned();
    let dry = project_import::run(root, &sheet, true)?;
    let dry_empty = projects::load(root)?.as_object().is_some_and(Map::is_empty);
    let first = project_import::run(root, &sheet, false)?;
    let mut initial = Map::new();
    for name in ["billing", "checkout-analytics", "payments"] {
        initial.insert(name.into(), project_view(root, name)?);
    }
    projects::check(root, "billing", Some("BILL-1"), "epic 3 of 5")?;
    let again = workspace.base.join("again.csv");
    std::fs::write(&again, "name,title,group,parent,links,trackers\nbilling,Billing revamp phase 2,,,,BILL-1 BILL-9\npayments,Payments platform,q4-cutline,,,PAY-1\n")?;
    let second = project_import::run(root, &again.to_string_lossy(), false)?;
    let checked = project_view(root, "billing")?;
    let cutline = project_import::run(
        root,
        &fixture("projects/cutline.json").to_string_lossy(),
        false,
    )?;
    let sheets = vec![
        project_import::sheet_csv_url(
            "https://docs.google.com/spreadsheets/d/abc_1-x/edit?usp=sharing#gid=42",
        )?,
        project_import::sheet_csv_url(
            "https://docs.google.com/spreadsheets/d/e/2PACX-pub/pubhtml",
        )?,
    ];
    let remote = project_import::run(
        root,
        "https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit#gid=0",
        false,
    )?;
    let remote_bom = project_import::run(
        root,
        "https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit",
        false,
    )?;
    let bad = workspace.base.join("bad.csv");
    std::fs::write(&bad, "name,title,parent,links\nalpha,Alpha,beta,\nbeta,Beta,alpha,\ngamma,Gamma,nowhere,gamma\n,No name,,\nalpha,Again,,\ndelta,Delta,,ghost\n")?;
    let bad_error = error(project_import::run(root, &bad.to_string_lossy(), false).map(|_| ()));
    let before = projects::load(root)?;
    let bad_cli = cli::project_import_command(root, &bad.to_string_lossy(), false);
    let unchanged = projects::load(root)? == before;
    let cli_result = cli::project_import_command(root, &sheet, true);
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    let private = workspace.base.join("private.csv");
    std::fs::write(&private, "name,title\nzebra,PROJECTZEBRA rollout\n")?;
    let private_cli = cli::project_import_command(root, &private.to_string_lossy(), false);
    let sealed = std::fs::read(projects::projects_path(root))?.starts_with(b"CBV1\n");
    let sealed_readback = projects::load(root)?
        .get("zebra")
        .and_then(|project| field(project, "title"))
        .and_then(Value::as_str)
        == Some("PROJECTZEBRA rollout");
    let canary_hidden = !text(field(&private_cli, "stdout")).contains("PROJECTZEBRA");
    let missing = workspace.base.join("missing.csv");
    let bad_inputs = vec![
        error(project_import::run(root, &missing.to_string_lossy(), false).map(|_| ())),
        error(project_import::run(root, "https://sheets.example.com/goals.csv", false).map(|_| ())),
    ];
    if std::env::var("CAREERBREW_RUST11_PLANT_WRITE").as_deref() == Ok("missing") {
        std::fs::remove_file(projects::projects_path(root))?;
    }
    Ok(json!({
        "dry": dry, "dry_empty": dry_empty, "first": first, "initial": initial, "second": second,
        "checked": checked, "cutline": cutline, "sheets": sheets, "remote": remote, "remote_bom": remote_bom,
        "bad_error": bad_error, "bad_cli": bad_cli, "unchanged": unchanged, "cli": cli_result,
        "private_cli": private_cli, "sealed": sealed, "sealed_readback": sealed_readback, "canary_hidden": canary_hidden, "bad_inputs": bad_inputs,
    }))
}

fn set_setting(root: &Path, key: &str, value: Value) -> Result<()> {
    let path = root.join("settings.yaml");
    careerbrew_vault::lock::with_lock(&path, || {
        let mut settings = careerbrew_projects::read_yaml(root, "settings.yaml", json!({}))?;
        settings[key] = if key == "project_priority" && value.is_string() {
            json!([value])
        } else {
            value
        };
        careerbrew_projects::write_yaml(root, "settings.yaml", &settings)
    })?
}

fn counts(value: Value) -> Value {
    value
}

fn memory_in(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "memory-in")?;
    let root = &workspace.root;
    let mem = workspace.base.join("agent-memory/billing-research-log.md");
    std::fs::create_dir_all(mem.parent().expect("memory parent"))?;
    std::fs::write(&mem, "# billing research log\n\nNotes.\n")?;
    projects::add(root, "search", Default::default())?;
    projects::add(root, "billing", Default::default())?;
    set_memory(root, "billing", &mem.to_string_lossy())?;
    projects::add_source(root, "billing", "tracker", "PROJ-1")?;
    let initial = memory_sync::sync_in(root)?;
    std::fs::write(&mem, format!("{}{}", std::fs::read_to_string(&mem)?, "\nstatus: schema frozen, migration next week\ntrackers: PROJ-7\nchannels: #eng-billing\nlinks: search, no-such-project\nPROJ-44 is blocked on the UTF-8 fix (ISO-8601 dates)\n- design doc: https://docs.google.com/document/d/doc-billing-9/edit\n- repo: [billing](https://github.com/acme/billing/pull/41).\n"))?;
    let changed = memory_sync::sync_in(root)?;
    let repeat = memory_sync::sync_in(root)?;
    let modified = std::fs::metadata(&mem)?.modified()?;
    set_mtime(&mem, modified + Duration::from_secs(3600))?;
    let touched = memory_sync::sync_in(root)?;
    std::fs::write(
        &mem,
        format!("{}trackers: PROJ-8\n", std::fs::read_to_string(&mem)?),
    )?;
    set_mtime(&mem, modified + Duration::from_secs(3600))?;
    let same_mtime = memory_sync::sync_in(root)?;
    research::write_memory(
        root,
        "billing",
        "2026-10-21 doc doc-billing-9: PRD",
        &["Decision: freeze on 2026-10-20 https://example.com/owned".into()],
    )?;
    let own_write = memory_sync::sync_in(root)?;
    std::fs::write(
        &mem,
        format!(
            "{}status: owner final\n- repo: https://github.com/acme/ledger\n",
            std::fs::read_to_string(&mem)?
        ),
    )?;
    let owner_after = memory_sync::sync_in(root)?;
    let every = projects::load(root)?;
    let mut sources = every["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| json!([text(field(source, "kind")), text(field(source, "ref"))]))
        .collect::<Vec<_>>();
    sources.sort_by_key(|source| source.to_string());
    let memory = every["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|source| text(field(source, "kind")) == "memory")
        .expect("memory source");
    let status = text(field(memory, "status"));
    let links = every["billing"]
        .get("links")
        .cloned()
        .unwrap_or_else(|| json!([]));
    set_setting(root, "project_memory_sync", json!(false))?;
    std::fs::write(
        &mem,
        format!("{}trackers: PROJ-10\n", std::fs::read_to_string(&mem)?),
    )?;
    research::write_memory(
        root,
        "billing",
        "2026-10-21 doc other: PRD",
        &["see https://example.com/off".into()],
    )?;
    set_setting(root, "project_memory_sync", json!(true))?;
    let resumed = memory_sync::sync_in(root)?;
    let every = projects::load(root)?;
    let mut resumed_sources = every["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| json!([text(field(source, "kind")), text(field(source, "ref"))]))
        .collect::<Vec<_>>();
    resumed_sources.sort_by_key(|source| source.to_string());
    Ok(
        json!({"initial": counts(initial), "changed": changed, "repeat": repeat, "touched": touched, "same_mtime": same_mtime, "own_write": own_write, "owner_after": owner_after, "sources": sources, "links": links, "status": status, "resumed": resumed, "resumed_sources": resumed_sources}),
    )
}

fn set_mtime(path: &Path, modified: SystemTime) -> Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_times(std::fs::FileTimes::new().set_modified(modified))?;
    Ok(())
}

fn set_memory(root: &Path, name: &str, memory: &str) -> Result<()> {
    projects::with_lock(root, || {
        let mut every = projects::load(root)?;
        every[name]["memory"] = json!(memory);
        let sources = every[name]["sources"].as_array_mut().expect("source list");
        if !sources.iter().any(|source| {
            text(field(source, "kind")) == "memory" && text(field(source, "ref")) == memory
        }) {
            sources
                .push(json!({"kind": "memory", "ref": memory, "checked_at": null, "status": ""}));
        }
        projects::save(root, &every)
    })
}

fn seed_items(root: &Path) -> Result<()> {
    write_jsonl(
        root,
        "work/feed/events.jsonl",
        &[
            json!({"id": "slack:C0BILL:1792594800.000300", "source": "slack", "kind": "mention", "at": "2026-10-21T14:00:00Z", "title": "Mention from sam in #eng-billing", "channel": "eng-billing", "url": "https://example.slack.com/archives/C0BILL/p1", "to_me": true, "triage": "act"}),
            json!({"id": "pagerduty:Q9", "source": "pagerduty", "kind": "page", "at": "2026-10-21T15:00:00Z", "title": "Page: Invoice export failing", "url": "https://example.pagerduty.com/incidents/Q9", "to_me": true, "triage": "act"}),
            json!({"id": "slack:C0RAND:2", "source": "slack", "kind": "message", "at": "2026-10-21T14:10:00Z", "title": "ana in #random", "channel": "random", "triage": "fyi"}),
            json!({"id": "slack:C0BILL:3", "source": "slack", "kind": "message", "at": "2026-10-21T14:20:00Z", "title": "monitor in #eng-billing", "channel": "eng-billing", "triage": "noise"}),
            json!({"id": "slack:D0PAT:4", "source": "slack", "kind": "dm", "at": "2026-10-21T14:30:00Z", "title": "PROJ-1 retro moved", "personal": true, "triage": "fyi"}),
            json!({"id": "slack:BAD:5", "source": "slack", "kind": "message", "at": "a+😀", "title": "PROJ-1 malformed time", "triage": "fyi"}),
        ],
    )?;
    write_jsonl(
        root,
        "work/evidence.jsonl",
        &[
            json!({"id": "jira:PROJ-7", "kind": "jira", "title": "PROJ-7: Freeze the schema", "at": "2026-10-20T12:00:00+00:00", "url": "https://jira.example.com/browse/PROJ-7"}),
            json!({"id": "pr:acme/billing#41", "kind": "pr", "title": "feat: invoice export", "at": "2026-10-19T10:00:00Z", "url": "https://github.com/acme/billing/pull/41"}),
            json!({"id": "slack:1792594800.000300", "kind": "slack", "title": "Recognition in #eng-billing", "at": "2026-10-21T14:00:00Z", "url": "https://example.slack.com/archives/C0BILL/p1"}),
            json!({"id": "jira:PROJ-11", "kind": "jira", "title": "PROJ-11: later instant", "at": "2026-10-21T09:30:00-04:00", "url": "https://jira.example.com/browse/PROJ-11"}),
            json!({"id": "jira:PROJ-12", "kind": "jira", "title": "PROJ-12: earlier instant", "at": "2026-10-21T14:00:00+02:00", "url": "https://jira.example.com/browse/PROJ-12"}),
            json!({"id": "jira:PROJ-13", "kind": "jira", "title": "PROJ-13: local date", "at": "2026-10-21T23:30:00-04:00", "url": "https://jira.example.com/browse/PROJ-13"}),
            json!({"id": "jira:OPS-9", "kind": "jira", "title": "OPS-9: Reindex", "at": "2026-10-20T12:00:00+00:00", "url": "https://jira.example.com/browse/OPS-9"}),
            json!({"id": "jira:PROJ-3", "kind": "jira", "title": "PROJ-3: Old work", "at": "2026-09-01T12:00:00+00:00", "url": "https://jira.example.com/browse/PROJ-3"}),
        ],
    )
}

fn notes(path: &Path) -> Result<Vec<String>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .filter(|line| line.starts_with("- 2026-"))
        .map(str::to_owned)
        .collect())
}

fn memory_out(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "memory-out")?;
    let root = &workspace.root;
    let mem = workspace.base.join("billing.md");
    std::fs::write(&mem, "# billing\n")?;
    projects::add(root, "billing", Default::default())?;
    set_memory(root, "billing", &mem.to_string_lossy())?;
    projects::add_source(root, "billing", "tracker", "PROJ-1")?;
    projects::add_source(root, "billing", "chat", "#eng-billing")?;
    projects::add_source(root, "billing", "repo", "acme/billing")?;
    projects::add_source(
        root,
        "billing",
        "url",
        "https://github.com/acme/billing/pull/41",
    )?;
    projects::add(root, "other", Default::default())?;
    set_memory(
        root,
        "other",
        &workspace.base.join("other.md").to_string_lossy(),
    )?;
    projects::add_source(
        root,
        "other",
        "url",
        "https://github.com/acme/billing/pull/4",
    )?;
    set_setting(root, "project_priority", json!("billing=*invoice*"))?;
    seed_items(root)?;
    let denied = memory_sync::sync_out(root, false)?;
    let first = memory_sync::sync_out(root, true)?;
    let second = memory_sync::sync_out(root, true)?;
    let lines = notes(&mem)?;
    let other = workspace.base.join("other.md");
    let other_text = std::fs::read_to_string(other).unwrap_or_default();
    let state = root.join("work/memory-sync.json");
    let before_state = std::fs::read(&state)?;
    let idle = memory_sync::sync_out(root, true)?;
    let idle_state = std::fs::read(&state)? == before_state;
    std::fs::remove_file(&state)?;
    let rebuilt = memory_sync::sync_out(root, true)?;
    let cli_allowed = cli::project_sync_command(root, false);
    projects::add(
        root,
        "ledger",
        projects::AddOptions {
            parent: "billing".into(),
            ..Default::default()
        },
    )?;
    projects::add_source(root, "ledger", "tracker", "LED-4")?;
    let mut evidence = read_jsonl(root, "work/evidence.jsonl")?;
    evidence.push(json!({"id": "jira:LED-4", "kind": "jira", "title": "LED-4: Ledger cutover", "at": "2026-10-20T13:00:00Z", "url": "https://jira.example.com/browse/LED-4"}));
    write_jsonl(root, "work/evidence.jsonl", &evidence)?;
    let child = memory_sync::sync_out(root, true)?;
    let child_header = std::fs::read_to_string(&mem)?.contains("(project ledger)");
    let own_write_reread = memory_sync::sync_in(root)?;
    Ok(json!({
        "denied": denied, "first": first, "second": second, "lines": lines,
        "other_unmatched": !other_text.contains("pull/4") && !other_text.contains("invoice export"),
        "idle_state": idle_state, "idle": idle, "rebuilt": rebuilt, "cli": cli_allowed,
        "child": child, "child_header": child_header, "own_write_reread": own_write_reread,
    }))
}

fn memory_removal(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "memory-removal")?;
    let root = &workspace.root;
    let mem = workspace.base.join("billing.md");
    std::fs::write(&mem, "# billing\n")?;
    std::fs::set_permissions(&mem, std::fs::Permissions::from_mode(0o600))?;
    projects::add(root, "billing", Default::default())?;
    set_memory(root, "billing", &mem.to_string_lossy())?;
    projects::add_source(root, "billing", "tracker", "PROJ-1")?;
    write_jsonl(
        root,
        "work/feed/events.jsonl",
        &[
            json!({"id": "slack:C0BILL:1", "source": "slack", "kind": "mention", "at": "2026-10-21T14:00:00Z", "title": "PROJ-1 review", "url": "https://example.com/item", "triage": "act"}),
        ],
    )?;
    let seeded = memory_sync::sync_out(root, true)?;
    let mut rows = read_jsonl(root, "work/feed/events.jsonl")?;
    rows[0]["personal"] = json!(true);
    write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    let removed = memory_sync::sync_out_with(root, true, |path| {
        let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
        std::io::Write::write_all(&mut file, b"trackers: PROJ-9\n")?;
        Ok(())
    })?;
    let kept_append = std::fs::read_to_string(&mem)?.ends_with("trackers: PROJ-9\n");
    let mode = std::fs::metadata(&mem)?.permissions().mode() & 0o777;
    let read_append = memory_sync::sync_in(root)?;
    let append_source = projects::load(root)?["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|source| {
            text(field(source, "kind")) == "tracker" && text(field(source, "ref")) == "PROJ-9"
        });
    rows[0]["personal"] = json!(false);
    write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    std::fs::remove_file(root.join("work/memory-sync.json"))?;
    memory_sync::sync_out(root, true)?;
    rows[0]["personal"] = json!(true);
    write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    let refused = memory_sync::sync_out_with(root, true, |path| {
        let text = std::fs::read_to_string(path)?;
        std::fs::write(path, format!("# owner rewrite\n{text}"))?;
        Ok(())
    })?;
    let owner_kept = std::fs::read_to_string(&mem)?.starts_with("# owner rewrite\n");
    let retry = memory_sync::sync_out(root, true)?;
    let final_has_note = std::fs::read_to_string(&mem)?.contains("PROJ-1 review");
    Ok(
        json!({"seeded": seeded, "removed": removed, "kept_append": kept_append, "mode": mode, "read_append": read_append, "append_source": append_source, "refused": refused, "owner_kept": owner_kept, "retry": retry, "final_has_note": final_has_note}),
    )
}

fn research_cli(base: &Path) -> Result<Value> {
    let workspace = Workspace::new(base, "research-cli")?;
    let root = &workspace.root;
    let local = workspace.base.join("kickoff.md");
    std::fs::write(&local, "# notes\n- Decision: cut over on 2026-11-15\n")?;
    set_mtime(
        &local,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1760000000),
    )?;
    projects::add(root, "billing", Default::default())?;
    set_memory(root, "billing", "work/billing-notes.md")?;
    projects::add_source(root, "billing", "doc", &local.to_string_lossy())?;
    projects::add_source(root, "billing", "url", "https://outlook.office.com/mail/x")?;
    projects::add_source(
        root,
        "billing",
        "url",
        "https://github.com/acme/billing/pull/41",
    )?;
    projects::check(root, "billing", Some(&local.to_string_lossy()), "")?;
    projects::check(
        root,
        "billing",
        Some("https://github.com/acme/billing/pull/41"),
        "",
    )?;
    let before = projects::load(root)?["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| {
            json!([
                text(field(source, "ref")),
                field(source, "checked_at").cloned().unwrap_or(Value::Null)
            ])
        })
        .collect::<Vec<_>>();
    let changes = research::changes(root, "billing", None, true)?;
    let after = projects::load(root)?["billing"]["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| {
            json!([
                text(field(source, "ref")),
                field(source, "checked_at").cloned().unwrap_or(Value::Null)
            ])
        })
        .collect::<Vec<_>>();
    let absorbed = research::absorb(
        root,
        "https://github.com/acme/billing/pull/41?query=ignored#fragment",
        Some("billing"),
        false,
    )?;
    let cli_absorb = cli::project_absorb_command(
        root,
        "billing",
        "https://github.com/acme/billing/issues/41",
        false,
    );
    let cli_since = cli::project_since_command(root, "billing", None, true, false);
    let bad_since = cli::project_since_command(root, "billing", None, true, true);
    let alias = research::changes(
        root,
        "billing",
        Some("https://github.com/acme/billing/pull/41/files"),
        true,
    )?;
    let cli_alias = cli::project_since_command(
        root,
        "billing",
        Some("https://github.com/acme/billing/pull/41/files"),
        true,
        false,
    );
    Ok(
        json!({"changes": changes, "stamps_unchanged": before == after, "absorbed": absorbed, "cli_absorb": cli_absorb, "cli_since": cli_since, "bad_since": bad_since, "alias": alias, "cli_alias": cli_alias}),
    )
}

fn run(case: &str, base: &Path) -> Result<Value> {
    match case {
        "source-identity" => source_identity(base),
        "project-import" => project_import_case(base),
        "memory-in" => memory_in(base),
        "memory-out" => memory_out(base),
        "memory-removal" => memory_removal(base),
        "research-cli" => research_cli(base),
        _ => Err(crate::error("ValueError", "unknown rust-11 case")),
    }
}

pub fn run_all(selected: Option<&str>) -> Result<String> {
    let base = crate::root()?.join("rust11-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for case in CASES {
        if selected.is_none_or(|selected| selected == *case) {
            cases.push(json!({"id": case, "result": run(case, &base)?}));
        }
    }
    if std::env::var("CAREERBREW_RUST11_PLANT_WRITE").as_deref() == Ok("duplicate") {
        let path = base.join("source-identity/data/work/projects.yaml");
        let bytes = std::fs::read(&path)?;
        std::fs::write(path, bytes)?;
    }
    let report = crate::typed(&json!({"cases": cases}));
    let encoded = careerbrew_vault::json_yaml::json_compact_python(
        &careerbrew_setup::value::to_canon(&report),
    )
    .map_err(|_| crate::error("ValueError", "cannot encode synthetic report"))?;
    Ok(encoded + "\n")
}
