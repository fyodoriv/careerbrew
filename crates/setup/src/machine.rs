//! Component port of machine roles, corporate readiness and evidence ownership.

use crate::value;
use crate::{context, Result, SetupError};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const PERSONAL_DIRS: &[&str] = &["jobs", "tutor", "prep", "linkedin"];
pub const JOB_HUNT_FIELDS: &[&str] = &["target_salary", "remote_preference", "want_to_learn"];
pub const PER_USER_CHECK: &str =
    "the company plugin evidence file holds no per-person values (emails, logins, home paths)";
const PERSONAL_ONLY: &[&str] = &[
    "jobs",
    "drill",
    "interview",
    "interview-patterns",
    "prep",
    "cards",
    "library",
    "import",
    "indie",
];

pub fn work_profile(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| {
            let key = line.split(':').next().unwrap_or("").trim();
            !JOB_HUNT_FIELDS.contains(&key)
        })
        .collect()
}

pub fn has_job_hunt_fields(text: &str) -> bool {
    let Ok(data) = value::yaml_read(text) else {
        return true;
    };
    if data.is_null() {
        return text
            .lines()
            .any(|line| !line.trim().is_empty() && !line.trim().starts_with('#'));
    }
    data.as_object()
        .is_none_or(|map| map.keys().any(|key| JOB_HUNT_FIELDS.contains(&key.trim())))
}

pub fn info() -> Result<Value> {
    let ctx = context()?;
    let value = ctx.store().read_yaml(
        &ctx.root.join("machine.yaml"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?;
    Ok(value::from_canon(&value))
}

pub fn role() -> Result<String> {
    let info = info()?;
    Ok(value::get(&info, "role")
        .and_then(Value::as_str)
        .unwrap_or("personal")
        .to_owned())
}

fn data_files(folder: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_dir() {
            data_files(&path, out);
        } else if path.is_file() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name != ".DS_Store" && !(name.starts_with('.') && name.ends_with(".lock")) {
                out.push(path);
            }
        }
    }
}

fn profile_state() -> Result<(PathBuf, String, bool)> {
    let ctx = context()?;
    let path = ctx.root.join("profile.yaml");
    let text = ctx
        .store()
        .read_text(&path, Some(""), true)?
        .unwrap_or_default();
    let example =
        std::fs::read_to_string(std::env::current_dir()?.join("profile/examples/profile.yaml"))?;
    Ok((path, text.clone(), text.trim() == example.trim()))
}

fn clear_personal_dirs() -> Result<()> {
    let ctx = context()?;
    let examples = std::env::current_dir()?.join("profile/examples");
    for name in PERSONAL_DIRS {
        let folder = ctx.root.join(name);
        if !folder.exists() {
            continue;
        }
        let mut files = Vec::new();
        data_files(&folder, &mut files);
        for file in files {
            let example = examples.join(file.strip_prefix(&ctx.root).unwrap());
            let current = ctx.store().read_text(&file, None, true)?;
            if !example.exists()
                || std::fs::read_to_string(example).ok().as_ref() != current.as_ref()
            {
                return Err(SetupError::role(format!(
                    "job-hunt data found in {name}/; move it to your personal machine, then retry"
                )));
            }
        }
        std::fs::remove_dir_all(folder)?;
    }
    let out = ctx.root.join("out");
    if out.exists() {
        for entry in std::fs::read_dir(&out)? {
            let path = entry?.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if path.is_file()
                && (matches!(name.as_ref(), "strategy.md" | "progress.html")
                    || (name.starts_with("interview-") && name.ends_with(".md")))
            {
                std::fs::remove_file(path)?;
            }
        }
    }
    let reports = out.join("reports");
    if reports.exists() {
        for entry in std::fs::read_dir(reports)? {
            let path = entry?.path();
            if path.is_file() {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn check_goals() -> Result<()> {
    let ctx = context()?;
    let path = ctx.root.join("goals.yaml");
    if !path.exists() {
        return Ok(());
    }
    let goals = value::from_canon(&ctx.store().read_yaml(
        &path,
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    let found = value::get(&goals, "queue")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|goal| {
            matches!(
                value::get(goal, "kind").and_then(Value::as_str),
                Some("interview" | "job-search")
            )
        });
    if found {
        return Err(SetupError::role(
            "job-hunt goals found in goals.yaml; move the file to your personal machine, then retry",
        ));
    }
    Ok(())
}

pub fn set_role(new: &str, label: &str) -> Result<Value> {
    if !matches!(new, "personal" | "corporate") {
        return Err(SetupError::value("role must be personal or corporate"));
    }
    let ctx = context()?;
    if new == "corporate" {
        let (_path, text, untouched) = profile_state()?;
        if !untouched && has_job_hunt_fields(&text) {
            return Err(SetupError::role(
                "profile.yaml has job-hunt fields or is not a YAML mapping; move them to your personal machine and delete them here, then retry",
            ));
        }
        check_goals()?;
        clear_personal_dirs()?;
        let (path, text, untouched) = profile_state()?;
        if untouched {
            ctx.store().write_text(&path, &work_profile(&text))?;
        }
    }
    let mut data = info()?;
    if !data.is_object() {
        data = json!({});
    }
    value::set(&mut data, "role", json!(new));
    let old_label = value::get(&data, "label")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    value::set(
        &mut data,
        "label",
        json!(if label.is_empty() {
            if old_label.is_empty() {
                new
            } else {
                old_label.as_str()
            }
        } else {
            label
        }),
    );
    ctx.store()
        .write_yaml(&ctx.root.join("machine.yaml"), &value::to_canon(&data))?;
    Ok(data)
}

pub fn guard_decision(command_group: &str, machine_role: &str) -> Option<String> {
    if PERSONAL_ONLY.contains(&command_group) && machine_role == "corporate" {
        Some(format!(
            "`{command_group}` is personal-only and this machine's role is corporate. Run it on your personal machine (job-hunt data never goes on employer hardware)."
        ))
    } else {
        None
    }
}

pub fn cloud_service(parts: &[String], icloud_dirs: &BTreeSet<String>) -> String {
    if parts.starts_with(&["Library".into(), "Mobile Documents".into()]) {
        return "iCloud Drive".into();
    }
    if parts.starts_with(&["Library".into(), "CloudStorage".into()]) && parts.len() > 2 {
        return parts[2].split('-').next().unwrap_or_default().into();
    }
    if let Some(first) = parts.first() {
        if matches!(
            first.as_str(),
            "Dropbox" | "Google Drive" | "OneDrive" | "Box" | "pCloud Drive" | "Sync"
        ) || first.starts_with("OneDrive")
        {
            return first.clone();
        }
        if matches!(first.as_str(), "Desktop" | "Documents") && icloud_dirs.contains(first) {
            return "iCloud Desktop & Documents".into();
        }
    }
    String::new()
}

pub fn per_user(item: &Value) -> bool {
    item.as_object().is_some_and(|map| {
        ["email", "user", "authors"]
            .iter()
            .any(|key| map.contains_key(*key))
            || map.get("path").and_then(Value::as_str).is_some_and(|path| {
                path.starts_with('~')
                    || Path::new(path).components().nth(1).is_some_and(|part| {
                        matches!(part.as_os_str().to_str(), Some("Users" | "home"))
                    })
            })
    })
}

fn evidence_documents() -> Result<Vec<(String, String, PathBuf, Value)>> {
    let ctx = context()?;
    let mut out = Vec::new();
    for plugin in crate::plugins::installed()? {
        let Some(paths) = value::get(&plugin.manifest, "provides")
            .and_then(Value::as_object)
            .and_then(|items| items.get("evidence_sources"))
        else {
            continue;
        };
        for item in value::strings(Some(paths)) {
            let path = plugin.root.join(item);
            let document = value::from_canon(&ctx.store().read_yaml(
                &path,
                careerbrew_vault::canon::Canon::Map(Vec::new()),
                true,
            )?);
            out.push((
                plugin.name.clone(),
                format!("plugin:{}", plugin.name),
                path,
                document,
            ));
        }
    }
    Ok(out)
}

pub fn per_user_values_in_plugins() -> Result<Vec<String>> {
    let mut found = Vec::new();
    for (_name, origin, path, document) in evidence_documents()? {
        let flagged = ["sources", "connectors"].into_iter().any(|key| {
            value::get(&document, key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(per_user)
        });
        if flagged {
            found.push(format!(
                "{origin}:{}",
                path.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    Ok(found)
}

fn current_evidence_total() -> Result<usize> {
    let ctx = context()?;
    let plugin_total = evidence_documents()?
        .iter()
        .map(|(_, _, _, document)| {
            ["sources", "connectors"]
                .into_iter()
                .map(|key| {
                    value::get(document, key)
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len)
                })
                .sum::<usize>()
        })
        .sum::<usize>();
    let private_total = ["sources", "connectors"]
        .into_iter()
        .map(|key| {
            let path = ctx.root.join("work").join(format!("{key}.yaml"));
            let document = ctx.store().read_yaml(
                &path,
                careerbrew_vault::canon::Canon::Map(Vec::new()),
                true,
            )?;
            let document = value::from_canon(&document);
            Ok(value::get(&document, key)
                .and_then(Value::as_array)
                .map_or(0, Vec::len))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    Ok(plugin_total + private_total)
}

/// Move or preview per-person evidence without printing its values.
pub fn split_plugin_evidence(dry_run: bool) -> Result<Value> {
    let ctx = context()?;
    let mut rows = Vec::new();
    let mut mine = Map::new();
    let mut had = Map::new();
    let mut private_documents = Map::new();
    for key in ["sources", "connectors"] {
        let path = ctx.root.join("work").join(format!("{key}.yaml"));
        let document = value::from_canon(&ctx.store().read_yaml(
            &path,
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        let items = value::get(&document, key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        had.insert(key.into(), json!(items.len()));
        mine.insert(key.into(), Value::Array(items));
        private_documents.insert(key.into(), document);
    }
    let documents = evidence_documents()?;
    let total = current_evidence_total()?;
    let mut moved_total = 0;
    let mut edits = Vec::new();
    let mut reinstall = BTreeSet::new();
    for (name, origin, installed_path, _document) in documents {
        let plugin = crate::plugins::installed()?
            .into_iter()
            .find(|plugin| plugin.name == name)
            .ok_or_else(|| SetupError::plugin("plugin disappeared"))?;
        let local = value::get(&plugin.entry, "local_path")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let relative = installed_path.strip_prefix(&plugin.root).unwrap();
        let Some(folder) = local.filter(|folder| folder.is_dir()) else {
            if per_user_values_in_plugins()?.contains(&format!(
                "{origin}:{}",
                installed_path.file_name().unwrap().to_string_lossy()
            )) {
                rows.push(json!({"plugin": name, "file": relative.to_string_lossy(), "skipped": "no plugin folder on this machine to edit"}));
            }
            continue;
        };
        let target = folder.join(relative);
        let source_document = value::from_canon(&ctx.store().read_yaml(
            &target,
            careerbrew_vault::canon::Canon::Map(Vec::new()),
            true,
        )?);
        let mut moved = Map::new();
        for key in ["sources", "connectors"] {
            moved.insert(
                key.into(),
                Value::Array(
                    value::get(&source_document, key)
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|item| {
                            per_user(item)
                                && value::get(item, "kind").and_then(Value::as_str)
                                    != Some("command")
                        })
                        .cloned()
                        .collect(),
                ),
            );
        }
        let count = moved
            .values()
            .map(|v| v.as_array().map_or(0, Vec::len))
            .sum::<usize>();
        if count == 0 {
            continue;
        }
        let before = ["sources", "connectors"]
            .into_iter()
            .map(|key| {
                value::get(&source_document, key)
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len)
            })
            .sum::<usize>();
        moved_total += count;
        rows.push(json!({
            "plugin": name,
            "file": relative.to_string_lossy(),
            "before": before,
            "after": before - count,
            "sources": moved["sources"].as_array().unwrap().len(),
            "connectors": moved["connectors"].as_array().unwrap().len(),
        }));
        let mut updated = source_document.clone();
        for key in ["sources", "connectors"] {
            let existing = mine[key].as_array_mut().unwrap();
            for item in moved[key].as_array().unwrap() {
                if !existing.contains(item) {
                    existing.push(item.clone());
                }
            }
            if let Some(items) = value::get(&source_document, key).and_then(Value::as_array) {
                value::set(
                    &mut updated,
                    key,
                    Value::Array(
                        items
                            .iter()
                            .filter(|item| !moved[key].as_array().unwrap().contains(item))
                            .cloned()
                            .collect(),
                    ),
                );
            }
        }
        edits.push((target, updated));
        reinstall.insert(name.clone());
    }
    let had_edits = !edits.is_empty();
    if !dry_run && had_edits {
        for key in ["sources", "connectors"] {
            if mine[key].as_array().unwrap().len() != had[key].as_u64().unwrap_or(0) as usize {
                let path = ctx.root.join("work").join(format!("{key}.yaml"));
                let mut document = private_documents[key].clone();
                value::set(&mut document, key, mine[key].clone());
                ctx.store().write_yaml(&path, &value::to_canon(&document))?;
            }
        }
        for (target, document) in edits {
            let header = "# Shared evidence sources only. Per-person sources and connectors live in each person's private\n# work/sources.yaml and work/connectors.yaml (`careerbrew machine split-plugin-evidence`).\n";
            std::fs::write(
                target,
                header.to_owned() + &value::yaml_write(&document).map_err(SetupError::plugin)?,
            )?;
        }
        for name in reinstall {
            let plugin = crate::plugins::installed()?
                .into_iter()
                .find(|plugin| plugin.name == name)
                .unwrap();
            crate::plugins::add(
                Path::new(
                    value::get(&plugin.entry, "local_path")
                        .and_then(Value::as_str)
                        .unwrap(),
                ),
                value::bool_value(value::get(&plugin.entry, "exec_allowed")),
            )?;
        }
    }
    let added = ["sources", "connectors"]
        .into_iter()
        .map(|key| mine[key].as_array().unwrap().len() - had[key].as_u64().unwrap_or(0) as usize)
        .sum::<usize>();
    let after = if dry_run || !had_edits {
        total - moved_total + added
    } else {
        current_evidence_total()?
    };
    let left = per_user_values_in_plugins()?;
    Ok(json!({
        "plugins": rows,
        "mine": {
            "sources": [had["sources"], mine["sources"].as_array().unwrap().len()],
            "connectors": [had["connectors"], mine["connectors"].as_array().unwrap().len()],
        },
        "total": [total, after],
        "left": left,
        "dry_run": dry_run,
    }))
}

pub fn find_repos(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut frontier = vec![root.to_path_buf()];
    for _ in 0..=depth {
        let mut next = Vec::new();
        for folder in frontier {
            if folder.join(".git").exists() {
                out.push(folder);
                continue;
            }
            if folder.is_dir() {
                let mut children = std::fs::read_dir(folder)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.is_dir()
                            && !path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .starts_with('.')
                    })
                    .collect::<Vec<_>>();
                children.sort();
                next.extend(children);
            }
        }
        frontier = next;
    }
    out
}

pub fn readiness_rows(facts: &Value) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut row = |ok: bool, check: String, fix: String| {
        rows.push(json!({"ok": ok, "check": check, "fix": if ok { "" } else { &fix }}));
    };
    let flag = |key: &str| value::bool_value(value::get(facts, key));
    let text = |key: &str| value::text(value::get(facts, key));
    row(
        text("role") == "corporate",
        "machine role is corporate".into(),
        "careerbrew machine set corporate --label <name>".into(),
    );
    row(
        flag("profile_exists") && !flag("job_hunt_fields"),
        "profile.yaml exists and has no job-hunt fields".into(),
        "careerbrew init, then delete job-hunt fields from profile.yaml".into(),
    );
    row(
        !matches!(text("profile_name").as_str(), "" | "Your Name")
            && !matches!(text("profile_employer").as_str(), "" | "Your Employer"),
        "profile.yaml has your real name and company (both become leak terms)".into(),
        "set name and employer with `careerbrew data set profile.yaml --name '<name>' --employer '<employer>'`".into(),
    );
    row(
        !flag("personal_data"),
        "no job-hunt folders on this machine".into(),
        "careerbrew machine set corporate (it removes untouched examples and refuses real job-hunt data)".into(),
    );
    row(
        flag("installed"),
        "a company plugin is installed".into(),
        "careerbrew plugin new <company> --connect jira,github,slack (see docs/corporate-machine.md)".into(),
    );
    row(
        flag("verified"),
        "installed plugins match their lock hashes".into(),
        "careerbrew plugin refresh".into(),
    );
    row(
        flag("real_ladder"),
        "a plugin ladder holds real criteria (not the generic copy)".into(),
        "edit ladders/*.yaml in the plugin with the company's promotion criteria (promo-research skill), then careerbrew plugin refresh".into(),
    );
    row(
        text("active_origin").starts_with("plugin:"),
        "the active ladder comes from a plugin".into(),
        "set `ladder: <org>-eng` with `careerbrew data edit settings.yaml` (plugin new does this)"
            .into(),
    );
    let levels = value::strings(value::get(facts, "active_levels"));
    let level = text("target_level");
    row(
        levels.contains(&level),
        format!("target_level '{level}' is a level of the active ladder"),
        format!("set target_level with `careerbrew data edit settings.yaml` to one of {levels:?}"),
    );
    let cloud = text("cloud");
    row(
        cloud.is_empty(),
        format!(
            "the data folder is not uploaded by a cloud service{}",
            if cloud.is_empty() {
                String::new()
            } else {
                format!(" (found: {cloud})")
            }
        ),
        "move it out of synced folders: set CAREERBREW_HOME to ~/careerbrew-data and move the folder there".into(),
    );
    row(
        value::get(facts, "synced_targets")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "no backup target is in a cloud-synced folder".into(),
        "set backup_targets with `careerbrew data edit settings.yaml` to a local or company-managed folder".into(),
    );
    let shared = value::strings(value::get(facts, "shared"));
    row(
        shared.is_empty(),
        PER_USER_CHECK.into(),
        format!(
            "move them out of {} into your private files, as they are, with `careerbrew machine split-plugin-evidence`; a coworker gets their own with `careerbrew machine new-user`",
            shared[..shared.len().min(3)].join(", ")
        ),
    );
    row(
        value::get(facts, "terms_count").and_then(Value::as_u64).unwrap_or(0) >= 2,
        "sensitive terms list the company plus its codenames".into(),
        "add project codenames, customers, and internal hosts to the plugin's sensitive-terms.txt, then careerbrew plugin refresh".into(),
    );
    let files = value::strings(value::get(facts, "scan_files"));
    row(
        flag("scan_ok"),
        format!(
            "the careerbrew repo has no company terms ({} files scanned)",
            value::get(facts, "files_scanned")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        format!(
            "move company content out of: {}; it belongs in the plugin",
            files[..files.len().min(5)].join(", ")
        ),
    );
    rows
}

pub fn suggest() -> Result<Option<String>> {
    for plugin in crate::plugins::installed()? {
        let detect = value::get(&plugin.manifest, "detect")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if value::strings(value::get(&detect, "env"))
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
            || value::strings(value::get(&detect, "paths"))
                .iter()
                .any(|path| Path::new(path).exists())
        {
            return Ok(Some("corporate".into()));
        }
    }
    Ok(None)
}
