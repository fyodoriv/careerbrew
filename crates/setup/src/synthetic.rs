//! Synthetic differential workload shared by the rust-07 candidate binary.

use crate::value;
use crate::{cli, machine, oauth, plugins, preflight, Result, SetupError};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "manifest-policy",
    "plugin-lifecycle",
    "command-ownership",
    "machine-readiness",
    "oauth-refs",
    "mcp-preflight",
    "managed-repair",
    "update-workflow",
    "cli-handlers",
];

struct Workspace {
    base: PathBuf,
    root: PathBuf,
    home: PathBuf,
    key: PathBuf,
    pair: PathBuf,
}

impl Workspace {
    fn new(base: &Path, name: &str) -> Result<Self> {
        let base = base.join(name);
        let root = base.join("data");
        let home = base.join("home");
        let key = base.join("keys/backup.key");
        let pair = base.join("keys/pair.key");
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&home)?;
        let workspace = Self {
            base,
            root,
            home,
            key,
            pair,
        };
        workspace.activate();
        Ok(workspace)
    }

    fn activate(&self) {
        for (key, value) in [
            ("HOME", self.home.clone()),
            ("CAREERBREW_HOME", self.root.clone()),
            ("CAREERBREW_KEY_FILE", self.key.clone()),
            ("CAREERBREW_PAIR_KEY_FILE", self.pair.clone()),
            ("CAREERBREW_COMMANDS_DIR", self.base.join("legacy-commands")),
        ] {
            std::env::set_var(key, value);
        }
        for (key, value) in [
            ("CAREERBREW_EVENTS", "off"),
            ("CAREERBREW_ENCRYPT", "on"),
            ("CAREERBREW_NOW", "2026-03-01T09:30:00+00:00"),
            ("TZ", "UTC"),
        ] {
            std::env::set_var(key, value);
        }
    }
}

fn manifest(name: &str) -> Value {
    json!({
        "name": name,
        "version": "1.0.0",
        "requires": ">=0.1.0",
        "provides": {"commands": "commands/"},
        "policy": {"allowed_routes": ["local"], "require_encrypted_backup": true},
        "work_steps": [{"step": "/review", "uses": ["browser:sso"], "timeout": 5}],
    })
}

fn facts(exists: bool) -> Value {
    json!({
        "symlinks": [],
        "provides": {"commands/": {"inside": true, "exists": exists}},
        "live": {},
        "terms_inside": true,
    })
}

fn manifest_policy(base_path: &Path) -> Result<Value> {
    let provider = Workspace::new(base_path, "manifest-provider")?;
    let real_version_refusal = match plugins::validate_manifest(
        &{
            let mut data = manifest("acme");
            value::set(&mut data, "requires", json!(">=1.0.0"));
            data
        },
        &provider.root,
    ) {
        Ok(_) => "accepted",
        Err(error) => error.class,
    };
    let valid = plugins::validate_manifest_data(
        &manifest("acme"),
        &facts(true),
        "1.0.0",
        &plugins::work_step_error,
    )?;
    let mut variants = Map::new();
    variants.insert("mapping".into(), json!([]));
    variants.insert("name".into(), json!({"name": "Bad", "version": "1.0.0"}));
    let mut version = manifest("acme");
    value::set(&mut version, "version", Value::Null);
    variants.insert("version".into(), version);
    let mut provides_shape = manifest("acme");
    value::set(&mut provides_shape, "provides", json!([]));
    variants.insert("provides-shape".into(), provides_shape);
    variants.insert("provides-missing".into(), manifest("acme"));
    let mut policy_list = manifest("acme");
    value::set(
        &mut policy_list,
        "policy",
        json!({"allowed_hosts": "github.com"}),
    );
    variants.insert("policy-list".into(), policy_list);
    let mut policy_bool = manifest("acme");
    value::set(&mut policy_bool, "policy", json!({"public_metrics": "no"}));
    variants.insert("policy-bool".into(), policy_bool);
    let mut export = manifest("acme");
    value::set(
        &mut export,
        "policy",
        json!({"export_to_personal": "everything"}),
    );
    variants.insert("export".into(), export);
    let mut requires = manifest("acme");
    value::set(&mut requires, "requires", json!(">=１.0"));
    variants.insert("requires".into(), requires);
    let mut requires_bool = manifest("acme");
    value::set(&mut requires_bool, "requires", json!(true));
    variants.insert("requires-bool".into(), requires_bool);
    let mut work_step = manifest("acme");
    value::set(
        &mut work_step,
        "work_steps",
        json!([{"step": "/review", "use": ["browser:sso"]}]),
    );
    variants.insert("work-step-field".into(), work_step);
    let mut detect = manifest("acme");
    value::set(&mut detect, "detect", json!({"env": "TOKEN"}));
    variants.insert("detect".into(), detect);
    let mut defaults = manifest("acme");
    value::set(&mut defaults, "connector_defaults", json!({"jira": []}));
    variants.insert("connector-defaults".into(), defaults);
    let mut reads = manifest("acme");
    value::set(&mut reads, "agent_reads", json!("slack"));
    variants.insert("agent-reads".into(), reads);
    let mut live = manifest("acme");
    value::set(&mut live, "live", json!({"other": "live/x.yaml"}));
    variants.insert("live".into(), live);
    let mut refusals = Map::new();
    for (name, data) in variants {
        let answer = plugins::validate_manifest_data(
            &data,
            &facts(name != "provides-missing"),
            "1.0.0",
            &plugins::work_step_error,
        );
        refusals.insert(
            name,
            json!(match answer {
                Ok(_) => "accepted",
                Err(error) => error.class,
            }),
        );
    }
    let team = json!({
        "name": "team",
        "extends": "acme",
        "policy": {
            "allowed_routes": ["local", "hosted"],
            "allowed_hosts": ["example.com"],
            "require_encrypted_backup": false,
            "agent_may_read_business": true,
            "public_metrics": false,
            "export_to_personal": "redacted",
            "backup_targets": ["~/Team"],
        }
    });
    let base = json!({
        "name": "acme",
        "policy": {
            "allowed_routes": ["local"],
            "allowed_hosts": ["example.com", "other.example"],
            "agent_may_read_business": true,
            "export_to_personal": "aggregates",
            "backup_targets": ["~/Base", "~/Team"],
        }
    });
    let expansions = BTreeMap::from([
        (
            "~/Base".into(),
            Path::new("/")
                .join("home/u/Base")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "~/Team".into(),
            Path::new("/")
                .join("home/u/Team")
                .to_string_lossy()
                .into_owned(),
        ),
    ]);
    let denied_team = {
        let mut denied = team.clone();
        value::set(
            value::get_mut(&mut denied, "policy").unwrap(),
            "agent_may_read_business",
            Value::Bool(false),
        );
        denied
    };
    let denied = plugins::merge_policy(&[base.clone(), denied_team], &expansions);
    let lock = json!({"plugins": {
        "team": {"extends": "acme"},
        "acme": {"extends": null},
        "cycle-a": {"extends": "cycle-b"},
        "cycle-b": {"extends": "cycle-a"},
        "unknown": {"extends": "absent"},
    }});
    let mut command_semantics = Map::new();
    for (name, field, command) in [
        ("verify-env", "verify", "env python3 -m unittest"),
        ("verify-shell", "verify", "sh -c 'python3 -m unittest'"),
        ("verify-managed", "verify", "uv run python -m unittest"),
        ("cycle-python", "cycle", "python3 -m unittest"),
        ("review-python", "review_ask_cycle", "python3 -m unittest"),
        ("cycle-unclosed", "cycle", "python3 -c 'unclosed"),
    ] {
        let data = {
            let mut data = manifest("acme");
            value::set(
                &mut data,
                "work_steps",
                json!([{"step": "/review", (field): command}]),
            );
            data
        };
        let answer = plugins::validate_manifest_data(
            &data,
            &facts(true),
            "1.0.0",
            &plugins::work_step_error,
        );
        command_semantics.insert(
            name.into(),
            json!(match answer {
                Ok(_) => "accepted",
                Err(error) => error.class,
            }),
        );
    }
    Ok(json!({
        "valid": valid["name"],
        "refusals": refusals,
        "merged": plugins::merge_policy(&[base, team], &expansions),
        "denied_agent_read": denied["agent_may_read_business"],
        "real_version_refusal": real_version_refusal,
        "command_semantics": command_semantics,
        "order": plugins::refresh_order(&lock),
    }))
}

fn write_plugin(root: &Path, name: &str, team: bool, command: bool) -> Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let mut provides = json!({"evidence_sources": "evidence.yaml"});
    if command {
        std::fs::create_dir(root.join("commands"))?;
        std::fs::write(
            root.join("commands").join(format!("{name}-check.md")),
            "---\ndescription: synthetic\n---\nRun careerbrew checkup.\n",
        )?;
        value::set(&mut provides, "commands", json!("commands/"));
    }
    let mut document = manifest(name);
    value::set(&mut document, "provides", provides);
    value::set(&mut document, "sensitive_terms", json!("terms.txt"));
    value::set(
        &mut document,
        "settings_defaults",
        json!({"ladder": format!("{name}-eng"), "target_level": "senior"}),
    );
    value::set(
        &mut document,
        "policy",
        json!({
            "allowed_routes": ["local"],
            "allowed_connectors": ["jira"],
            "allowed_hosts": ["example.com"],
            "secret_backends": ["keychain"],
            "require_encrypted_backup": true,
            "agent_may_read_business": false,
        }),
    );
    if team {
        value::set(&mut document, "extends", json!("acme"));
    }
    std::fs::write(
        root.join(plugins::MANIFEST),
        value::yaml_write(&document).map_err(SetupError::plugin)?,
    )?;
    std::fs::write(
        root.join("evidence.yaml"),
        "retained: keep\nsources:\n  - {kind: git, path: '~/repo', authors: [owner@example.com]}\n  - {kind: git, path: '/shared/repo'}\nconnectors:\n  - {kind: jira, token: 'keychain:shared'}\n",
    )?;
    std::fs::write(
        root.join("terms.txt"),
        "synthetic-company\nsynthetic-codename\n",
    )?;
    Ok(root.to_path_buf())
}

fn selected_entry(entry: &Value) -> Value {
    json!({
        "name": entry["name"],
        "version": entry["version"],
        "exec_allowed": entry["exec_allowed"],
        "origin": entry["origin"],
    })
}

fn plugin_lifecycle(base: &Path) -> Result<Value> {
    let w = Workspace::new(base, "plugin-lifecycle")?;
    let source = write_plugin(&w.base.join("source"), "acme", false, true)?;
    let added = plugins::add(&source, true)?;
    let listed = plugins::installed()?
        .iter()
        .map(|plugin| {
            json!([
                plugin.name,
                value::text(value::get(&plugin.manifest, "version")),
                value::bool_value(value::get(&plugin.entry, "exec_allowed")),
            ])
        })
        .collect::<Vec<_>>();
    let verified = plugins::verify()?;
    let policy = plugins::effective_policy()?;
    let team = write_plugin(&w.base.join("team"), "team", true, false)?;
    plugins::add(&team, false)?;
    let order = plugins::refresh_order(&plugins::lock(&crate::context()?)?);
    let dependent_refusal = match plugins::remove("acme", false) {
        Ok(_) => "accepted",
        Err(error) => error.class,
    };
    let installed_manifest = crate::context()?
        .root
        .join("plugins/acme/careerbrew-plugin.yaml");
    let mut original = std::fs::read(&installed_manifest)?;
    original.extend_from_slice(b"# tampered\n");
    std::fs::write(&installed_manifest, original)?;
    let tampered = plugins::verify()?;
    let refreshed = plugins::refresh()?;
    let verified_after = plugins::verify()?;
    let event_path = crate::context()?.root.join("work/events.jsonl");
    crate::context()?.store().write_jsonl(
        &event_path,
        &[
            value::to_canon(&json!({"id": "a", "origin": "plugin:acme"})),
            value::to_canon(&json!({"id": "b", "origin": "base"})),
        ],
    )?;
    crate::context()?.store().write_json(
        &crate::context()?.root.join("day/inbox.json"),
        &value::to_canon(&json!({"items": [{"id": "inbox-a", "origin": "plugin:acme"}, {"id": "inbox-b", "origin": "base"}]})),
    )?;
    let removed_team = plugins::remove("team", false)?;
    let removed_base = plugins::remove("acme", true)?;
    let purged_left = crate::context()?
        .store()
        .read_jsonl(&event_path, true)?
        .iter()
        .map(|row| value::from_canon(row)["id"].clone())
        .collect::<Vec<_>>();
    let inbox = value::from_canon(&crate::context()?.store().read_json(
        &crate::context()?.root.join("day/inbox.json"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    let inbox_left = value::get(&inbox, "items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| row["id"].clone())
        .collect::<Vec<_>>();
    let inbox_lock = crate::context()?.root.join("day/inbox.lock").is_file();
    let sources = Workspace::new(base, "plugin-sources")?;
    let home_source = write_plugin(&sources.home.join("home-plugin"), "acme", false, true)?;
    plugins::add_source("~/home-plugin", false)?;
    let home_ok = {
        let lock = plugins::lock(&crate::context()?)?;
        let expected = home_source.canonicalize()?.to_string_lossy().into_owned();
        value::get(&lock["plugins"]["acme"], "local_path").and_then(Value::as_str)
            == Some(expected.as_str())
    };
    plugins::remove("acme", false)?;
    let repo = write_plugin(&sources.base.join("git-source"), "acme", false, true)?;
    std::env::set_var("PATH", "/usr/bin:/bin");
    let run_git = |args: &[&str]| -> Result<()> {
        let status = std::process::Command::new("git")
            .args(args)
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(SetupError::io())
        }
    };
    run_git(&["init", "--quiet", &repo.to_string_lossy()])?;
    run_git(&[
        "-C",
        &repo.to_string_lossy(),
        "config",
        "user.name",
        "Synthetic",
    ])?;
    run_git(&[
        "-C",
        &repo.to_string_lossy(),
        "config",
        "user.email",
        "synthetic@example.com",
    ])?;
    run_git(&["-C", &repo.to_string_lossy(), "add", "-A"])?;
    run_git(&[
        "-C",
        &repo.to_string_lossy(),
        "commit",
        "--quiet",
        "-m",
        "synthetic",
    ])?;
    let uri = format!("file://{}", repo.to_string_lossy());
    plugins::add_source(&uri, false)?;
    let git_locked = {
        let lock = plugins::lock(&crate::context()?)?;
        value::get(&lock["plugins"]["acme"], "source").and_then(Value::as_str) == Some(uri.as_str())
            && value::get(&lock["plugins"]["acme"], "local_path") == Some(&Value::Null)
    };
    std::fs::write(repo.join("commands/acme-next.md"), "synthetic next\n")?;
    run_git(&["-C", &repo.to_string_lossy(), "add", "-A"])?;
    run_git(&[
        "-C",
        &repo.to_string_lossy(),
        "commit",
        "--quiet",
        "-m",
        "next",
    ])?;
    plugins::refresh()?;
    let git_refreshed = crate::context()?
        .root
        .join("plugins/acme/commands/acme-next.md")
        .is_file()
        && plugins::verify()?.into_iter().all(|(_, ok)| ok);
    plugins::remove("acme", false)?;
    let lock_empty = plugins::lock(&crate::context()?)? == json!({"plugins": {}});
    let tuples = |rows: Vec<(String, bool)>| {
        rows.into_iter()
            .map(|(name, ok)| json!([name, ok]))
            .collect::<Vec<_>>()
    };
    Ok(json!({
        "added": selected_entry(&added),
        "listed": listed,
        "verified": tuples(verified),
        "policy": policy,
        "order": order,
        "dependent_refusal": dependent_refusal,
        "tampered": tuples(tampered),
        "refreshed": refreshed,
        "verified_after": tuples(verified_after),
        "removed": [removed_team, removed_base],
        "purged_left": purged_left,
        "inbox_left": inbox_left,
        "inbox_lock": inbox_lock,
        "home_source": home_ok,
        "git_locked": git_locked,
        "git_refreshed": git_refreshed,
        "lock_empty": lock_empty,
    }))
}

fn files_named(root: &Path, name: &str, out: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            files_named(&path, name, out)?;
        } else if path.file_name().is_some_and(|found| found == name) {
            out.push(path);
        }
    }
    Ok(())
}

fn command_ownership(base: &Path) -> Result<Value> {
    let mut out = Map::new();
    let w = Workspace::new(base, "command-lifecycle")?;
    let source = write_plugin(&w.base.join("source"), "acme", false, true)?;
    plugins::add(&source, false)?;
    let fragment = w.root.join("agentbrew/Agentfile.yaml");
    let declared =
        value::yaml_read(&std::fs::read_to_string(&fragment)?).map_err(SetupError::plugin)?;
    let selected = value::strings(value::get(&declared, "commands"));
    let legacy = w.base.join("legacy-commands/acme-check.md");
    std::fs::create_dir_all(legacy.parent().unwrap())?;
    let installed = w.root.join("plugins/acme/commands/acme-check.md");
    std::fs::write(&legacy, std::fs::read(&installed)?)?;
    plugins::refresh()?;
    let mut archives = Vec::new();
    files_named(
        &w.root.join("command-archive"),
        "acme-check.md",
        &mut archives,
    )?;
    let readback = if archives.len() == 1 {
        crate::context()?
            .store()
            .read_text(&archives[0], None, true)?
            .unwrap_or_default()
    } else {
        String::new()
    };
    let lifecycle = json!({
        "selected": selected.len() == 1 && selected[0].ends_with("/acme/commands/acme-check.md"),
        "legacy_removed": !legacy.exists(),
        "archived": archives.len() == 1 && readback == std::fs::read_to_string(&installed)?,
        "archive_sealed": archives.len() == 1 && std::fs::read(&archives[0])?.starts_with(b"CBV1\n"),
        "fragment_mode": format!("{:03o}", std::fs::metadata(&fragment)?.permissions().mode() & 0o777),
    });
    plugins::remove("acme", false)?;
    let after =
        value::yaml_read(&std::fs::read_to_string(&fragment)?).map_err(SetupError::plugin)?;
    let mut lifecycle = lifecycle;
    value::set(
        &mut lifecycle,
        "empty_after_remove",
        Value::Bool(
            value::get(&after, "commands")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
        ),
    );
    out.insert("lifecycle".into(), lifecycle);

    let w = Workspace::new(base, "command-collision")?;
    let source = write_plugin(&w.base.join("source"), "acme", false, true)?;
    let legacy = w.base.join("legacy-commands/acme-check.md");
    std::fs::create_dir_all(legacy.parent().unwrap())?;
    std::fs::write(&legacy, "owner edit\n")?;
    let collision = match plugins::add(&source, false) {
        Ok(_) => json!({"error": "accepted"}),
        Err(error) => json!({
            "error": error.class,
            "kept": std::fs::read_to_string(&legacy)? == "owner edit\n",
            "installed": !plugins::installed()?.is_empty(),
        }),
    };
    out.insert("collision".into(), collision);

    let w = Workspace::new(base, "command-symlink")?;
    let source = write_plugin(&w.base.join("source"), "acme", false, true)?;
    std::os::unix::fs::symlink("acme-check.md", source.join("commands/alias.md"))?;
    let symlink = match plugins::add(&source, false) {
        Ok(_) => json!({"error": "accepted"}),
        Err(error) => json!({
            "error": error.class,
            "staging_removed": !w.root.join("plugins/.staging").exists(),
        }),
    };
    out.insert("symlink".into(), symlink);

    let w = Workspace::new(base, "metadata-symlink")?;
    let outside = w.base.join("outside");
    std::fs::create_dir(&outside)?;
    std::os::unix::fs::symlink(&outside, w.root.join("agentbrew"))?;
    let installed = w.root.join("plugins/acme");
    std::fs::create_dir_all(&installed)?;
    std::fs::write(installed.join("acme-check.md"), "synthetic command\n")?;
    std::fs::write(
        installed.join(plugins::MANIFEST),
        "name: acme\nversion: 1.0.0\nprovides: {commands: acme-check.md}\n",
    )?;
    let lock = json!({"plugins": {"acme": {"commands": ["acme-check.md"]}}});
    let metadata = match plugins::sync_command_sources(&lock) {
        Ok(_) => json!({"error": "accepted"}),
        Err(error) => json!({
            "error": error.class,
            "outside_empty": std::fs::read_dir(&outside)?.next().is_none(),
        }),
    };
    out.insert("metadata_symlink".into(), metadata);
    Ok(Value::Object(out))
}

fn readiness_facts() -> Value {
    json!({
        "role": "corporate",
        "profile_exists": true,
        "job_hunt_fields": false,
        "profile_name": "Synthetic Owner",
        "profile_employer": "Synthetic Company",
        "personal_data": false,
        "installed": true,
        "verified": true,
        "real_ladder": true,
        "active_origin": "plugin:acme",
        "target_level": "e5",
        "active_levels": ["e4", "e5"],
        "cloud": "",
        "synced_targets": [],
        "shared": [],
        "terms_count": 2,
        "scan_ok": true,
        "files_scanned": 42,
        "scan_files": [],
    })
}

fn machine_readiness(base: &Path) -> Result<Value> {
    let rows = machine::readiness_rows(&readiness_facts());
    let mut failed = readiness_facts();
    for (key, value) in [
        ("role", json!("personal")),
        ("personal_data", json!(true)),
        ("cloud", json!("Dropbox")),
        ("shared", json!(["plugin:acme:evidence.yaml"])),
        ("terms_count", json!(1)),
        ("scan_ok", json!(false)),
        ("scan_files", json!(["src/x.py"])),
    ] {
        value::set(&mut failed, key, value);
    }
    let failed_rows = machine::readiness_rows(&failed);
    let w = Workspace::new(base, "machine-role")?;
    machine::set_role("corporate", "work")?;
    std::fs::create_dir_all(w.root.join("out/reports"))?;
    for relative in [
        "out/interview-notes.md",
        "out/interview-notes.txt",
        "out/strategy.md",
        "out/progress.html",
        "out/other.md",
        "out/reports/run.html",
    ] {
        std::fs::write(w.root.join(relative), "synthetic")?;
    }
    machine::set_role("corporate", "work")?;
    let output_cleanup = [
        "out/interview-notes.md",
        "out/interview-notes.txt",
        "out/strategy.md",
        "out/progress.html",
        "out/other.md",
        "out/reports/run.html",
    ]
    .iter()
    .map(|name| ((*name).to_owned(), Value::Bool(w.root.join(name).exists())))
    .collect::<Map<String, Value>>();
    let tutor = w.root.join("tutor");
    std::fs::create_dir(&tutor)?;
    std::fs::write(tutor.join(".DS_Store"), [0])?;
    std::fs::write(tutor.join(".readiness.jsonl.lock"), "")?;
    machine::set_role("corporate", "work")?;
    let cleared = !tutor.exists();
    std::fs::create_dir(&tutor)?;
    std::fs::write(
        tutor.join(".readiness.jsonl.123.abc.tmp"),
        "synthetic partial file",
    )?;
    let temp_refusal = match machine::set_role("corporate", "") {
        Ok(_) => "accepted",
        Err(error) => error.class,
    };
    let repos = w.base.join("repos");
    for relative in ["a/.git", "group/b/.git", "group/deep/x/c/.git", "notrepo"] {
        std::fs::create_dir_all(repos.join(relative))?;
    }
    let clouds = [
        vec!["Dropbox".into(), "x".into()],
        vec![
            "Library".into(),
            "CloudStorage".into(),
            "OneDrive-Company".into(),
        ],
        vec!["Desktop".into(), "x".into()],
        vec!["local".into(), "x".into()],
    ];
    let icloud = BTreeSet::from(["Desktop".to_owned()]);
    let mut repo_names = machine::find_repos(&repos, 2)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    repo_names.sort();
    let goal_inputs = [
        json!({}),
        json!({"queue": [{"kind": "interview"}]}),
        json!({"history": [{"kind": "job-search"}]}),
        json!({"upcoming": {"kind": "interview"}}),
        json!({"queue": ["uncertain"]}),
        json!([]),
        json!({"queue": [{"kind": "promotion"}]}),
    ];
    let goal_counts = goal_inputs
        .iter()
        .map(machine::job_hunt_goals)
        .collect::<Vec<_>>();
    let mut goal_refusals = Map::new();
    for (name, data) in [
        ("history", &goal_inputs[2]),
        ("upcoming", &goal_inputs[3]),
        ("malformed", &goal_inputs[4]),
        ("safe", &goal_inputs[6]),
    ] {
        let workspace = Workspace::new(base, &format!("goals-{name}"))?;
        crate::context()?
            .store()
            .write_yaml(&workspace.root.join("goals.yaml"), &value::to_canon(data))?;
        let answer = match machine::set_role("corporate", "") {
            Ok(_) => "accepted",
            Err(error) => error.class,
        };
        goal_refusals.insert(name.into(), json!(answer));
    }
    Ok(json!({
        "ready_count": [rows.iter().filter(|row| row["ok"] == true).count(), rows.len()],
        "failed": failed_rows.iter().filter(|row| row["ok"] == false).map(|row| row["check"].clone()).collect::<Vec<_>>(),
        "guard": machine::guard_decision("jobs", "corporate"),
        "cloud": clouds.iter().map(|parts| machine::cloud_service(parts, &icloud)).collect::<Vec<_>>(),
        "per_user": [
            machine::per_user(&json!({"email": "x"})),
            machine::per_user(&json!({"path": "~/repo"})),
            machine::per_user(&json!({"path": "/shared/repo"})),
            machine::per_user(&json!([])),
        ],
        "cleared": cleared,
        "output_cleanup": output_cleanup,
        "temp_refusal": temp_refusal,
        "repos": repo_names,
        "goal_counts": goal_counts,
        "goal_refusals": goal_refusals,
        "work_profile": machine::work_profile("name: Synthetic\ntarget_salary: 1\nemployer: Example\n"),
    }))
}

fn oauth_refs(base: &Path) -> Result<Value> {
    let w = Workspace::new(base, "oauth")?;
    let polls = [
        json!({"access_token": "not-reported", "refresh_token": "not-reported"}),
        json!({"error": "authorization_pending"}),
        json!({"error": "slow_down"}),
        json!({"error": "authorization_declined"}),
        json!({"error": "expired_token"}),
        json!({}),
    ];
    let refreshes = [
        json!({"access_token": "not-reported"}),
        json!({"error": "invalid_grant"}),
        json!({"error": "interaction_required"}),
        json!({"error": "invalid_client"}),
        json!({}),
    ];
    let answer = json!({"access_token": "not-reported", "refresh_token": "rotated-not-reported", "expires_in": 3600});
    let reference = "keychain:careerbrew.outlook.refresh";
    let bearer_is_value = answer["access_token"] == "not-reported";
    let rotated = oauth::secret_backend(reference).is_ok_and(|kind| kind == "keychain")
        && value::bool_value(value::get(&answer, "refresh_token"));
    let fields = ["client_id", "grant_type", "refresh_token", "scope"];
    let mut secret_refs = Map::new();
    for (name, reference) in [
        ("keychain", "keychain:service"),
        ("op", "op://synthetic/item/field"),
        ("env", "env:SYNTHETIC"),
        ("literal", "not-a-reference"),
    ] {
        secret_refs.insert(
            name.into(),
            json!(oauth::secret_backend(reference).unwrap_or("SecretError")),
        );
    }
    oauth::write_reconnect(false)?;
    let state = value::from_canon(&crate::context()?.store().read_json(
        &w.root.join("work/outlook-auth.json"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    Ok(json!({
        "poll": polls.iter().map(oauth::classify_poll).collect::<Vec<_>>(),
        "refresh": refreshes.iter().map(oauth::classify_refresh).collect::<Vec<_>>(),
        "settings": [
            oauth::settings("", "", "setting-client", "setting-tenant", "plugin-client", "plugin-tenant"),
            oauth::settings("flag-client", "flag-tenant", "setting-client", "setting-tenant", "plugin-client", "plugin-tenant"),
        ],
        "secret_refs": secret_refs,
        "bearer_is_value": bearer_is_value,
        "rotated": rotated,
        "fields": [fields],
        "reconnect_cleared": !value::bool_value(value::get(&state, "reconnect")),
        "state_exists": w.root.join("work/outlook-auth.json").exists(),
    }))
}

fn mcp_preflight() -> Result<Value> {
    let github = json!({"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}});
    let local = json!({"url": "http://localhost:8765/mcp"});
    let command = json!({"command": "missing-synthetic-tool"});
    let servers = json!({"github": github, "local": local, "command": command});
    let facts = json!({"servers": {
        "github": {"vars": {"GITHUB_TOKEN": "not-reported-old"}, "github_token": "not-reported-new"},
        "local": {"vars": {}, "port_open": false},
        "command": {"vars": {}, "command_found": false},
    }});
    let rows = preflight::check_rows(&servers, &facts, "claude");
    let empty_facts =
        json!({"servers": {"github": {"vars": {"GITHUB_TOKEN": ""}, "ownership_error": null}}});
    let repair = preflight::check_rows(&json!({"github": github}), &empty_facts, "claude");
    let native = preflight::check_rows(&json!({"github": github}), &empty_facts, "codex");
    let strict = [
        "{\"mcpServers\": []}",
        "{\"mcpServers\": {\"x\": {\"enabled\": \"yes\"}}}",
        "{\"mcpServers\": {\"x\": {\"headers\": []}}}",
    ]
    .iter()
    .map(|text| match preflight::servers(text, true) {
        Ok(_) => "accepted",
        Err(_) => "ValueError",
    })
    .collect::<Vec<_>>();
    let foreign = json!({"url": "https://github.com.example.test/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}});
    let existing = json!({"url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}"}, "headersHelper": "owner"});
    Ok(json!({
        "rows": rows,
        "repair": repair,
        "native": native,
        "strict": strict,
        "auth": [preflight::github_auth_header(&github), preflight::github_auth_header(&foreign), preflight::github_auth_header(&existing)],
        "vars": preflight::vars_in(&github),
    }))
}

fn config(base: &Path, name: &str) -> Result<(Workspace, PathBuf, PathBuf)> {
    let w = Workspace::new(base, name)?;
    let target = w.base.join("owned-config.json");
    let data = json!({"unrelated": {"keep": true}, "mcpServers": {"fixture": {"type": "http", "url": "https://api.githubcopilot.com/mcp", "headers": {"Authorization": "Bearer ${GITHUB_TOKEN}", "X-Test": "keep"}}}});
    std::fs::write(&target, data.to_string())?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640))?;
    let link = w.base.join("claude.json");
    std::os::unix::fs::symlink(&target, &link)?;
    Ok((w, link, target))
}

fn managed_repair(base: &Path) -> Result<Value> {
    let mut out = Map::new();
    let (w, link, target) = config(base, "repair")?;
    let before = std::fs::read(&target)?;
    let confirmation = match preflight::fix_github_headers(
        false,
        &link,
        Some(""),
        Path::new("/synthetic/tools/gh"),
    ) {
        Ok(_) => json!({"error": "accepted"}),
        Err(error) => json!({"error": error.class, "unchanged": std::fs::read(&target)? == before}),
    };
    out.insert("confirmation".into(), confirmation);
    let result =
        preflight::fix_github_headers(true, &link, Some(""), Path::new("/synthetic/tools/gh"))?;
    let data: Value = serde_json::from_slice(&std::fs::read(&target)?)?;
    let success = json!({
        "result": result,
        "link": link.is_symlink() && std::fs::read_link(&link)? == target,
        "mode": format!("{:03o}", std::fs::metadata(&target)?.permissions().mode() & 0o777),
        "unrelated": data["unrelated"] == json!({"keep": true}),
        "headers": data["mcpServers"]["fixture"]["headers"] == json!({"X-Test": "keep"}),
        "helper": data["mcpServers"]["fixture"].get("headersHelper").is_some(),
        "temporary_left": std::fs::read_dir(&w.base)?.flatten().any(|entry| entry.file_name().to_string_lossy().starts_with(".careerbrew-mcp-") && entry.file_name().to_string_lossy().ends_with(".tmp")),
    });
    let second =
        preflight::fix_github_headers(true, &link, Some(""), Path::new("/synthetic/tools/gh"))?;
    let mut success = success;
    value::set(&mut success, "second", second);
    let unknown =
        preflight::fix_github_headers(true, &link, None, Path::new("/synthetic/tools/gh"))?;
    out.insert("success".into(), success);
    out.insert("unknown_probe".into(), unknown);

    let (w, link, target) = config(base, "managed")?;
    let state = w.home.join(".config/agentbrew/state.yaml");
    std::fs::create_dir_all(state.parent().unwrap())?;
    std::fs::write(&state, "mcpServers:\n  - name: fixture\n    source: user\n")?;
    let before = std::fs::read(&target)?;
    let managed = match preflight::fix_github_headers(
        true,
        &link,
        Some(""),
        Path::new("/synthetic/tools/gh"),
    ) {
        Ok(_) => json!({"error": "accepted"}),
        Err(error) => {
            json!({"error": error.class, "unchanged": std::fs::read(&target)? == before, "link": link.is_symlink()})
        }
    };
    out.insert("managed".into(), managed);

    let (_w, link, target) = config(base, "foreign")?;
    let mut data: Value = serde_json::from_slice(&std::fs::read(&target)?)?;
    value::set(
        &mut data["mcpServers"]["fixture"],
        "url",
        json!("https://github.com.example.test/mcp"),
    );
    std::fs::write(&target, data.to_string())?;
    let before = std::fs::read(&target)?;
    let mut foreign =
        preflight::fix_github_headers(true, &link, Some(""), Path::new("/synthetic/tools/gh"))?;
    value::set(
        &mut foreign,
        "unchanged",
        Value::Bool(std::fs::read(&target)? == before),
    );
    out.insert("foreign".into(), foreign);
    Ok(Value::Object(out))
}

fn update_workflow(base: &Path) -> Result<Value> {
    let w = Workspace::new(base, "update-workflow")?;
    let launcher = w.base.join("bin/careerbrew");
    std::fs::create_dir_all(launcher.parent().unwrap())?;
    std::fs::write(
        &launcher,
        "#!/bin/sh\n# careerbrew launcher, written by scripts/install-launcher.sh\n",
    )?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))?;
    std::env::set_var("CAREERBREW_BIN_DIR", launcher.parent().unwrap());
    let ready = preflight::launcher_ready();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o600))?;
    let not_executable = preflight::launcher_ready();
    let repair_ok = w.base.join("repair-ok");
    let repair_fail = w.base.join("repair-fail");
    std::fs::write(&repair_ok, "#!/bin/sh\nexit 0\n")?;
    std::fs::write(&repair_fail, "#!/bin/sh\nexit 1\n")?;
    std::fs::set_permissions(&repair_ok, std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(&repair_fail, std::fs::Permissions::from_mode(0o700))?;
    let repairs = [
        preflight::repair(&[repair_ok.to_string_lossy().into_owned()], &w.base),
        preflight::repair(&[repair_fail.to_string_lossy().into_owned()], &w.base),
        preflight::repair(
            &[w.base.join("missing").to_string_lossy().into_owned()],
            &w.base,
        ),
    ];
    let repo = w.base.join("repo");
    std::fs::create_dir_all(repo.join("commands"))?;
    std::fs::write(repo.join("Agentfile.yaml"), "commands: [./commands]\n")?;
    let source = repo.join("commands/work.md");
    std::fs::write(&source, "# Work\n\nRun the plan.\n")?;
    let installed = w.base.join("installed.md");
    std::fs::write(&installed, std::fs::read_to_string(&source)?)?;
    let report = json!({"version": 1, "commands": [{"sourcePath": source.canonicalize()?.to_string_lossy(), "targets": [{"agent": "claude-code", "ready": true, "status": "ready", "path": installed.to_string_lossy()}]}]});
    let agentfile = w.base.join("plugin-Agentfile.yaml");
    std::fs::write(&agentfile, "workSteps: [/missing]\n")?;
    let rows = preflight::workflow_check(
        &[json!({"step": "/work", "path": source.to_string_lossy()})],
        &repo,
        &[agentfile],
        &report,
        &BTreeSet::from(["claude-code".into()]),
    )?;
    Ok(json!({"launcher": [ready, not_executable], "repairs": repairs, "workflow": rows}))
}

fn normalized(mut text: String, base: &Path) -> String {
    text = text.replace(&base.to_string_lossy().into_owned(), "<work>");
    text
}

fn handler(output: Result<String>, base: &Path) -> Value {
    match output {
        Ok(text) => json!({"exit": 0, "stdout": normalized(text, base)}),
        Err(error) => json!({"exit": 1, "stdout": error.class}),
    }
}

fn cli_handlers(base: &Path) -> Result<Value> {
    let w = Workspace::new(base, "cli-handlers")?;
    let mut outputs = Map::new();
    outputs.insert("init".into(), handler(cli::cmd_init(), &w.base));
    let source = write_plugin(&w.base.join("source"), "acme", false, true)?;
    outputs.insert(
        "plugin-add".into(),
        handler(cli::cmd_plugin_add(&source, true), &w.base),
    );
    outputs.insert(
        "plugin-list".into(),
        handler(cli::cmd_plugin_list(), &w.base),
    );
    let verified = cli::cmd_plugin_verify();
    outputs.insert(
        "plugin-verify".into(),
        match verified {
            Ok((text, code)) => json!({"exit": code, "stdout": normalized(text, &w.base)}),
            Err(error) => json!({"exit": 1, "stdout": error.class}),
        },
    );
    outputs.insert(
        "machine-set".into(),
        handler(cli::cmd_machine_set("corporate", "work"), &w.base),
    );
    outputs.insert(
        "machine-show".into(),
        handler(cli::cmd_machine_show(), &w.base),
    );
    let document = json!({"private_note": "keep", "connectors": [
        {"kind": "jira", "token": "keychain:careerbrew.jira"},
        {"kind": "jira", "token": "op://synthetic/item/field"},
        {"kind": "jira", "token": "env:SYNTHETIC"},
    ]});
    crate::context()?.store().write_yaml(
        &w.root.join("work/connectors.yaml"),
        &value::to_canon(&document),
    )?;
    let sources = json!({"private_note": "keep", "sources": [{"kind": "git", "path": "~/repo", "authors": ["owner@example.com"]}]});
    crate::context()?.store().write_yaml(
        &w.root.join("work/sources.yaml"),
        &value::to_canon(&sources),
    )?;
    outputs.insert(
        "connectors-list".into(),
        handler(cli::cmd_connectors_list(), &w.base),
    );
    outputs.insert("pair-init".into(), handler(cli::cmd_pair_init(), &w.base));
    let split = cli::cmd_machine_split_plugin_evidence(true);
    outputs.insert(
        "split-evidence".into(),
        match split {
            Ok((text, code)) => json!({"exit": code, "stdout": normalized(text, &w.base)}),
            Err(error) => json!({"exit": 1, "stdout": error.class}),
        },
    );
    let split_live = cli::cmd_machine_split_plugin_evidence(false);
    outputs.insert(
        "split-evidence-live".into(),
        match split_live {
            Ok((text, code)) => json!({"exit": code, "stdout": normalized(text, &w.base)}),
            Err(error) => json!({"exit": 1, "stdout": error.class}),
        },
    );
    let source_data = value::from_canon(&crate::context()?.store().read_yaml(
        &source.join("evidence.yaml"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    let private_sources = value::from_canon(&crate::context()?.store().read_yaml(
        &w.root.join("work/sources.yaml"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    let private_connectors = value::from_canon(&crate::context()?.store().read_yaml(
        &w.root.join("work/connectors.yaml"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    outputs.insert(
        "split_fields_preserved".into(),
        Value::Bool(
            source_data["retained"] == "keep"
                && private_sources["private_note"] == "keep"
                && private_connectors["private_note"] == "keep"
                && source_data["sources"].as_array().map_or(0, Vec::len) == 1
                && source_data["connectors"].as_array().map_or(0, Vec::len) == 1
                && private_sources["sources"].as_array().map_or(0, Vec::len) == 1,
        ),
    );
    outputs.insert(
        "plugin-refresh".into(),
        handler(cli::cmd_plugin_refresh(), &w.base),
    );
    outputs.insert(
        "plugin-remove".into(),
        handler(cli::cmd_plugin_remove("acme", false), &w.base),
    );
    outputs.insert("pair_key_not_printed".into(), Value::Bool(w.pair.is_file()));
    let invalid = Workspace::new(base, "cli-invalid-secret")?;
    crate::context()?.store().write_yaml(
        &invalid.root.join("work/connectors.yaml"),
        &value::to_canon(&json!({"connectors": [{"kind": "jira", "token": "literal-value"}]})),
    )?;
    outputs.insert(
        "connectors-list-invalid".into(),
        handler(cli::cmd_connectors_list(), &invalid.base),
    );
    Ok(Value::Object(outputs))
}

fn run(selected: Option<&str>) -> Result<Value> {
    let base = std::env::var_os("CAREERBREW_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| SetupError::value("CAREERBREW_HOME is required"))?
        .join("rust07-workload");
    std::fs::create_dir_all(&base)?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|selected| selected != *name) {
            continue;
        }
        let result = match *name {
            "manifest-policy" => manifest_policy(&base),
            "plugin-lifecycle" => plugin_lifecycle(&base),
            "command-ownership" => command_ownership(&base),
            "machine-readiness" => machine_readiness(&base),
            "oauth-refs" => oauth_refs(&base),
            "mcp-preflight" => mcp_preflight(),
            "managed-repair" => managed_repair(&base),
            "update-workflow" => update_workflow(&base),
            "cli-handlers" => cli_handlers(&base),
            _ => unreachable!(),
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => json!({"error": error.class}),
        };
        if let Ok(plant) = std::env::var("CAREERBREW_RUST07_PLANT_WRITE") {
            match (plant.as_str(), *name) {
                ("extra", "manifest-policy") => {
                    std::fs::write(base.join("planted-extra"), "synthetic")?;
                }
                ("missing", "plugin-lifecycle") => {
                    std::fs::remove_file(base.join("plugin-lifecycle/data/plugins.lock.json"))?;
                }
                ("duplicate", "managed-repair") => {
                    let path = base.join("plugin-lifecycle/data/agentbrew/Agentfile.yaml");
                    let bytes = std::fs::read(&path)?;
                    std::fs::write(path, bytes)?;
                }
                _ => {}
            }
        }
        cases.push(json!({"id": name, "result": value::typed(&result)}));
    }
    let text = serde_json::to_string(&json!({"cases": cases})).unwrap() + "\n";
    for marker in [
        "not-reported-old",
        "not-reported-new",
        "rotated-not-reported",
    ] {
        if text.contains(marker) {
            return Err(SetupError::value(
                "synthetic credential reached the native report",
            ));
        }
    }
    Ok(Value::String(text))
}

pub fn run_all() -> Result<String> {
    match run(None)? {
        Value::String(text) => Ok(text),
        _ => unreachable!(),
    }
}

pub fn run_case(name: &str) -> Result<String> {
    if !CASES.contains(&name) {
        return Err(SetupError::value("unknown synthetic case"));
    }
    match run(Some(name))? {
        Value::String(text) => Ok(text),
        _ => unreachable!(),
    }
}
