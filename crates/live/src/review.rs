//! Reviewer selection, CODEOWNERS and review-draft validation.

use crate::{emit, field, review_error, text, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use careerbrew_vault::canon::Canon;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MIN_PEOPLE: usize = 2;
pub const RECENT_PRS: usize = 20;
pub const SHOW_PATHS: usize = 4;
pub const CODEOWNERS: &[&str] = &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
const CHAT_WIDE: &[&str] = &["here", "channel", "everyone", "all"];

pub fn parse_url(url: &str) -> Result<Value> {
    let pattern = Regex::new(
        r"^https?://(?P<host>[^/]+)/(?P<owner>[\w.-]+)/(?P<repo>[\w.-]+)/pull/(?P<number>\d+)",
    )
    .expect("PR URL");
    let Some(found) = pattern.captures(url.trim()) else {
        return Err(review_error(format!(
            "not a pull request link: {} (fix: pass https://<host>/<owner>/<repo>/pull/<number>)",
            url.chars().take(80).collect::<String>()
        )));
    };
    let number = found["number"]
        .parse::<i64>()
        .map_err(|_| review_error("invalid pull request number"))?;
    Ok(
        json!({"host": &found["host"], "owner": &found["owner"], "repo": &found["repo"], "number": number}),
    )
}

fn connector(root: &Path) -> Result<Value> {
    let allowed = careerbrew_setup::plugins::effective_policy()?;
    if field(&allowed, "allowed_connectors")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.iter().any(|row| text(Some(row)) == "github"))
    {
        return Err(review_error(
            "github is not in the plugin policy allowed_connectors",
        ));
    }
    careerbrew_feed::sources::inventory(root)?
        .into_iter()
        .find(|row| text(field(row, "kind")) == "github")
        .ok_or_else(|| review_error("no github connector (fix: careerbrew connect github)"))
}

fn web_host(api: &str) -> String {
    careerbrew_sources::http::parse_url(api)
        .map(|url| url.host.trim_start_matches("api.").to_owned())
        .unwrap_or_default()
}

fn fixtures() -> Result<std::path::PathBuf> {
    std::env::var_os("CAREERBREW_HTTP_FIXTURES")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| {
            review_error("recorded GitHub fixtures are required for the synthetic review adapter")
        })
}

fn open(root: &Path, url: &str) -> Result<(Value, String, Value)> {
    let reference = parse_url(url)?;
    let connector = connector(root)?;
    let api = {
        let value = text(field(&connector, "api"));
        if value.is_empty() {
            "https://api.github.com".into()
        } else {
            value.trim_end_matches('/').to_owned()
        }
    };
    if text(field(&reference, "host")).to_lowercase() != web_host(&api).to_lowercase() {
        return Err(review_error(format!("the link's host is not the github connector's host ({}) (fix: pass a pull request link on {})", web_host(&api), web_host(&api))));
    }
    // Resolve through the existing secret adapter; the value never enters a
    // report, request fixture key or event row.
    let _ = careerbrew_sources::resolve(&text(field(&connector, "token")))?;
    let repo_api = format!(
        "{api}/repos/{}/{}/pulls/{}",
        text(field(&reference, "owner")),
        text(field(&reference, "repo")),
        reference["number"].as_i64().unwrap_or_default()
    );
    let pr = careerbrew_sources::http::get_json(&fixtures()?, &repo_api, None)?;
    Ok((
        reference,
        repo_api
            .rsplit_once("/pulls/")
            .expect("pull endpoint")
            .0
            .to_owned(),
        pr,
    ))
}

fn get(url: &str) -> Result<Option<Value>> {
    match careerbrew_sources::http::get_json(&fixtures()?, url, None) {
        Ok(value) => Ok(Some(value)),
        Err(error) if matches!(error.status, Some(403 | 404)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn recheck_pr_link(url: &str) -> Result<Value> {
    let pattern = Regex::new(r"(?i)^https://(?P<host>[a-z0-9][a-z0-9.-]*)/(?P<owner>[a-z0-9][a-z0-9-]*)/(?P<repo>[a-z0-9_.-]+)/pull/(?P<number>[1-9][0-9]{0,15})$").expect("review recheck URL");
    let Some(found) = pattern.captures(url) else {
        return Err(crate::error(
            "ValueError",
            "--recheck: use one HTTPS pull request link",
        ));
    };
    if &found["repo"] == "." || &found["repo"] == ".." {
        return Err(crate::error(
            "ValueError",
            "--recheck: use one HTTPS pull request link",
        ));
    }
    Ok(
        json!({"host": &found["host"], "owner": &found["owner"], "repo": &found["repo"], "number": &found["number"]}),
    )
}

pub fn recheck_pr(root: &Path, url: &str) -> (String, String) {
    match recheck_pr_inner(root, url) {
        Ok(result) => result,
        Err(_) => (
            "unknown".into(),
            "review recheck: could not verify PR state".into(),
        ),
    }
}

fn recheck_pr_inner(root: &Path, url: &str) -> Result<(String, String)> {
    let reference = recheck_pr_link(url)?;
    let source = connector(root)?;
    let api = {
        let value = text(field(&source, "api"));
        if value.is_empty() {
            "https://api.github.com".into()
        } else {
            value.trim_end_matches('/').to_owned()
        }
    };
    let parsed = careerbrew_sources::http::parse_url(&api)?;
    if parsed.scheme != "https"
        || text(field(&reference, "host")).to_lowercase() != web_host(&api).to_lowercase()
    {
        return Err(review_error("review recheck source mismatch"));
    }
    careerbrew_sources::http::check_host(&api, None)?;
    let token = careerbrew_sources::resolve(&text(field(&source, "token")))?;
    if token.is_empty() {
        return Err(review_error("review recheck authorization missing"));
    }
    let endpoint = format!(
        "{api}/repos/{}/{}/pulls/{}",
        text(field(&reference, "owner")),
        text(field(&reference, "repo")),
        text(field(&reference, "number"))
    );
    let pr = careerbrew_sources::http::get_json(&fixtures()?, &endpoint, None)?;
    let reported = recheck_pr_link(&text(field(&pr, "html_url")))?;
    let same = ["host", "owner", "repo", "number"].iter().all(|key| {
        text(field(&reference, key)).to_lowercase() == text(field(&reported, key)).to_lowercase()
    });
    if pr["number"].as_i64().map(|number| number.to_string())
        != Some(text(field(&reference, "number")))
        || !same
        || !pr["merged"].is_boolean()
    {
        return Err(review_error("review recheck identity mismatch"));
    }
    if text(field(&pr, "state")) == "closed" {
        return Ok(("closed".into(), "review recheck: PR is closed".into()));
    }
    if text(field(&pr, "state")) != "open" || truthy_merge(&pr) {
        return Err(review_error("review recheck state mismatch"));
    }
    let author = text(field(&pr, "user").and_then(|user| field(user, "login"))).to_lowercase();
    let requested = field(&pr, "requested_reviewers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let teams = field(&pr, "requested_teams")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if author.is_empty()
        || requested.iter().any(|person| {
            text(field(person, "login")).is_empty()
                || !matches!(text(field(person, "type")).as_str(), "User" | "Bot")
        })
        || teams
            .iter()
            .any(|team| text(field(team, "slug")).is_empty())
    {
        return Err(review_error("review recheck requests are incomplete"));
    }
    if !teams.is_empty()
        || requested.iter().any(|person| {
            text(field(person, "type")) == "User"
                && text(field(person, "login")).to_lowercase() != author
        })
    {
        return Ok(("closed".into(), "review recheck: reviewer requested".into()));
    }
    let reviews = careerbrew_sources::http::get_json(
        &fixtures()?,
        &format!("{endpoint}/reviews?per_page=100"),
        None,
    )?;
    let reviews = reviews.as_array().cloned().unwrap_or_default();
    if reviews.len() >= 100 {
        return Err(review_error("review recheck reviews are incomplete"));
    }
    let mut submitted = false;
    for review in reviews {
        if matches!(
            text(field(&review, "state")).as_str(),
            "PENDING" | "DISMISSED"
        ) {
            continue;
        }
        let person = field(&review, "user").cloned().unwrap_or_else(|| json!({}));
        let login = text(field(&person, "login")).to_lowercase();
        if !matches!(
            text(field(&review, "state")).as_str(),
            "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED"
        ) || login.is_empty()
            || !matches!(text(field(&person, "type")).as_str(), "User" | "Bot")
            || crate::parse_time(&text(field(&review, "submitted_at"))).is_none()
        {
            return Err(review_error("review recheck review facts are incomplete"));
        }
        submitted |= text(field(&person, "type")) == "User" && login != author;
    }
    if submitted {
        Ok((
            "closed".into(),
            "review recheck: reviewer has responded".into(),
        ))
    } else {
        Ok(("open".into(), "review recheck: still open".into()))
    }
}

fn truthy_merge(pr: &Value) -> bool {
    pr["merged"].as_bool().unwrap_or(false)
}

pub fn is_group(handle: &str) -> bool {
    handle.trim_start_matches('@').contains('/')
}

pub fn required_approvals(repo_api: &str, base: &str) -> Result<Value> {
    let base = careerbrew_sources::connectors::urlencode(base);
    let mut found = Vec::new();
    if let Some(protection) = get(&format!("{repo_api}/branches/{base}/protection"))? {
        let count = field(&protection, "required_pull_request_reviews")
            .and_then(|row| field(row, "required_approving_review_count"))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        found.push((count, "branch protection"));
    }
    if let Some(rules) = get(&format!("{repo_api}/rules/branches/{base}"))? {
        let counts = rules
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| text(field(row, "type")) == "pull_request")
            .map(|row| {
                field(row, "parameters")
                    .and_then(|value| field(value, "required_approving_review_count"))
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        found.push((counts.into_iter().max().unwrap_or_default(), "rulesets"));
    }
    if found.is_empty() {
        return Ok(json!({"count": 0, "source": "unknown"}));
    }
    found.sort();
    let (count, source) = found.pop().expect("approval rule");
    Ok(json!({"count": count, "source": source}))
}

fn pattern(pattern: &str) -> Regex {
    let anchored = pattern.starts_with('/') || pattern.trim_end_matches('/').contains('/');
    let body = pattern.trim_matches('/');
    let mut out = String::new();
    let mut index = 0;
    while index < body.len() {
        let rest = &body[index..];
        if rest.starts_with("**/") {
            out.push_str("(?:.*/)?");
            index += 3;
        } else if rest.starts_with("**") {
            out.push_str(".*");
            index += 2;
        } else {
            let ch = rest.chars().next().expect("pattern char");
            match ch {
                '*' => out.push_str("[^/]*"),
                '?' => out.push_str("[^/]"),
                _ => out.push_str(&regex::escape(&ch.to_string())),
            }
            index += ch.len_utf8();
        }
    }
    let prefix = if anchored { "" } else { "(?:.*/)?" };
    let below = if pattern.ends_with("/*") {
        ""
    } else {
        "(?:/.*)?"
    };
    Regex::new(&format!("^{prefix}{out}{below}$")).expect("CODEOWNERS regex")
}

pub fn parse_codeowners(text_: &str) -> Vec<(Regex, Vec<String>)> {
    text_
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                return None;
            }
            let mut parts = line.split_whitespace();
            let pat = parts.next().expect("codeowners pattern");
            let owners = parts
                .filter(|owner| owner.starts_with('@'))
                .map(|owner| owner.trim_start_matches('@').to_owned())
                .collect::<Vec<_>>();
            Some((pattern(pat), owners))
        })
        .collect()
}

pub fn owners_of(path: &str, rules: &[(Regex, Vec<String>)]) -> Vec<String> {
    let mut hit = Vec::new();
    for (regex, owners) in rules {
        if regex.is_match(path) {
            hit = owners.clone();
        }
    }
    hit
}

fn codeowners(repo_api: &str, base: &str) -> Result<Vec<(Regex, Vec<String>)>> {
    for where_ in CODEOWNERS {
        if let Some(got) = get(&format!(
            "{repo_api}/contents/{where_}?ref={}",
            careerbrew_sources::connectors::urlencode(base)
        ))? {
            if let Some(content) = field(&got, "content").and_then(Value::as_str) {
                let compact = content.split_whitespace().collect::<String>();
                let decoded = STANDARD
                    .decode(compact)
                    .map_err(|_| review_error("CODEOWNERS content is not valid base64"))?;
                return Ok(parse_codeowners(&String::from_utf8_lossy(&decoded)));
            }
        }
    }
    Ok(Vec::new())
}

fn near(left: &str, right: &str) -> bool {
    fn parent(path: &str) -> &str {
        path.rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("")
    }
    left == right || parent(left) == parent(right)
}

fn bot(user: &Value) -> bool {
    text(field(user, "type")) == "Bot" || text(field(user, "login")).ends_with("[bot]")
}

fn recent_reviewers(
    repo_api: &str,
    base: &str,
    number: i64,
    files: &[String],
) -> Result<Vec<(String, Vec<String>)>> {
    let pulls = get(&format!(
        "{repo_api}/pulls?state=closed&base={}&sort=updated&direction=desc&per_page={RECENT_PRS}",
        careerbrew_sources::connectors::urlencode(base)
    ))?
    .and_then(|value| value.as_array().cloned())
    .unwrap_or_default();
    let mut order = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut paths = BTreeMap::<String, BTreeSet<String>>::new();
    for pr in pulls.into_iter().take(RECENT_PRS) {
        if pr["number"].as_i64() == Some(number) {
            continue;
        }
        let number = pr["number"].as_i64().unwrap_or_default();
        let theirs = get(&format!("{repo_api}/pulls/{number}/files?per_page=100"))?
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|row| text(field(row, "filename")))
            .collect::<Vec<_>>();
        let mine = files
            .iter()
            .filter(|file| theirs.iter().any(|other| near(file, other)))
            .cloned()
            .collect::<BTreeSet<_>>();
        if mine.is_empty() {
            continue;
        }
        let author = text(field(&pr, "user").and_then(|user| field(user, "login")));
        let reviews = get(&format!("{repo_api}/pulls/{number}/reviews?per_page=100"))?
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let reviewers = reviews
            .iter()
            .filter(|review| !bot(field(review, "user").unwrap_or(&json!({}))))
            .map(|review| text(field(review, "user").and_then(|user| field(user, "login"))))
            .filter(|login| !login.is_empty() && *login != author)
            .collect::<BTreeSet<_>>();
        for login in reviewers {
            if !counts.contains_key(&login) {
                order.push(login.clone());
            }
            *counts.entry(login.clone()).or_default() += 1;
            paths.entry(login).or_default().extend(mine.iter().cloned());
        }
    }
    order.sort_by_key(|login| std::cmp::Reverse(counts[login]));
    Ok(order
        .into_iter()
        .map(|login| {
            let paths = paths[&login].iter().cloned().collect::<Vec<_>>();
            (login, paths)
        })
        .collect())
}

fn take_person(
    out: &mut Vec<Value>,
    taken: &mut BTreeSet<String>,
    login: &str,
    why: &str,
    paths: &[String],
) {
    if !login.is_empty() && !is_group(login) && taken.insert(login.to_lowercase()) {
        out.push(json!({"login": login, "why": why, "paths": paths}));
    }
}

pub fn pick(
    needed: usize,
    author: &str,
    code_owners: &[(String, Vec<String>)],
    recent: &[(String, Vec<String>)],
    requested: &[String],
    extra: &[String],
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut taken = BTreeSet::from([author.to_lowercase()]);
    for login in extra {
        take_person(
            &mut out,
            &mut taken,
            login.trim_start_matches('@'),
            "you named them",
            &[],
        );
    }
    for (login, paths) in code_owners {
        if out.len() < needed {
            take_person(&mut out, &mut taken, login, "code owner", paths);
        }
    }
    for (login, paths) in recent {
        if out.len() < needed {
            take_person(
                &mut out,
                &mut taken,
                login,
                "reviewed recent changes here",
                paths,
            );
        }
    }
    for login in requested {
        if out.len() < needed {
            take_person(&mut out, &mut taken, login, "already asked on the PR", &[]);
        }
    }
    out
}

fn what(paths: &[String], all_files: &[String]) -> String {
    let all = all_files.iter().collect::<BTreeSet<_>>();
    if paths.is_empty() || paths.iter().collect::<BTreeSet<_>>().is_superset(&all) {
        return "the whole change".into();
    }
    let shown = paths
        .iter()
        .take(SHOW_PATHS)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > SHOW_PATHS {
        format!("{shown} and {} more", paths.len() - SHOW_PATHS)
    } else {
        shown
    }
}

pub fn build(root: &Path, url: &str, extra: &[String]) -> Result<Value> {
    let (reference, repo_api, pr) = open(root, url)?;
    let base = {
        let value = text(field(&pr, "base").and_then(|base| field(base, "ref")));
        if value.is_empty() {
            "main".into()
        } else {
            value
        }
    };
    let author = text(field(&pr, "user").and_then(|user| field(user, "login")));
    let number = reference["number"].as_i64().unwrap_or_default();
    let files = careerbrew_sources::http::get_json(
        &fixtures()?,
        &format!("{repo_api}/pulls/{number}/files?per_page=100"),
        None,
    )?
    .as_array()
    .cloned()
    .unwrap_or_default()
    .iter()
    .map(|row| text(field(row, "filename")))
    .collect::<Vec<_>>();
    let required = required_approvals(&repo_api, &base)?;
    let needed = (required["count"].as_i64().unwrap_or_default() as usize).max(MIN_PEOPLE);
    let rules = codeowners(&repo_api, &base)?;
    let mut owned = Vec::<(String, Vec<String>)>::new();
    let mut groups = BTreeSet::new();
    for file in &files {
        for owner in owners_of(file, &rules) {
            if is_group(&owner) {
                groups.insert(owner);
            } else if let Some((_, paths)) = owned.iter_mut().find(|(login, _)| *login == owner) {
                paths.push(file.clone());
            } else {
                owned.push((owner, vec![file.clone()]));
            }
        }
    }
    owned.sort_by_key(|left| std::cmp::Reverse(left.1.len()));
    let recent = recent_reviewers(&repo_api, &base, number, &files)?;
    let requested = field(&pr, "requested_reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|user| !bot(user))
        .map(|user| text(field(user, "login")))
        .collect::<Vec<_>>();
    groups.extend(
        field(&pr, "requested_teams")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|team| {
                format!(
                    "{}/{}",
                    text(field(&reference, "owner")),
                    text(field(team, "slug"))
                )
            }),
    );
    let people = pick(needed, &author, &owned, &recent, &requested, extra);
    let status = if people.len() >= needed {
        "ready"
    } else {
        "short"
    };
    let why_count = if text(field(&required, "source")) != "unknown" {
        format!(
            "{} required by {}",
            required["count"].as_i64().unwrap_or_default(),
            text(field(&required, "source"))
        )
    } else {
        "the required count could not be read".into()
    };
    let mut lines = vec![
        format!("Review ask: {}/{}#{} {}", text(field(&reference, "owner")), text(field(&reference, "repo")), number, text(field(&pr, "title"))).trim_end().to_owned(),
        format!("This PR needs {needed} approvals on {base} ({why_count}; never fewer than {MIN_PEOPLE}). Please review:"),
    ];
    lines.extend(people.iter().map(|person| {
        format!(
            "- @{} ({}): {}",
            text(field(person, "login")),
            text(field(person, "why")),
            what(
                &person["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|value| text(Some(value)))
                    .collect::<Vec<_>>(),
                &files
            )
        )
    }));
    if !groups.is_empty() {
        lines.push(format!(
            "FYI, not counted toward approvals: {}",
            groups
                .iter()
                .map(|group| format!("@{group}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if status == "short" {
        lines.push(format!(
            "Missing: {} more person(s). Add them with --reviewer <login>.",
            needed - people.len()
        ));
    }
    let text_ = lines.join("\n") + "\n";
    emit(
        root,
        "work.review_ask",
        vec![
            ("status", Canon::Str(status.into())),
            ("needed", Canon::Int(needed.into())),
            ("named", Canon::Int(people.len().into())),
            ("groups", Canon::Int(groups.len().into())),
            ("files", Canon::Int(files.len().into())),
            ("source", Canon::Str(text(field(&required, "source")))),
        ],
    )?;
    Ok(json!({
        "status": status,
        "needed": needed,
        "required": required["count"].clone(),
        "source": required["source"].clone(),
        "people": people,
        "groups": groups.into_iter().collect::<Vec<_>>(),
        "files": files.len(),
        "text": text_,
        "author": author,
    }))
}

pub fn check(root: &Path, text_: &str, needed: usize, author: &str) -> Result<Value> {
    let mention =
        Regex::new(r"@([A-Za-z0-9][A-Za-z0-9-]*(?:/[A-Za-z0-9][\w.-]*)?)").expect("mention");
    let handles = mention
        .captures_iter(text_)
        .filter(|found| {
            let start = found.get(0).expect("whole mention").start();
            start == 0
                || !text_[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '@')
        })
        .map(|found| found[1].to_owned())
        .collect::<BTreeSet<_>>();
    let excluded = CHAT_WIDE
        .iter()
        .copied()
        .chain([author])
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>();
    let people = handles
        .iter()
        .filter(|handle| !is_group(handle) && !excluded.contains(&handle.to_lowercase()))
        .map(|handle| handle.to_lowercase())
        .collect::<BTreeSet<_>>();
    let groups = handles.iter().filter(|handle| is_group(handle)).count();
    let need = needed.max(MIN_PEOPLE);
    let ok = people.len() >= need;
    emit(
        root,
        "work.review_ask.check",
        vec![
            (
                "status",
                Canon::Str(if ok { "ok".into() } else { "refused".into() }),
            ),
            ("needed", Canon::Int(need.into())),
            ("named", Canon::Int(people.len().into())),
            ("groups", Canon::Int(groups.into())),
        ],
    )?;
    Ok(json!({"ok": ok, "needed": need, "people": people.len(), "groups": groups}))
}

pub fn check_pr(root: &Path, url: &str, text_: &str) -> Result<Value> {
    let (_reference, repo_api, pr) = open(root, url)?;
    let base = {
        let value = text(field(&pr, "base").and_then(|base| field(base, "ref")));
        if value.is_empty() {
            "main".into()
        } else {
            value
        }
    };
    let required = required_approvals(&repo_api, &base)?;
    check(
        root,
        text_,
        required["count"].as_i64().unwrap_or_default() as usize,
        &text(field(&pr, "user").and_then(|user| field(user, "login"))),
    )
}
