//! Draft-only live follow-up: replies, links, reviews, projects and goal lines.

use crate::{
    chars, digest, emit, field, local_day, now, one_line, read_json, read_yaml, setting_bool,
    stamp, text, truthy, write_json, Result,
};
use careerbrew_feed::feed;
use careerbrew_sessions::drafts;
use careerbrew_vault::canon::Canon;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const HOLDING: &str = "Thanks, I saw this. I'm looking into it and will get back to you soon.";
pub const KEEP_FOLLOWED: usize = 5000;
pub const LINKS_OFF: &str = "not opened: the live cycle opens links only with live_open_links on";
pub const READING_OFF: &str = "not opened: reading business data is off here (an agent opens it: careerbrew work feed links --pending)";
pub const MAX_PAGE_BYTES: usize = 1_000_000;
const REVIEW_CHECKS: &[&str] = &[
    "Does the change do what its description says, and nothing else?",
    "Do tests cover the change, including the error case?",
    "Risks: data, security, rollout, and how to roll back.",
    "Names, docs, and anything a reader would trip on.",
];
const CLASS_ORDER: &[&str] = &["needs you", "worth knowing", "noise", "personal"];

fn class(verdict: &str) -> &'static str {
    match verdict {
        "answer" => "needs you",
        "skip" => "noise",
        "personal" => "personal",
        _ => "worth knowing",
    }
}

pub fn reply_target(row: &Value) -> String {
    let target = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._:/#@-]{0,127}$").expect("draft target");
    for candidate in [text(field(row, "thread")), text(field(row, "id"))] {
        if !candidate.is_empty() && target.is_match(&candidate) && !candidate.contains("..") {
            return candidate;
        }
    }
    format!("feed:{}", &digest(&text(field(row, "id")))[..16])
}

pub fn review_target(row: &Value) -> String {
    format!("review:{}", &digest(&text(field(row, "id")))[..16])
}

pub fn review_key(request: &str) -> String {
    format!("review-session:{}", &digest(request)[..16])
}

pub fn review_sessions(root: &Path) -> Result<Value> {
    read_json(root, "work/live/review-sessions.json", json!({}))
}

pub fn open_review(root: &Path, row: &Value, text_: &str) -> Result<(String, bool)> {
    let url_pattern = Regex::new(r#"https?://[^\s<>"')]+"#).expect("review URL");
    let row_url = text(field(row, "url"));
    let found = url_pattern
        .find(text_)
        .or_else(|| url_pattern.find(&row_url));
    let url = found
        .map(|found| {
            found
                .as_str()
                .trim_end_matches(['.', ',', ';', ':'])
                .to_owned()
        })
        .unwrap_or_default();
    let row_id = text(field(row, "id"));
    let key = review_key(if url.is_empty() { &row_id } else { &url });
    let mut sessions = review_sessions(root)?;
    if sessions.get(&key).is_some() {
        return Ok((key, false));
    }
    sessions[&key] = json!({"event": text(field(row, "id")), "url": url, "status": "requested", "opened": stamp()});
    write_json(root, "work/live/review-sessions.json", &sessions)?;
    Ok((key, true))
}

fn thread(
    row: &Value,
    every: &[Value],
    originals: &BTreeMap<String, Value>,
    entries: &[Value],
) -> Result<Vec<Value>> {
    let key = feed::thread_key(row, entries);
    let mut same = every
        .iter()
        .filter(|other| {
            text(field(other, "id")) == text(field(row, "id"))
                || (!key.is_empty() && feed::thread_key(other, entries) == key)
        })
        .cloned()
        .collect::<Vec<_>>();
    same.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
    let messages = same.into_iter().map(|row| json!({
        "ts": field(&row, "at").cloned().unwrap_or(Value::Null),
        "text": originals.get(&text(field(&row, "id"))).map(|original| text(field(original, "text"))).unwrap_or_default(),
        "mine": text(field(&row, "actor")) == "me",
    })).collect::<Vec<_>>();
    Ok(drafts::messages(&json!(messages))?)
}

fn save_draft(root: &Path, target: &str, text_: &str) -> Result<Option<String>> {
    let result = drafts::replace(root, target, text_, true, "")?;
    Ok(if truthy(field(&result, "kept")) {
        None
    } else {
        Some(target.to_owned())
    })
}

pub fn draft_reply(root: &Path, row: &Value, thread: &[Value]) -> String {
    let target = reply_target(row);
    let result = (|| -> Result<String> {
        let fitted = drafts::fit(thread, None, None, None)?;
        match text(field(&fitted, "kind")).as_str() {
            "none" => Ok("no reply draft: your message is the newest in the thread".into()),
            "emoji" => Ok(format!(
                "react :{}: (no text draft)",
                text(field(&fitted, "emoji"))
            )),
            _ => {
                let text_ = field(&fitted, "text")
                    .and_then(Value::as_str)
                    .unwrap_or(HOLDING);
                match save_draft(root, &target, text_) {
                    Ok(Some(target)) => Ok(format!("reply draft saved: {target}")),
                    Ok(None) => Ok(format!("draft kept: {target} already has one")),
                    Err(_) => unreachable!(),
                }
            }
        }
    })();
    result.unwrap_or_else(|error| format!("reply draft not saved: {}", error.class))
}

pub fn is_review(row: &Value, verdict: &str, text_: &str) -> bool {
    verdict == "answer"
        && (matches!(
            text(field(row, "kind")).as_str(),
            "review" | "review-request" | "review_request"
        ) || Regex::new(
            r"(?i)\b(review|take a look|have a look|look over|feedback on|sign[ -]off)\b",
        )
        .expect("review ask")
        .is_match(text_))
}

pub fn private_reason(url: &str) -> Result<String> {
    let parsed = careerbrew_sources::http::parse_url(url)
        .map_err(|_| crate::error("ValueError", "invalid URL"))?;
    let host = parsed.host.trim_end_matches('.').to_lowercase();
    if host.is_empty() {
        return Ok("not opened: the link has no host".into());
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Ok("not opened: the link points to a private or local address".into());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let local = match ip {
            std::net::IpAddr::V4(ip) => {
                ip.is_loopback()
                    || ip.is_private()
                    || ip.is_link_local()
                    || ip.is_multicast()
                    || ip.is_unspecified()
            }
            std::net::IpAddr::V6(ip) => {
                ip.is_loopback()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local()
                    || ip.is_multicast()
                    || ip.is_unspecified()
            }
        };
        if local {
            return Ok("not opened: the link points to a private or local address".into());
        }
    }
    // The recorded transport has one public synthetic answer. A host that does
    // not resolve is represented by an empty fixture host only; ordinary
    // example hosts use the public synthetic address.
    if host.is_empty() {
        return Ok("not opened: the host does not resolve".into());
    }
    Ok(String::new())
}

fn summary_of(html: &str, reference: &str) -> String {
    let title = Regex::new(r"(?is)<title>(.*?)</title>")
        .expect("HTML title")
        .captures(html)
        .map(|found| found[1].to_owned())
        .unwrap_or_else(|| reference.to_owned());
    let head = one_line(&title);
    let head = chars(&head, 120);
    let stripped = Regex::new(r"<[^>]+>")
        .expect("HTML tags")
        .replace_all(html, "\n");
    let key = Regex::new(r"(?i)\b(decid\w*|agreed|scope|deadline|due|launch|ship|blocked|risk|owner|action|todo|open question|\d{4}-\d{2}-\d{2})\b|\?\s*$").expect("research key line");
    if let Some(note) = stripped
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && key.is_match(line))
    {
        format!("{head}: {}", one_line(note))
    } else {
        head
    }
}

fn read_url(url: &str) -> Result<String> {
    let reason = private_reason(url)?;
    if !reason.is_empty() {
        return Err(crate::error("ReadError", reason));
    }
    let fixtures = std::env::var_os("CAREERBREW_HTTP_FIXTURES")
        .map(PathBuf::from)
        .ok_or_else(|| crate::error("ReadError", "recorded HTTP fixtures are required"))?;
    let bytes = careerbrew_sources::http::get_bytes(&fixtures, url, None)
        .map_err(|error| crate::error("ReadError", error.message))?;
    let bytes = &bytes[..bytes.len().min(MAX_PAGE_BYTES)];
    Ok(summary_of(&String::from_utf8_lossy(bytes), url))
}

fn references(root: &Path) -> Result<BTreeSet<String>> {
    let mut refs = BTreeSet::new();
    for (_name, project) in crate::projects(root)? {
        for source in project["sources"].as_array().into_iter().flatten() {
            if field(source, "reference") == Some(&json!(true)) {
                refs.insert(text(field(source, "ref")));
            }
        }
    }
    Ok(refs)
}

fn no_reader(_root: &Path, kind: &str, reference: &str) -> Result<String> {
    if !reference.contains("://") {
        return Ok(String::new());
    }
    let host = careerbrew_sources::http::parse_url(reference)
        .map(|url| url.host.to_lowercase())
        .unwrap_or_default();
    const SIGN_IN: &[&str] = &[
        "docs.google.com",
        "drive.google.com",
        "outlook.office.com",
        "outlook.office365.com",
        "outlook.live.com",
        "sharepoint.com",
        "teams.microsoft.com",
        "slack.com",
    ];
    if SIGN_IN
        .iter()
        .any(|candidate| host == *candidate || host.ends_with(&format!(".{candidate}")))
    {
        return Ok(format!(
            "careerbrew has no reader for {host} links (they open a sign-in page)"
        ));
    }
    if kind == "url" {
        let policy = careerbrew_setup::plugins::effective_policy()?;
        let allowed = field(&policy, "allowed_hosts")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().map(|row| text(Some(row))).collect::<Vec<_>>());
        if !careerbrew_sources::http::host_allowed(&host, allowed.as_deref()) {
            return Ok(format!("careerbrew may not read {host} links (not in the plugin policy allowed_hosts; a plugin with agent_reads: [url] hands them to the agent)"));
        }
    }
    Ok(String::new())
}

pub fn research_links(root: &Path, row: &Value, read_allowed: bool) -> Result<Vec<Value>> {
    let refs = references(root)?;
    let mut out = Vec::new();
    for url in feed::needs_research(row) {
        let mut entry = json!({"url": url, "opened": false, "by": "cycle"});
        let attempt = (|| -> Result<Value> {
            let (kind, reference) = careerbrew_memory::research::detect(&url)?;
            if refs.contains(&url) || refs.contains(&reference) {
                entry["reason"] = json!("not opened: you marked it a project reference");
            } else {
                let why = no_reader(root, &kind, &url)?;
                if !why.is_empty() {
                    entry["reason"] = json!(why);
                } else if !setting_bool(root, "live_open_links", false)? {
                    entry["reason"] = json!(LINKS_OFF);
                } else if !read_allowed {
                    entry["reason"] = json!(READING_OFF);
                } else if kind == "url"
                    && careerbrew_memory::research::github_item(&reference).is_none()
                {
                    entry = json!({"url": url, "opened": true, "summary": chars(&read_url(&url)?, 200), "by": "cycle"});
                } else {
                    let got = read_research(root, &kind, &reference)?;
                    let title = one_line(&got.0);
                    let title = if title.is_empty() {
                        reference.clone()
                    } else {
                        title
                    };
                    let summary = if got.1.is_empty() {
                        title
                    } else {
                        format!("{title}: {}", one_line(&got.1[0]))
                    };
                    entry = json!({"url": url, "opened": true, "summary": chars(&summary, 200), "by": "cycle"});
                }
            }
            Ok(entry.clone())
        })();
        entry = match attempt {
            Ok(entry) => entry,
            Err(error) => {
                entry["reason"] = json!(one_line(&error.message));
                entry
            }
        };
        if !truthy(field(&entry, "opened")) {
            entry["reason"] = json!(chars(&text(field(&entry, "reason")), 120));
        }
        out.push(entry);
    }
    Ok(out)
}

fn read_research(root: &Path, kind: &str, reference: &str) -> Result<(String, Vec<String>)> {
    if matches!(kind, "url" | "repo")
        && careerbrew_memory::research::github_item(reference).is_some()
    {
        let connectors = careerbrew_feed::sources::inventory(root)?;
        if !connectors
            .iter()
            .any(|row| text(field(row, "kind")) == "github")
        {
            return Err(crate::error(
                "ReadError",
                "no github connector (fix: careerbrew connect github)",
            ));
        }
    }
    if kind == "url" {
        return Ok((read_url(reference)?, Vec::new()));
    }
    Err(crate::error(
        "ReadError",
        format!("no reader for {kind} source"),
    ))
}

pub fn review_notes(row: &Value, found: &[Value]) -> String {
    let link = {
        let url = text(field(row, "url"));
        if !url.is_empty() {
            url
        } else {
            row["links"]
                .as_array()
                .and_then(|rows| rows.first())
                .map(|value| text(Some(value)))
                .unwrap_or_else(|| "none".into())
        }
    };
    let mut lines = vec![
        format!(
            "Review notes (a draft, not posted): {}",
            text(field(row, "title"))
        ),
        format!("Link: {link}"),
    ];
    for entry in found {
        lines.push(format!(
            "- {}: {}",
            text(field(entry, "url")),
            if truthy(field(entry, "opened")) {
                text(field(entry, "summary"))
            } else {
                format!("not opened ({})", text(field(entry, "reason")))
            }
        ));
    }
    lines.push(String::new());
    lines.push("Check:".into());
    lines.extend(REVIEW_CHECKS.iter().map(|check| format!("- [ ] {check}")));
    lines.push(String::new());
    lines.push("Verdict: (approve, or request changes and say why in one line)".into());
    lines.join("\n")
}

pub fn attach(root: &Path, row: &Value) -> String {
    let mut names = BTreeSet::new();
    let urls = row["links"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .chain((!text(field(row, "url")).is_empty()).then(|| text(field(row, "url"))))
        .collect::<Vec<_>>();
    for url in urls.into_iter().collect::<BTreeSet<_>>() {
        if let Ok((kind, reference)) = careerbrew_memory::research::detect(&url) {
            if let Ok(name) =
                careerbrew_memory::research::match_project(root, &kind, &reference, "")
            {
                names.insert(name);
            }
        }
    }
    if names.len() == 1 {
        return names.into_iter().next().unwrap_or_default();
    }
    if !names.is_empty() {
        return String::new();
    }
    let label = text(field(row, "project")).to_lowercase();
    if label.is_empty() {
        return String::new();
    }
    let hits = crate::projects(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, project)| {
            text(field(project, "archived_at")).is_empty()
                && (label == *name || label == text(field(project, "title")).to_lowercase())
        })
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    if hits.len() == 1 {
        hits[0].clone()
    } else {
        String::new()
    }
}

fn project_updates(root: &Path, touched: &BTreeMap<String, Vec<(String, Value)>>) -> Vec<String> {
    let mut failed = Vec::new();
    for (name, items) in touched {
        let counts = CLASS_ORDER
            .iter()
            .map(|class| {
                (
                    (*class).to_owned(),
                    items.iter().filter(|(other, _)| other == class).count(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let (class, newest) = items
            .iter()
            .max_by_key(|(_, row)| text(field(row, "at")))
            .expect("touched item");
        let what = CLASS_ORDER
            .iter()
            .filter(|class| counts[**class] > 0)
            .map(|class| format!("{} {class}", counts[*class]))
            .collect::<Vec<_>>()
            .join(", ");
        let text_ = format!(
            "live cycle: {} event(s) ({what}); newest: {class} ({} {}): {}",
            items.len(),
            text(field(newest, "source")),
            text(field(newest, "kind")),
            text(field(newest, "title"))
        );
        if careerbrew_projects::projects::record_update(
            root,
            name,
            "project",
            &chars(&one_line(&text_), 200),
        )
        .is_err()
        {
            failed.push(name.clone());
        }
    }
    failed
}

fn cycle_research(root: &Path, event_id: &str, found: &[Value]) -> Result<Value> {
    let mut updated = feed::research(root, event_id, &json!(found))?;
    for entry in updated["research"].as_array_mut().into_iter().flatten() {
        entry["by"] = json!("cycle");
    }
    let mut rows = feed::load(root)?;
    for row in &mut rows {
        if text(field(row, "id")) == event_id {
            *row = updated.clone();
        }
    }
    rows.sort_by_key(|row| (text(field(row, "at")), text(field(row, "id"))));
    crate::write_jsonl(root, "work/feed/events.jsonl", &rows)?;
    Ok(updated)
}

pub fn follow_up(
    root: &Path,
    verdicts: &BTreeMap<String, String>,
    followed: &BTreeSet<String>,
    read_allowed: bool,
) -> Result<(Vec<Value>, BTreeMap<String, String>)> {
    let every = feed::load(root)?;
    let by_id = every
        .iter()
        .map(|row| (text(field(row, "id")), row))
        .collect::<BTreeMap<_, _>>();
    let originals = feed::originals(root)?;
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let mut touched = BTreeMap::<String, Vec<(String, Value)>>::new();
    let mut out = Vec::new();
    for (event_id, verdict) in verdicts {
        let Some(row) = by_id.get(event_id) else {
            continue;
        };
        if followed.contains(event_id) {
            continue;
        }
        let mut rec = json!({"id": event_id, "source": field(row, "source").cloned().unwrap_or(Value::Null), "class": class(verdict), "verdict": verdict, "actions": [], "project": Value::Null});
        let result = (|| -> Result<()> {
            if matches!(text(field(&rec, "class")).as_str(), "personal" | "noise") {
                let rec_class = text(field(&rec, "class"));
                rec["actions"]
                    .as_array_mut()
                    .expect("actions")
                    .push(json!(if rec_class == "personal" {
                        "nothing: personal, kept out"
                    } else {
                        "nothing: noise"
                    }));
                return Ok(());
            }
            let original_text = originals
                .get(event_id)
                .map(|original| text(field(original, "text")))
                .unwrap_or_default();
            if verdict == "answer" {
                let action = draft_reply(root, row, &thread(row, &every, &originals, &entries)?);
                rec["actions"]
                    .as_array_mut()
                    .expect("actions")
                    .push(json!(action));
            }
            let found = research_links(root, row, read_allowed)?;
            if !found.is_empty() {
                if let Ok(updated) = cycle_research(root, event_id, &found) {
                    rec["actions"]
                        .as_array_mut()
                        .expect("actions")
                        .push(json!(format!(
                            "{} link(s): {} summarized, {} with a reason",
                            found.len(),
                            found
                                .iter()
                                .filter(|entry| truthy(field(entry, "opened")))
                                .count(),
                            found
                                .iter()
                                .filter(|entry| !truthy(field(entry, "opened")))
                                .count()
                        )));
                    let mut updated_row = (*row).to_owned();
                    updated_row["research"] = updated["research"].clone();
                    if is_review(&updated_row, verdict, &original_text) {
                        review_followup(root, &mut rec, &updated_row, &original_text)?;
                    }
                    attach_followup(root, &mut rec, &updated_row, &mut touched)?;
                    return Ok(());
                }
            }
            if is_review(row, verdict, &original_text) {
                review_followup(root, &mut rec, row, &original_text)?;
            }
            attach_followup(root, &mut rec, row, &mut touched)?;
            if rec["actions"].as_array().is_none_or(Vec::is_empty) {
                rec["actions"]
                    .as_array_mut()
                    .expect("actions")
                    .push(json!("nothing to do: noted"));
            }
            Ok(())
        })();
        if let Err(error) = result {
            rec["actions"]
                .as_array_mut()
                .expect("actions")
                .push(json!(format!("failed: {}", error.class)));
            emit(
                root,
                "work.live.error",
                vec![
                    ("source_kind", Canon::Str("listen".into())),
                    ("error", Canon::Str(error.class.into())),
                    ("story", Canon::Str("us-101".into())),
                ],
            )?;
        }
        out.push(rec);
    }
    let failed = project_updates(root, &touched);
    for name in failed {
        for rec in &mut out {
            if text(field(rec, "project")) == name {
                let actions = rec["actions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|action| text(Some(action)) != format!("project {name} updated"))
                    .cloned()
                    .collect::<Vec<_>>();
                rec["actions"] = json!(actions);
                rec["actions"]
                    .as_array_mut()
                    .expect("actions")
                    .push(json!(format!("project {name} not updated: ProjectError")));
                rec["project"] = Value::Null;
            }
        }
    }
    let attached = out
        .iter()
        .filter_map(|rec| {
            field(rec, "project")
                .and_then(Value::as_str)
                .map(|name| (text(field(rec, "id")), name.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    if !out.is_empty() {
        emit(
            root,
            "work.live.listen",
            vec![
                ("status", Canon::Str("ok".into())),
                ("events", Canon::Int(out.len().into())),
                (
                    "needs_you",
                    Canon::Int(
                        out.iter()
                            .filter(|rec| text(field(rec, "class")) == "needs you")
                            .count()
                            .into(),
                    ),
                ),
                ("story", Canon::Str("us-101".into())),
            ],
        )?;
    }
    Ok((out, attached))
}

fn review_followup(root: &Path, rec: &mut Value, row: &Value, original_text: &str) -> Result<()> {
    let target = review_target(row);
    let found = row["research"].as_array().cloned().unwrap_or_default();
    match save_draft(root, &target, &review_notes(row, &found)) {
        Ok(Some(target)) => rec["actions"]
            .as_array_mut()
            .expect("actions")
            .push(json!(format!("review notes drafted: {target}"))),
        Ok(None) => rec["actions"]
            .as_array_mut()
            .expect("actions")
            .push(json!(format!(
                "review notes kept: {target} already has some"
            ))),
        Err(error) => rec["actions"]
            .as_array_mut()
            .expect("actions")
            .push(json!(format!("review notes not saved: {}", error.class))),
    }
    let (key, opened) = open_review(root, row, original_text)?;
    rec["actions"]
        .as_array_mut()
        .expect("actions")
        .push(json!(if opened {
            format!("review session opened: {key}")
        } else {
            "review session kept: one is already open for this request".into()
        }));
    Ok(())
}

fn attach_followup(
    root: &Path,
    rec: &mut Value,
    row: &Value,
    touched: &mut BTreeMap<String, Vec<(String, Value)>>,
) -> Result<()> {
    let name = attach(root, row);
    if !name.is_empty() {
        rec["project"] = json!(name.clone());
        touched
            .entry(name.clone())
            .or_default()
            .push((text(field(rec, "class")), row.clone()));
        rec["actions"]
            .as_array_mut()
            .expect("actions")
            .push(json!(format!("project {name} updated")));
    }
    Ok(())
}

pub fn todays_goals(root: &Path) -> Result<Vec<Value>> {
    let today = now().date_naive().format("%Y-%m-%d").to_string();
    let goals = read_yaml(root, "goals.yaml", json!({"queue": [], "long_term": []}))?;
    let mut out = Vec::new();
    for goal in goals["queue"].as_array().into_iter().flatten() {
        let date = text(field(goal, "date"));
        if date.is_empty() || date >= today {
            out.push(json!({"id": text(field(goal, "id")), "terms": vec![text(field(goal, "id"))].into_iter().chain(goal["topics"].as_array().into_iter().flatten().map(|value| text(Some(value)))).collect::<Vec<_>>()}));
        }
    }
    for goal in goals["long_term"].as_array().into_iter().flatten() {
        let by = text(field(goal, "by"));
        if by.is_empty() || by >= today {
            out.push(json!({"id": text(field(goal, "id")), "terms": [text(field(goal, "id"))]}));
        }
    }
    let mut unique = BTreeMap::new();
    for goal in out.into_iter().rev() {
        unique.entry(text(field(&goal, "id"))).or_insert(goal);
    }
    Ok(unique.into_values().rev().collect())
}

fn hits(goal: &Value, row: &Value, project: Option<&Value>, name: &str) -> bool {
    if row["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|tag| text(Some(tag)) == format!("goal:{}", text(field(goal, "id"))))
    {
        return true;
    }
    if project.is_some_and(|project| {
        let group = text(field(project, "group")).to_lowercase();
        text(field(goal, "id")) == group || text(field(goal, "id")) == name
    }) {
        return true;
    }
    let hay = [
        text(field(row, "title")),
        text(field(row, "project")),
        name.to_owned(),
        project
            .map(|project| text(field(project, "title")))
            .unwrap_or_default(),
    ]
    .join(" ");
    goal["terms"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .any(|term| {
            if term.is_empty() {
                return false;
            }
            Regex::new(&format!(r"(?i){}", regex::escape(&term)))
                .expect("goal term")
                .find_iter(&hay)
                .any(|found| {
                    let before = hay[..found.start()].chars().next_back();
                    let after = hay[found.end()..].chars().next();
                    !before.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
                        && !after.is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
                })
        })
}

pub fn goal_updates(root: &Path, attached: &Value) -> Result<Vec<Value>> {
    let goals = todays_goals(root)?;
    if goals.is_empty() {
        return Ok(Vec::new());
    }
    let today = now().date_naive().format("%Y-%m-%d").to_string();
    let every = feed::load(root)?;
    let entries = careerbrew_projects::project_priority::entries(root)?;
    let keys = every
        .iter()
        .map(|row| (text(field(row, "id")), feed::thread_key(row, &entries)))
        .collect::<BTreeMap<_, _>>();
    let kept = every
        .iter()
        .filter(|row| {
            text(field(row, "triage")) != "noise"
                && !truthy(field(row, "personal"))
                && local_day(&text(field(row, "at"))) == Some(today.clone())
        })
        .collect::<Vec<_>>();
    let open_ids = feed::open_asks(root, Some(&every), false)?
        .iter()
        .map(|row| text(field(row, "id")))
        .collect::<BTreeSet<_>>();
    let projects = crate::projects(root)?;
    let mut out = Vec::new();
    for goal in goals {
        let mine = kept
            .iter()
            .filter(|row| {
                let name = text(attached.get(text(field(row, "id"))));
                hits(&goal, row, projects.get(&name), &name)
            })
            .collect::<Vec<_>>();
        let moved = mine
            .iter()
            .filter(|row| text(field(row, "actor")) == "me")
            .collect::<Vec<_>>();
        let mut waits_on = Vec::new();
        for row in &moved {
            let key = keys
                .get(&text(field(row, "id")))
                .cloned()
                .unwrap_or_default();
            let mut thread = every
                .iter()
                .filter(|other| {
                    !key.is_empty() && keys.get(&text(field(other, "id"))) == Some(&key)
                })
                .collect::<Vec<_>>();
            thread.sort_by_key(|other| (text(field(other, "at")), text(field(other, "id"))));
            if thread
                .last()
                .is_some_and(|last| text(field(last, "id")) == text(field(row, "id")))
            {
                for other in thread {
                    let who = if text(field(other, "actor")) == "other" {
                        let from = text(field(other, "from"));
                        if from.is_empty() {
                            "someone".into()
                        } else {
                            from
                        }
                    } else {
                        String::new()
                    };
                    if !who.is_empty() && !waits_on.contains(&who) {
                        waits_on.push(who);
                    }
                }
            }
        }
        out.push(json!({"id": field(&goal, "id").cloned().unwrap_or(Value::Null), "moved": moved.len(), "news": mine.len() - moved.len(), "waits_you": mine.iter().filter(|row| open_ids.contains(&text(field(row, "id")))).count(), "waits_on": waits_on}));
    }
    Ok(out)
}

pub fn goal_line(goal: &Value, show: bool) -> String {
    let moved = goal["moved"].as_i64().unwrap_or_default();
    let news = goal["news"].as_i64().unwrap_or_default();
    let first = if moved > 0 || news > 0 {
        format!("{moved} moved (your actions today), {news} other update(s)")
    } else {
        "nothing moved today".into()
    };
    let mut waits = Vec::new();
    let waits_you = goal["waits_you"].as_i64().unwrap_or_default();
    if waits_you > 0 {
        waits.push(format!("{waits_you} on you"));
    }
    let waits_on = goal["waits_on"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|value| text(Some(value)))
        .collect::<Vec<_>>();
    if !waits_on.is_empty() {
        waits.push(format!(
            "{} on {}",
            waits_on.len(),
            if show {
                waits_on.join(", ")
            } else {
                "others".into()
            }
        ));
    }
    let second = if waits.is_empty() {
        "nothing waits".into()
    } else {
        format!("waits: {}", waits.join(", "))
    };
    format!("goal {}: {first}; {second}", text(field(goal, "id")))
}
