//! Invocation-scoped Agentfile workflow inventory and owned source reads.
//!
//! Agentbrew owns deployment. This adapter only reads the current checkout's
//! declared sources; no inventory or source-body cache survives the call.

use crate::value;
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    Skill,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Skill => "skill",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: Kind,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub command_sources: Vec<PathBuf>,
    pub skill_sources: Vec<PathBuf>,
    pub problems: Vec<String>,
}

fn owned(path: &Path, checkout: &Path) -> Result<PathBuf, String> {
    let owner = checkout
        .canonicalize()
        .map_err(|_| "declared workflow source is unreadable".to_owned())?;
    let selected = if path.is_absolute() {
        path.to_path_buf()
    } else {
        owner.join(path)
    };
    if !selected.starts_with(&owner) {
        return Err("declared workflow source is outside its owning checkout".into());
    }
    let real = crate::resolved(&selected);
    if !real.starts_with(&owner) {
        return Err("declared workflow source is outside its owning checkout".into());
    }
    Ok(real)
}

pub fn read(
    path: &Path,
    checkout: &Path,
    source_directory: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let selected = owned(path, checkout)?;
    if let Some(directory) = source_directory {
        if !selected.starts_with(owned(directory, checkout)?) {
            return Err("requested resource is outside its selected source directory".into());
        }
    }
    std::fs::read(selected).map_err(|_| "declared workflow source is unreadable".into())
}

fn children(folder: &Path, checkout: &Path) -> Result<Vec<PathBuf>, String> {
    let real = owned(folder, checkout)?;
    let mut children = std::fs::read_dir(real)
        .map_err(|_| "declared workflow source is unreadable".to_owned())?
        .filter_map(|entry| entry.ok().map(|item| item.file_name()))
        .collect::<Vec<_>>();
    children.sort();
    Ok(children.into_iter().map(|name| folder.join(name)).collect())
}

fn insert_skill(found: &mut Vec<PathBuf>, path: PathBuf) {
    let name = path.parent().and_then(Path::file_name).unwrap_or_default();
    if !found
        .iter()
        .any(|old| old.parent().and_then(Path::file_name).unwrap_or_default() == name)
    {
        found.push(path);
    }
}

fn skill_files(folder: &Path, checkout: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for base in [folder.to_path_buf(), folder.join("skills")] {
        if owned(&base, checkout)?.is_dir() {
            for child in children(&base, checkout)? {
                let real = owned(&child, checkout)?;
                if real.is_dir() && real.join("SKILL.md").is_file() {
                    insert_skill(&mut found, child.join("SKILL.md"));
                }
            }
        }
    }
    if found.is_empty() {
        let mut queue = VecDeque::from([(folder.to_path_buf(), 0usize)]);
        while let Some((base, depth)) = queue.pop_front() {
            if depth > 6 {
                continue;
            }
            for child in children(&base, checkout)? {
                let name = child.file_name().unwrap_or_default().to_string_lossy();
                if name.starts_with('.')
                    || matches!(name.as_ref(), "node_modules" | "__pycache__")
                    || child.is_symlink()
                {
                    continue;
                }
                let real = owned(&child, checkout)?;
                if !real.is_dir() {
                    continue;
                }
                if child.join("SKILL.md").is_file() {
                    insert_skill(&mut found, child.join("SKILL.md"));
                }
                queue.push_back((child, depth + 1));
            }
        }
    }
    Ok(found)
}

fn selected_paths(data: &Value, field: &str, default: &str) -> Result<Vec<String>, String> {
    let Some(raw) = value::get(data, field) else {
        return Ok(vec![default.to_owned()]);
    };
    let Some(items) = raw.as_array() else {
        return Err(format!("Agentfile {field} must be a list of source paths"));
    };
    if !items.iter().all(Value::is_string) {
        return Err(format!("Agentfile {field} must be a list of source paths"));
    }
    Ok(items
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect())
}

/// Read one invocation's current inventory. Configuration and source problems
/// stay in `problems`, so the caller can fail its own resolution closed.
pub fn repository(checkout: &Path) -> crate::Result<Inventory> {
    let checkout = checkout
        .canonicalize()
        .map_err(|_| crate::SetupError::value("owning checkout is unreadable"))?;
    let config_path = checkout.join("Agentfile.yaml");
    let data = if config_path.is_file() {
        match std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|text| value::yaml_read(&text).ok())
        {
            Some(data @ Value::Object(_)) => data,
            _ => {
                return Ok(Inventory {
                    problems: vec!["owning Agentfile workflow declarations are unreadable".into()],
                    ..Default::default()
                })
            }
        }
    } else {
        Value::Object(Default::default())
    };
    let mut inventory = Inventory::default();
    for (kind, field, default) in [
        (Kind::Command, "commands", "./commands"),
        (Kind::Skill, "skillDirectories", "./skills"),
    ] {
        let selected = match selected_paths(&data, field, default) {
            Ok(selected) => selected,
            Err(problem) => {
                inventory.problems.push(problem);
                continue;
            }
        };
        for raw in selected {
            let expanded = crate::expand_user(
                Path::new(&raw),
                &std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            );
            let path = if expanded.is_absolute() {
                expanded
            } else {
                checkout.join(expanded)
            };
            let result = (|| -> Result<(), String> {
                let real = owned(&path, &checkout)?;
                if !real.exists() && value::get(&data, field).is_none() {
                    return Ok(());
                }
                let files = match kind {
                    Kind::Command => {
                        inventory.command_sources.push(path.clone());
                        if real.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                            vec![path.clone()]
                        } else if real.is_dir() {
                            children(&path, &checkout)?
                                .into_iter()
                                .filter(|child| {
                                    child.extension().is_some_and(|ext| ext == "md")
                                        && child.is_file()
                                })
                                .collect()
                        } else {
                            return Err(
                                "declared workflow source is missing or has the wrong kind".into(),
                            );
                        }
                    }
                    Kind::Skill => {
                        if real.is_dir() {
                            inventory.skill_sources.push(path.clone());
                            skill_files(&path, &checkout)?
                        } else {
                            return Err(
                                "declared workflow source is missing or has the wrong kind".into(),
                            );
                        }
                    }
                };
                for source in files {
                    owned(&source, &checkout)?;
                    if !inventory
                        .entries
                        .iter()
                        .any(|entry| entry.kind == kind && entry.path == source)
                    {
                        inventory.entries.push(Entry { kind, path: source });
                    }
                }
                Ok(())
            })();
            if let Err(problem) = result {
                inventory.problems.push(problem);
            }
        }
    }
    Ok(inventory)
}
