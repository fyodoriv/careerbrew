//! Earliest-start stages and live readiness over supplied row snapshots.

use crate::{field, step_error, text, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const ALONE: &str = "declares no scope or uses: runs alone, in its declared place";
pub const LONG: &str = "long-running: starts when its own prerequisites and resources are ready";
pub const SIGNIN: &str = "needs sign-in, and today's sign-in probe failed";

fn inside(a: &str, b: &str) -> bool {
    let a = if a.trim_end_matches('/').is_empty() {
        "/"
    } else {
        a.trim_end_matches('/')
    };
    let b = if b.trim_end_matches('/').is_empty() {
        "/"
    } else {
        b.trim_end_matches('/')
    };
    a == b || a.starts_with(&(b.to_owned() + "/")) || b.starts_with(&(a.to_owned() + "/"))
}

fn strings(step: &Value, key: &str) -> Vec<String> {
    step.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect()
}

fn alone(step: &Value) -> bool {
    strings(step, "scope").is_empty() && strings(step, "uses").is_empty()
}

pub fn clash(step: &Value, other: &Value) -> String {
    for use_ in strings(step, "uses") {
        if strings(other, "uses").contains(&use_) {
            return format!("shares {use_} with {}", text(field(other, "step")));
        }
    }
    for a in strings(step, "scope") {
        for b in strings(other, "scope") {
            if inside(&a, &b) {
                return format!("scope {a} overlaps {}'s {b}", text(field(other, "step")));
            }
        }
    }
    String::new()
}

fn order(group: &[Value], index: &BTreeMap<String, usize>) -> Result<Vec<Value>> {
    let names = group
        .iter()
        .map(|step| text(field(step, "step")))
        .collect::<BTreeSet<_>>();
    let deps = group
        .iter()
        .map(|step| {
            let name = text(field(step, "step"));
            (
                name.clone(),
                strings(step, "after")
                    .into_iter()
                    .filter(|after| names.contains(after))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut waiting = deps
        .iter()
        .map(|(name, deps)| (name.clone(), deps.len()))
        .collect::<BTreeMap<_, _>>();
    let mut ready = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(name, _)| (index[name], name.clone()))
        .collect::<BTreeSet<_>>();
    let by = group
        .iter()
        .map(|step| (text(field(step, "step")), step.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut out = Vec::new();
    while let Some((_, name)) = ready.pop_first() {
        out.push(by[&name].clone());
        for (other, dependencies) in &deps {
            if dependencies.contains(&name) {
                let count = waiting.get_mut(other).expect("waiting step");
                *count -= 1;
                if *count == 0 {
                    ready.insert((index[other], other.clone()));
                }
            }
        }
    }
    if out.len() < group.len() {
        let loop_names = waiting
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(step_error(format!(
            "work steps {loop_names} wait for each other through `after`: drop one `after` entry"
        )));
    }
    Ok(out)
}

fn late_closure(declared: &[Value], late: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = declared
        .iter()
        .map(|step| text(field(step, "step")))
        .filter(|name| late.contains(name))
        .collect::<BTreeSet<_>>();
    loop {
        let mut grew = false;
        for step in declared {
            let name = text(field(step, "step"));
            if !out.contains(&name)
                && strings(step, "after")
                    .iter()
                    .any(|after| out.contains(after))
            {
                out.insert(name);
                grew = true;
            }
        }
        if !grew {
            return out;
        }
    }
}

type DependencyView = (BTreeMap<String, Vec<String>>, BTreeMap<String, Vec<String>>);

fn dependencies(declared: &[Value], late: &BTreeSet<String>) -> Result<DependencyView> {
    let by = declared
        .iter()
        .map(|step| (text(field(step, "step")), step.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut deps = by
        .iter()
        .map(|(name, step)| {
            (
                name.clone(),
                strings(step, "after")
                    .into_iter()
                    .filter(|after| by.contains_key(after))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut why = by
        .iter()
        .map(|(name, step)| {
            (
                name.clone(),
                strings(step, "after")
                    .into_iter()
                    .filter(|after| !by.contains_key(after))
                    .map(|after| format!("after {after}: not in today's plan, ignored"))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let operators = declared
        .iter()
        .filter(|step| {
            field(step, "operator_turn")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .map(|step| text(field(step, "step")))
        .collect::<Vec<_>>();
    let longs = declared
        .iter()
        .filter(|step| {
            field(step, "long_running")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    for step in declared {
        let name = text(field(step, "step"));
        for after in deps[&name].clone() {
            if field(&by[&after], "long_running")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Err(step_error(format!("work step {name} runs after {after}, which is long-running and never ends: drop that `after` entry")));
            }
            if field(step, "operator_turn")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && after != "/careerbrew"
                && !operators.contains(&after)
            {
                return Err(step_error(format!("operatorTurn step {name} waits for background step {after}; remove that after entry or mark its quick dispatcher operatorTurn: true")));
            }
        }
    }
    for (index, step) in longs.iter().enumerate() {
        for other in &longs[..index] {
            if alone(step) || alone(other) || !clash(step, other).is_empty() {
                return Err(step_error(format!("long-running work steps {} and {} share an unknown or named resource; each never ends: declare separate scope/uses", text(field(step, "step")), text(field(other, "step")))));
            }
        }
    }
    fn add(
        deps: &mut BTreeMap<String, Vec<String>>,
        why: &mut BTreeMap<String, Vec<String>>,
        name: &str,
        dependency: &str,
        reason: String,
    ) {
        if name != dependency && !deps[name].contains(&dependency.to_owned()) {
            deps.get_mut(name)
                .expect("dependency")
                .push(dependency.to_owned());
            why.get_mut(name).expect("reason").push(reason);
        }
    }
    let index = declared
        .iter()
        .enumerate()
        .map(|(index, step)| (text(field(step, "step")), index))
        .collect::<BTreeMap<_, _>>();
    let finite = order(
        &declared
            .iter()
            .filter(|step| {
                !field(step, "long_running")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && !field(step, "operator_turn")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .cloned()
            .collect::<Vec<_>>(),
        &index,
    )?;
    let waits = late_closure(declared, late);
    let finite = finite
        .iter()
        .filter(|step| !waits.contains(&text(field(step, "step"))))
        .chain(
            finite
                .iter()
                .filter(|step| waits.contains(&text(field(step, "step")))),
        )
        .cloned()
        .collect::<Vec<_>>();
    for (position, step) in finite.iter().enumerate() {
        let name = text(field(step, "step"));
        for other in &finite[..position] {
            if alone(step) || alone(other) {
                let other_name = text(field(other, "step"));
                let reason = if alone(other) {
                    format!("after {other_name}, which runs alone")
                } else {
                    ALONE.to_owned()
                };
                add(&mut deps, &mut why, &name, &other_name, reason);
            }
        }
    }
    for step in declared {
        let name = text(field(step, "step"));
        if by.contains_key("/careerbrew") && name != "/careerbrew" {
            add(
                &mut deps,
                &mut why,
                &name,
                "/careerbrew",
                "after /careerbrew starts the day".into(),
            );
        }
        if !field(step, "operator_turn")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && name != "/careerbrew"
        {
            for operator in &operators {
                if waits.contains(operator) {
                    continue;
                }
                add(
                    &mut deps,
                    &mut why,
                    &name,
                    operator,
                    format!("after operator-turn dispatcher {operator}"),
                );
            }
        }
        if field(step, "long_running")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            for other in &finite {
                if alone(step) || alone(other) || !clash(step, other).is_empty() {
                    let other_name = text(field(other, "step"));
                    add(&mut deps, &mut why, &name, &other_name, format!("after {other_name}: finite resource work must finish before this long-running step"));
                }
            }
        }
    }
    for step in declared {
        let name = text(field(step, "step"));
        if waits.contains(&name) {
            if field(step, "operator_turn")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                why.get_mut(&name).expect("reason").push(format!(
                    "{SIGNIN}: record this initial-turn dispatcher blocked"
                ));
                continue;
            }
            for other in &finite {
                let other_name = text(field(other, "step"));
                if !waits.contains(&other_name) {
                    add(&mut deps, &mut why, &name, &other_name, SIGNIN.to_owned());
                }
            }
        }
    }
    Ok((deps, why))
}

pub fn build(declared: &[Value], late: &BTreeSet<String>) -> Result<(Vec<Value>, Vec<Value>)> {
    let index = declared
        .iter()
        .enumerate()
        .map(|(index, step)| (text(field(step, "step")), index))
        .collect::<BTreeMap<_, _>>();
    let (deps, mut why) = dependencies(declared, late)?;
    let signin_waits = late_closure(declared, late);
    let ordered_input = declared
        .iter()
        .map(|step| {
            let mut step = step.clone();
            step["after"] = json!(deps[&text(field(&step, "step"))]);
            step
        })
        .collect::<Vec<_>>();
    let ordered = order(&ordered_input, &index)?;
    let mut groups: Vec<Vec<Value>> = Vec::new();
    let mut stage_of = BTreeMap::new();
    for step in ordered {
        let name = text(field(&step, "step"));
        let mut stage = deps[&name]
            .iter()
            .filter_map(|after| stage_of.get(after).copied())
            .max()
            .unwrap_or(0)
            + 1;
        for after in strings(&step, "after") {
            let alone_reason = format!("after {after}, which runs alone");
            if why[&name].contains(&alone_reason) {
                let position = why[&name]
                    .iter()
                    .position(|reason| reason == &alone_reason)
                    .expect("alone reason");
                why.get_mut(&name).expect("reason")[position] =
                    format!("{alone_reason} (stage {})", stage_of[&after]);
            } else {
                why.get_mut(&name)
                    .expect("reason")
                    .push(format!("after {after} (stage {})", stage_of[&after]));
            }
        }
        while stage <= groups.len() {
            let hit = groups[stage - 1]
                .iter()
                .map(|other| clash(&step, other))
                .find(|reason| !reason.is_empty())
                .unwrap_or_default();
            if hit.is_empty() && !alone(&step) && !groups[stage - 1].iter().any(alone) {
                break;
            }
            if !hit.is_empty() {
                why.get_mut(&name)
                    .expect("reason")
                    .push(format!("{hit} (stage {stage})"));
            }
            stage += 1;
        }
        while groups.len() < stage {
            groups.push(Vec::new());
        }
        groups[stage - 1].push(step.clone());
        stage_of.insert(name.clone(), stage);
        if alone(&step) {
            why.get_mut(&name).expect("reason").push(ALONE.into());
        }
        if field(&step, "long_running")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            why.get_mut(&name).expect("reason").push(LONG.into());
        }
    }
    let found = declared
        .iter()
        .map(|step| {
            let name = text(field(step, "step"));
            let mut step = step.clone();
            step["stage"] = json!(stage_of[&name]);
            step["why"] = json!(if why[&name].is_empty() { "no unmet prerequisites".to_owned() } else { why[&name].join("; ") });
            step["ready_when"] = json!({
                "after": deps[&name],
                "scope_free": step.get("scope").cloned().unwrap_or_else(|| json!([])),
                "resources_free": step.get("uses").cloned().unwrap_or_else(|| json!([])),
                "runs_alone": alone(&step),
                "signin_required": field(&step, "operator_turn").and_then(Value::as_bool).unwrap_or(false) && signin_waits.contains(&name),
            });
            step
        })
        .collect::<Vec<_>>();
    let view = groups
        .iter()
        .enumerate()
        .map(|(index, group)| json!({
            "stage": index + 1,
            "steps": group.iter().map(|step| text(field(step, "step"))).collect::<Vec<_>>(),
            "long_running": !group.is_empty() && group.iter().all(|step| field(step, "long_running").and_then(Value::as_bool).unwrap_or(false)),
        }))
        .collect();
    Ok((found, view))
}

pub fn ready(declared: &[Value], rows: &[Value]) -> Vec<String> {
    let states = rows
        .iter()
        .map(|row| (text(field(row, "step")), text(field(row, "status"))))
        .collect::<BTreeMap<_, _>>();
    let running = declared
        .iter()
        .filter(|step| {
            states
                .get(&text(field(step, "step")))
                .is_some_and(|status| status == "running")
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut selected: Vec<Value> = Vec::new();
    for step in declared {
        let name = text(field(step, "step"));
        if states.get(&name).map_or("waiting", String::as_str) != "waiting" {
            continue;
        }
        let ready_when = field(step, "ready_when")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if field(&ready_when, "signin_required")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        if strings(&ready_when, "after").iter().any(|after| {
            !matches!(
                states.get(after).map_or("", String::as_str),
                "done" | "blocked"
            )
        }) {
            continue;
        }
        if running
            .iter()
            .chain(selected.iter())
            .any(|other| alone(step) || alone(other) || !clash(step, other).is_empty())
        {
            continue;
        }
        selected.push(step.clone());
    }
    selected
        .iter()
        .map(|step| text(field(step, "step")))
        .collect()
}
