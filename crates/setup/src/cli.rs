//! Component adapters for the eleven CLI handlers owned by rust-07.

use crate::value;
use crate::{context, machine, oauth, plugins, Result};
use careerbrew_vault::sync;
use serde_json::{json, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

fn example_files(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let path = entry?.path();
        if path.is_dir() {
            example_files(root, &path, out)?;
        } else if path.is_file() {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    Ok(())
}

pub fn cmd_init() -> Result<String> {
    let ctx = context()?;
    let examples = std::env::current_dir()?.join("profile/examples");
    let corporate = machine::role()? == "corporate";
    let mut files = Vec::new();
    example_files(&examples, &examples, &mut files)?;
    files.sort();
    let mut created = Vec::new();
    for relative in files {
        if corporate
            && relative.components().next().is_some_and(|part| {
                machine::PERSONAL_DIRS.contains(&part.as_os_str().to_str().unwrap_or(""))
            })
        {
            continue;
        }
        let target = ctx.root.join(&relative);
        if target.exists() {
            continue;
        }
        let mut text = std::fs::read_to_string(examples.join(&relative))?;
        if corporate
            && relative
                .file_name()
                .is_some_and(|name| name == "profile.yaml")
        {
            text = machine::work_profile(&text);
        }
        ctx.store().write_text(&target, &text)?;
        created.push(relative.to_string_lossy().into_owned());
    }
    let next = if corporate {
        "next: set name and employer with `careerbrew data set profile.yaml --name '<name>' --employer '<employer>'`, then follow docs/corporate-machine.md"
    } else {
        "next: `careerbrew data edit profile.yaml` (also jobs/criteria.yaml, jobs/sources.yaml), then `careerbrew jobs scan`"
    };
    Ok(format!(
        "data root: {}\ncreated {} file(s): {}\n{next}\n",
        ctx.root.display(),
        created.len(),
        if created.is_empty() {
            "none (all present)".into()
        } else {
            created.join(", ")
        }
    ))
}

pub fn cmd_plugin_add(source: &Path, allow_exec: bool) -> Result<String> {
    let entry = plugins::add_source(&source.to_string_lossy(), allow_exec)?;
    Ok(format!(
        "added plugin {} {} ({}), exec_allowed={}\n",
        value::text(value::get(&entry, "name")),
        value::text(value::get(&entry, "version")),
        value::text(value::get(&entry, "origin")),
        if allow_exec { "True" } else { "False" },
    ))
}

pub fn cmd_plugin_refresh() -> Result<String> {
    let names = plugins::refresh()?;
    Ok(format!(
        "refreshed {} plugin(s): {}\n",
        names.len(),
        if names.is_empty() {
            "none installed".into()
        } else {
            names.join(", ")
        }
    ))
}

pub fn cmd_plugin_remove(name: &str, purge_data: bool) -> Result<String> {
    let result = plugins::remove(name, purge_data)?;
    Ok(format!(
        "removed plugin {name}; purged {} record(s)\n",
        value::get(&result, "purged_records")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    ))
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(value) => if *value { "True" } else { "False" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(python_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(key, value)| format!("'{key}': {}", python_repr(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn policy_json(policy: &Value) -> String {
    let order = [
        "allowed_routes",
        "allowed_connectors",
        "allowed_hosts",
        "secret_backends",
        "require_encrypted_backup",
        "backup_targets",
        "export_to_personal",
        "agent_may_read_business",
        "public_metrics",
    ];
    format!(
        "{{{}}}",
        order
            .iter()
            .map(|key| format!(
                "\"{key}\": {}",
                value::python_json(value::get(policy, key).unwrap_or(&Value::Null))
            ))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub fn cmd_plugin_list() -> Result<String> {
    let installed = plugins::installed()?;
    let mut out = String::new();
    if installed.is_empty() {
        out.push_str("no plugins installed\n");
    }
    for plugin in installed {
        let provides = value::get(&plugin.manifest, "provides")
            .and_then(Value::as_object)
            .map(|items| items.keys().cloned().collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        out.push_str(&format!(
            "{:<16} {:<8} provides={} exec={}\n",
            plugin.name,
            value::text(value::get(&plugin.manifest, "version")),
            provides,
            if value::bool_value(value::get(&plugin.entry, "exec_allowed")) {
                "True"
            } else {
                "False"
            },
        ));
    }
    out.push_str(&format!(
        "effective policy: {}\n",
        policy_json(&plugins::effective_policy()?)
    ));
    Ok(out)
}

pub fn cmd_plugin_verify() -> Result<(String, i32)> {
    let mut out = String::new();
    let mut bad = 0;
    for (name, ok) in plugins::verify()? {
        out.push_str(&format!("{}  {name}\n", if ok { "ok" } else { "TAMPERED" }));
        bad += i32::from(!ok);
    }
    Ok((out, i32::from(bad > 0)))
}

pub fn cmd_machine_set(role: &str, label: &str) -> Result<String> {
    let data = machine::set_role(role, label)?;
    Ok(format!(
        "role set: {}\n",
        value::text(value::get(&data, "role"))
    ))
}

pub fn cmd_machine_show() -> Result<String> {
    let info = machine::info()?;
    let role = machine::role()?;
    let mut out = format!(
        "role: {role} label: {}\n",
        value::get(&info, "label")
            .and_then(Value::as_str)
            .unwrap_or("-")
    );
    if let Some(hint) = machine::suggest()? {
        if hint != role {
            out.push_str(&format!(
                "plugin signals suggest this is a {hint} machine: `careerbrew machine set {hint}`\n"
            ));
        }
    }
    Ok(out)
}

pub fn cmd_connectors_list() -> Result<String> {
    let ctx = context()?;
    let document = value::from_canon(&ctx.store().read_yaml(
        &ctx.root.join("work/connectors.yaml"),
        careerbrew_vault::canon::Canon::Map(Vec::new()),
        true,
    )?);
    let policy = plugins::effective_policy()?;
    let allowed_connectors = value::get(&policy, "allowed_connectors");
    let mut out = String::new();
    for connector in value::get(&document, "connectors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = value::text(value::get(connector, "kind"));
        let allowed = match allowed_connectors {
            None | Some(Value::Null) => true,
            Some(Value::Array(items)) => items.contains(&json!(kind.clone())),
            _ => false,
        };
        out.push_str(&format!(
            "{kind:<8} secret={} allowed_by_policy={}\n",
            oauth::secret_backend(&value::text(value::get(connector, "token")))?,
            if allowed { "True" } else { "False" },
        ));
    }
    out.push_str(&format!(
        "policy: connectors={} hosts={} secret_backends={}\n",
        python_repr(value::get(&policy, "allowed_connectors").unwrap_or(&Value::Null)),
        python_repr(value::get(&policy, "allowed_hosts").unwrap_or(&Value::Null)),
        python_repr(value::get(&policy, "secret_backends").unwrap_or(&Value::Null)),
    ));
    Ok(out)
}

pub fn cmd_pair_init() -> Result<String> {
    let ctx = context()?;
    let corporate = machine::role()? == "corporate";
    let key = sync::pair_init(&ctx.pair_file, &ctx.root)
        .map_err(|error| crate::SetupError::new(error.class, error.message))?;
    let mut out = String::new();
    if corporate {
        out.push_str("export level: none. careerbrew never copies corporate data to your personal machine; pairing only receives settings (ladder, target level) from your personal machine.\n");
    }
    if std::io::stdout().is_terminal() {
        out.push_str(&format!("pairing key (store it in your password manager, then `careerbrew pair join <key>` on the other machine):\n{key}\n"));
    } else {
        out.push_str(&format!(
            "pairing key written to {} (not printed: stdout is not a terminal). Copy it to your password manager yourself.\n",
            ctx.pair_file.display()
        ));
    }
    Ok(out)
}

pub fn cmd_machine_split_plugin_evidence(dry_run: bool) -> Result<(String, i32)> {
    let result = machine::split_plugin_evidence(dry_run)?;
    let verb = if dry_run { "would move" } else { "moved" };
    let mut out = String::new();
    let rows = value::get(&result, "plugins")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for row in &rows {
        let plugin = value::text(value::get(row, "plugin"));
        let file = value::text(value::get(row, "file"));
        if let Some(skipped) = value::get(row, "skipped").and_then(Value::as_str) {
            out.push_str(&format!("skip  plugin {plugin} {file}: {skipped}\n"));
        } else {
            out.push_str(&format!(
                "plugin {plugin} {file}: {} evidence entries -> {} shared ({verb} {} source(s), {} connector(s))\n",
                value::get(row, "before").and_then(Value::as_u64).unwrap_or(0),
                value::get(row, "after").and_then(Value::as_u64).unwrap_or(0),
                value::get(row, "sources").and_then(Value::as_u64).unwrap_or(0),
                value::get(row, "connectors").and_then(Value::as_u64).unwrap_or(0),
            ));
        }
    }
    if !rows.iter().any(|row| value::get(row, "skipped").is_none()) {
        out.push_str(
            "nothing to move: no plugin folder here holds per-person sources or connectors\n",
        );
    }
    let mine = value::get(&result, "mine")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let count = |group: &str, index: usize| {
        value::get(&mine, group)
            .and_then(Value::as_array)
            .and_then(|items| items.get(index))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    out.push_str(&format!(
        "your work/sources.yaml: {} -> {} source(s); work/connectors.yaml: {} -> {} connector(s)\n",
        count("sources", 0),
        count("sources", 1),
        count("connectors", 0),
        count("connectors", 1),
    ));
    let total = value::get(&result, "total")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    out.push_str(&format!(
        "evidence sources work collect reads: {} -> {}\n",
        total.first().and_then(Value::as_u64).unwrap_or(0),
        total.get(1).and_then(Value::as_u64).unwrap_or(0),
    ));
    if dry_run {
        out.push_str("dry run: nothing written; rerun without --dry-run to move them\n");
        return Ok((out, 0));
    }
    let left = value::strings(value::get(&result, "left"));
    let ok = left.is_empty();
    out.push_str(&format!(
        "{}  {}",
        if ok { "ok  " } else { "FAIL" },
        machine::PER_USER_CHECK
    ));
    if !ok {
        out.push_str(&format!(
            "  -> still in {}",
            left[..left.len().min(3)].join(", ")
        ));
    }
    out.push('\n');
    if rows.iter().any(|row| value::get(row, "skipped").is_none()) {
        out.push_str("next: commit the change in the plugin's own repo, so a coworker's copy holds shared entries only\n");
    }
    Ok((out, i32::from(!ok)))
}
