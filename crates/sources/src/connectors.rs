//! Read-only source adapters. Each request goes through the caller's bounded
//! recorded transport; no adapter retains a credential or response body.

use crate::{field, resolve, secret_backend, text, Result, SourceError};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::DateTime;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const JIRA_MAX_PAGES: usize = 50;
pub const PD_MAX_PAGES: usize = 10;
pub const REPLY_PAGES: usize = 3;
pub const GRAPH_API: &str = "https://graph.microsoft.com/v1.0";
pub const PAGERDUTY_API: &str = "https://api.pagerduty.com";
pub type Fetch<'a> = &'a mut dyn FnMut(&str) -> Result<Value>;

pub fn urlencode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn tags_for(title: &str) -> BTreeSet<String> {
    let low = title.to_lowercase();
    let head = low
        .split(':')
        .next()
        .unwrap_or("")
        .split('(')
        .next()
        .unwrap_or("")
        .trim();
    let mut tags = BTreeSet::new();
    let fixed = match head {
        "fix" => &["quality"][..],
        "feat" => &["delivery"][..],
        "perf" => &["quality", "impact"][..],
        "refactor" => &["quality"][..],
        "docs" => &["communication"][..],
        "test" => &["quality"][..],
        "ci" => &["tooling"][..],
        _ => &[][..],
    };
    tags.extend(fixed.iter().map(|tag| (*tag).to_owned()));
    for (word, tag) in [
        ("incident", "ownership"),
        ("mentor", "leadership"),
        ("design", "design"),
        ("rfc", "design"),
        ("migrat", "impact"),
        ("latency", "impact"),
        ("review", "collaboration"),
    ] {
        if low.contains(word) {
            tags.insert(tag.to_owned());
        }
    }
    tags
}

fn item(
    origin: &str,
    kind: &str,
    id: &str,
    title: &str,
    at: &str,
    url: &str,
    tags: &[&str],
) -> Value {
    let mut all = tags_for(title);
    all.extend(tags.iter().map(|tag| (*tag).to_owned()));
    json!({
        "id": format!("{kind}:{id}"),
        "kind": kind,
        "title": title.chars().take(200).collect::<String>(),
        "at": at,
        "url": url,
        "tags": all.into_iter().collect::<Vec<_>>(),
        "origin": origin,
    })
}

fn rows<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    field(value, key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn object<'a>(value: &'a Value, key: &str) -> &'a Value {
    field(value, key).unwrap_or(&Value::Null)
}

pub fn github(cfg: &Value, origin: &str, fetch: Fetch) -> Result<Vec<Value>> {
    let api = if text(field(cfg, "api")).is_empty() {
        "https://api.github.com".to_owned()
    } else {
        text(field(cfg, "api")).trim_end_matches('/').to_owned()
    };
    let _token = resolve(&text(field(cfg, "token")))?;
    let user = text(field(cfg, "user"));
    let query = urlencode(&format!("author:{user} type:pr created:>=2025-10-19"));
    let data = fetch(&format!("{api}/search/issues?q={query}&per_page=100"))?;
    let mut out = Vec::new();
    for pr in rows(&data, "items") {
        let merged = field(object(pr, "pull_request"), "merged_at")
            .cloned()
            .unwrap_or(Value::Null);
        let mut row = item(
            origin,
            "github",
            &text(field(pr, "id")),
            &text(field(pr, "title")),
            &text(field(pr, "created_at")),
            &text(field(pr, "html_url")),
            if merged.is_null() { &[] } else { &["delivery"] },
        );
        let map = row.as_object_mut().expect("item object");
        map.insert(
            "draft".into(),
            Value::Bool(field(pr, "draft") == Some(&Value::Bool(true))),
        );
        map.insert(
            "state".into(),
            field(pr, "state").cloned().unwrap_or(Value::Null),
        );
        map.insert("merged_at".into(), merged);
        out.push(row);
    }
    let found = fetch(&format!(
        "{api}/search/issues?q={}&per_page=50",
        urlencode(&format!("reviewed-by:{user} type:pr updated:>=2026-10-19"))
    ))?;
    let mut count = 0usize;
    for pr in rows(&found, "items") {
        let repository = text(field(pr, "repository_url"));
        let number = text(field(pr, "number"));
        if repository.is_empty() || number.is_empty() {
            continue;
        }
        let reviews = fetch(&format!("{repository}/pulls/{number}/reviews?per_page=100"))?;
        if reviews.as_array().is_some_and(|items| {
            items.iter().any(|review| {
                text(field(object(review, "user"), "login")) == user
                    && text(field(review, "submitted_at")).starts_with("2026-10-19")
            })
        }) {
            count += 1;
        }
    }
    if count > 0 {
        out.push(item(
            origin,
            "github",
            "reviews-2026-10-19",
            &format!("Reviewed {count} pull requests"),
            "2026-10-19T10:00:00+00:00",
            "",
            &["collaboration"],
        ));
    }
    Ok(out)
}

pub fn jira_auth(email: &str, token: &str) -> String {
    if email.is_empty() {
        format!("Bearer {token}")
    } else {
        format!("Basic {}", STANDARD.encode(format!("{email}:{token}")))
    }
}

fn jira_issue(issue: &Value, base: &str, origin: &str) -> Value {
    let fields = object(issue, "fields");
    let key = text(field(issue, "key"));
    let kind = text(field(object(fields, "issuetype"), "name")).to_lowercase();
    let tags: &[&str] = match kind.as_str() {
        "bug" => &["quality"],
        "incident" => &["ownership"],
        "epic" => &["impact"],
        "story" | "task" => &["delivery"],
        _ => &["delivery"],
    };
    item(
        origin,
        "jira",
        &key,
        &format!("{key}: {}", text(field(fields, "summary"))),
        &text(field(fields, "resolutiondate")),
        &format!("{base}/browse/{key}"),
        tags,
    )
}

pub fn jira(cfg: &Value, origin: &str, fetch: Fetch) -> Result<Vec<Value>> {
    let base = text(field(cfg, "base_url"))
        .trim_end_matches('/')
        .to_owned();
    let token = resolve(&text(field(cfg, "token")))?;
    let _auth = jira_auth(&text(field(cfg, "email")), &token);
    if let Err(error) = fetch(&format!("{base}/rest/api/3/myself")) {
        if matches!(error.status, Some(401 | 403)) {
            return Err(SourceError::connector(format!("jira rejected the token ({} on /rest/api/3/myself) (fix: check the email and token; a scoped token needs base_url https://api.atlassian.com/ex/jira/<cloudId>)", error.status.unwrap_or_default())));
        }
        return Err(error);
    }
    let days = field(cfg, "days").and_then(Value::as_i64).unwrap_or(365);
    let jql = if text(field(cfg, "jql")).is_empty() {
        format!("assignee = currentUser() AND statusCategory = Done AND resolved >= -{days}d")
    } else {
        text(field(cfg, "jql"))
    };
    let mut issues = Vec::new();
    let mut cursor = String::new();
    for _ in 0..JIRA_MAX_PAGES {
        let mut url = format!("{base}/rest/api/3/search/jql?jql={}&fields=summary,resolutiondate,issuetype,priority&maxResults=100", urlencode(&jql));
        if !cursor.is_empty() {
            url.push_str(&format!("&nextPageToken={}", urlencode(&cursor)));
        }
        let data = fetch(&url)?;
        issues.extend(rows(&data, "issues").iter().cloned());
        cursor = text(field(&data, "nextPageToken"));
        if field(&data, "isLast") != Some(&Value::Bool(false)) || cursor.is_empty() {
            break;
        }
    }
    Ok(issues
        .iter()
        .map(|issue| jira_issue(issue, &base, origin))
        .collect())
}

pub fn slack(cfg: &Value, origin: &str, fetch: Fetch, personal: bool) -> Result<Vec<Value>> {
    let _token = resolve(&text(field(cfg, "token")))?;
    let query = if text(field(cfg, "query")).is_empty() {
        "thanks OR kudos OR shoutout to:me".to_owned()
    } else {
        text(field(cfg, "query"))
    };
    let data = fetch(&format!(
        "https://slack.com/api/search.messages?query={}&count=100",
        urlencode(&query)
    ))?;
    if field(&data, "ok") != Some(&Value::Bool(true)) {
        return Err(SourceError::connector(format!(
            "slack error: {} (fix: a user token with search:read)",
            text(field(&data, "error"))
        )));
    }
    if personal {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for message in rows(object(&data, "messages"), "matches") {
        let stamp = text(field(message, "ts"));
        let seconds = stamp
            .parse::<f64>()
            .map_err(|_| SourceError::value("invalid Slack timestamp"))?
            as i64;
        let at = DateTime::from_timestamp(seconds, 0)
            .ok_or_else(|| SourceError::value("invalid Slack timestamp"))?
            .format("%Y-%m-%dT%H:%M:%S+00:00")
            .to_string();
        let channel = text(field(object(message, "channel"), "name"));
        out.push(item(
            origin,
            "slack",
            &stamp,
            &format!(
                "Recognition in #{}",
                if channel.is_empty() { "dm" } else { &channel }
            ),
            &at,
            &text(field(message, "permalink")),
            &["collaboration"],
        ));
    }
    Ok(out)
}

pub fn gdocs(cfg: &Value, origin: &str, fetch: Fetch) -> Result<Vec<Value>> {
    let _token = resolve(&text(field(cfg, "token")))?;
    let query = "'me' in owners and mimeType='application/vnd.google-apps.document' and modifiedTime > '2025-10-19T10:00:00'";
    let data = fetch(&format!("https://www.googleapis.com/drive/v3/files?q={}&fields=files(id,name,createdTime,webViewLink)&pageSize=200", urlencode(query)))?;
    let mut out = Vec::new();
    for file in rows(&data, "files") {
        let name = text(field(file, "name"));
        let low = name.to_lowercase();
        let tags: &[&str] = if low.contains("postmortem") || low.contains("incident") {
            &["ownership"]
        } else if ["design", "rfc", "proposal", "adr", "architecture"]
            .iter()
            .any(|word| low.contains(word))
        {
            &["design"]
        } else {
            &["communication"]
        };
        out.push(item(
            origin,
            "gdocs",
            &text(field(file, "id")),
            &name,
            &text(field(file, "createdTime")),
            &text(field(file, "webViewLink")),
            tags,
        ));
    }
    Ok(out)
}

pub fn meeting_tags(topic: &str) -> &'static [&'static str] {
    let low = topic.to_lowercase();
    if ["design review", "architecture", "rfc"]
        .iter()
        .any(|word| low.contains(word))
    {
        &["design"]
    } else if ["1:1", "mentor", "onboard", "interview"]
        .iter()
        .any(|word| low.contains(word))
    {
        &["leadership"]
    } else {
        &["collaboration"]
    }
}

pub fn zoom(cfg: &Value, origin: &str, fetch: Fetch) -> Result<Vec<Value>> {
    let _token = resolve(&text(field(cfg, "token")))?;
    let data = fetch("https://api.zoom.us/v2/users/me/meetings?type=previous_meetings&page_size=300&from=2025-10-19")?;
    Ok(rows(&data, "meetings")
        .iter()
        .map(|meeting| {
            let topic = text(field(meeting, "topic"));
            item(
                origin,
                "zoom",
                &text(field(meeting, "uuid")),
                &format!("Hosted: {topic}"),
                &text(field(meeting, "start_time")),
                "",
                meeting_tags(&topic),
            )
        })
        .collect())
}

pub fn outlook(cfg: &Value, origin: &str, fetch: Fetch) -> Result<Vec<Value>> {
    let _token = resolve(&text(field(cfg, "token")))?;
    let api = if text(field(cfg, "api")).is_empty() {
        GRAPH_API.to_owned()
    } else {
        text(field(cfg, "api")).trim_end_matches('/').to_owned()
    };
    let data = fetch(&format!("{api}/me/calendarView?startDateTime=2025-10-19T10:00:00Z&endDateTime=2026-10-19T10:00:00Z&$select=id,subject,start,isOrganizer,isCancelled,sensitivity&$top=500"))?;
    let mut out = Vec::new();
    for event in rows(&data, "value") {
        if field(event, "isOrganizer") != Some(&Value::Bool(true))
            || field(event, "isCancelled") == Some(&Value::Bool(true))
            || matches!(
                text(field(event, "sensitivity")).as_str(),
                "personal" | "private"
            )
        {
            continue;
        }
        let subject = text(field(event, "subject"));
        let subject = if subject.is_empty() {
            "Meeting"
        } else {
            &subject
        };
        let raw = text(field(object(event, "start"), "dateTime"));
        let at = format!("{}Z", raw.chars().take(19).collect::<String>());
        let id = text(field(event, "id"));
        out.push(item(
            origin,
            "outlook",
            if id.is_empty() { &at } else { &id },
            &format!("Organized: {subject}"),
            &at,
            "",
            meeting_tags(subject),
        ));
    }
    Ok(out)
}

pub fn run(cfg: &Value, origin: &str, fetch: Fetch, personal: bool) -> Result<Vec<Value>> {
    if let Some(reference) = field(cfg, "token") {
        secret_backend(&text(Some(reference)))?;
    }
    match text(field(cfg, "kind")).as_str() {
        "github" => github(cfg, origin, fetch),
        "jira" => jira(cfg, origin, fetch),
        "slack" => slack(cfg, origin, fetch, personal),
        "gdocs" => gdocs(cfg, origin, fetch),
        "zoom" => zoom(cfg, origin, fetch),
        "outlook" => outlook(cfg, origin, fetch),
        kind => Err(SourceError::new(
            "KeyError",
            format!("unknown connector {kind}"),
        )),
    }
}

pub fn pd_actions(fetch: Fetch, me: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for _ in 0..PD_MAX_PAGES {
        let data = fetch(&format!("{PAGERDUTY_API}/users/{me}/log_entries?since=2026-10-19T10:00:00Z&until=2026-10-19T10:00:00Z&time_zone=UTC&is_overview=true&limit=100&offset={offset}"))?;
        let entries = rows(&data, "log_entries");
        for entry in entries {
            if text(field(object(entry, "agent"), "id")) != me {
                continue;
            }
            let (title, status) = match text(field(entry, "type")).as_str() {
                "acknowledge_log_entry" => ("Acknowledged page", "acknowledged"),
                "resolve_log_entry" => ("Resolved incident", "resolved"),
                _ => continue,
            };
            let incident = object(entry, "incident");
            let summary = text(field(incident, "summary"));
            out.push(json!({
                "id": format!("pagerduty:{}", text(field(entry, "id"))),
                "source": "pagerduty",
                "kind": "incident-action",
                "at": text(field(entry, "created_at")),
                "title": format!("{title}: {summary}"),
                "actor": "me",
                "status": status,
                "url": text(field(incident, "html_url")),
                "thread": format!("pagerduty:{}", text(field(incident, "id"))),
                "tags": ["ownership"],
                "text": summary,
                "original": entry,
            }));
        }
        if field(&data, "more") != Some(&Value::Bool(true)) {
            break;
        }
        offset += if entries.is_empty() {
            100
        } else {
            entries.len()
        };
    }
    Ok(out)
}

pub fn token_page(kind: &str, api: &str) -> String {
    if kind == "github" && !api.is_empty() {
        if let Ok(parsed) = crate::http::parse_url(api) {
            if parsed.host != "api.github.com" {
                return format!(
                    "https://{}/settings/personal-access-tokens/new",
                    parsed.host
                );
            }
        }
    }
    match kind {
        "jira" => "https://id.atlassian.com/manage-profile/security/api-tokens".into(),
        "github" => "https://github.com/settings/personal-access-tokens/new".into(),
        "outlook" => "the place your admin names (manual token only, with --paste: use only a token your admin issued or allows)".into(),
        _ => String::new(),
    }
}

fn verify_inner(cfg: &Value, token: &str, fetch: Fetch) -> Result<Value> {
    let kind = text(field(cfg, "kind"));
    match kind.as_str() {
        "jira" => {
            let base = text(field(cfg, "base_url"))
                .trim_end_matches('/')
                .to_owned();
            let _auth = jira_auth(&text(field(cfg, "email")), token);
            match fetch(&format!("{base}/rest/api/3/myself")) {
                Ok(_) => {
                    return Ok(
                        json!({"ok": true, "message": "jira: token works on the site URL", "base_url": base}),
                    )
                }
                Err(error) if !matches!(error.status, Some(401 | 403)) => return Err(error),
                Err(_) => {}
            }
            let cloud = fetch(&format!("{base}/_edge/tenant_info"))
                .ok()
                .and_then(|value| {
                    field(&value, "cloudId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            if let Some(cloud) = cloud {
                let gateway = format!("https://api.atlassian.com/ex/jira/{cloud}");
                if fetch(&format!("{gateway}/rest/api/3/myself")).is_ok() {
                    return Ok(
                        json!({"ok": true, "message": "jira: scoped token works through the API gateway; base_url set to it", "base_url": gateway}),
                    );
                }
            }
            Ok(
                json!({"ok": false, "message": format!("jira rejected the token on /rest/api/3/myself (fix: check the email is your Atlassian login, create a new token at {}, and paste it whole)", token_page("jira", ""))}),
            )
        }
        "github" => {
            let api = if text(field(cfg, "api")).is_empty() {
                "https://api.github.com".to_owned()
            } else {
                text(field(cfg, "api"))
            };
            match fetch(&format!("{api}/user")) {
                Ok(me) => Ok(
                    json!({"ok": true, "message": format!("github: token works for {}", text(field(&me, "login")))}),
                ),
                Err(error) if error.status.is_some() => Ok(
                    json!({"ok": false, "message": format!("github rejected the token ({}) (fix: create a new one at {})", error.status.unwrap_or_default(), token_page("github", ""))}),
                ),
                Err(error) => Err(error),
            }
        }
        "outlook" => match fetch(&format!("{}/me", GRAPH_API)) {
            Ok(_) => {
                Ok(json!({"ok": true, "message": "outlook: token works on Microsoft Graph /me"}))
            }
            Err(error) if error.status.is_some() => Ok(
                json!({"ok": false, "message": format!("outlook rejected the token ({}) (fix: create a new one at the tool)", error.status.unwrap_or_default())}),
            ),
            Err(error) => Err(error),
        },
        _ => Ok(
            json!({"ok": true, "message": format!("{kind}: no check endpoint; the first `careerbrew work collect` tests it")}),
        ),
    }
}

pub fn verify(cfg: &Value, token: &str, fetch: Fetch) -> Result<Value> {
    match verify_inner(cfg, token, fetch) {
        Err(error) if error.class == "ConnectorError" => Ok(json!({
            "ok": false,
            "message": format!("{} (fix: for a host the plugin's owner approved, add it to the plugin's allowed_hosts, then careerbrew plugin refresh)", error.message),
        })),
        answer => answer,
    }
}

pub fn status(cfg: &Value) -> Result<Value> {
    let kind = text(field(cfg, "kind"));
    let needed: &[&str] = match kind.as_str() {
        "jira" => &["base_url", "email"],
        "github" => &["user"],
        _ => &[],
    };
    let missing = needed
        .iter()
        .filter(|key| text(field(cfg, key)).is_empty())
        .map(|key| (*key).to_owned())
        .collect::<Vec<_>>();
    let reference = text(field(cfg, "token"));
    let backend = if reference.is_empty() {
        None
    } else {
        Some(secret_backend(&reference)?)
    };
    let secret_ok = match backend {
        Some("env") => {
            Some(std::env::var(reference.strip_prefix("env:").unwrap_or_default()).is_ok())
        }
        Some("keychain") => Some(false),
        _ => None,
    };
    let mut steps = Vec::new();
    if (secret_ok == Some(false) || reference.is_empty()) && kind == "jira" {
        steps.push("Create a token at https://id.atlassian.com/manage-profile/security/api-tokens (it looks like a long string that often starts with ATATT (Cloud) or a personal access token (Data Center); scope: read:jira-work and read:jira-user (a scoped token works only through https://api.atlassian.com/ex/jira/<cloudId>; careerbrew finds it)).".to_owned());
        steps.push(format!(
            "Run in your terminal: careerbrew connect {kind}{}",
            needed
                .iter()
                .map(|key| format!(" --{} <{key}>", key.replace('_', "-")))
                .collect::<String>()
        ));
    }
    Ok(
        json!({"kind": kind, "origin": if text(field(cfg, "origin")).is_empty() { "user".to_owned() } else { text(field(cfg, "origin")) }, "secret": backend, "secret_ok": secret_ok, "missing": missing, "ready": steps.is_empty(), "steps": steps}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_tags_and_auth_match_the_oracle() {
        let row = item("user", "github", "1", "perf: cut latency", "at", "", &[]);
        assert_eq!(field(&row, "tags"), Some(&json!(["impact", "quality"])));
        assert_eq!(
            jira_auth("me@example.com", "synthetic"),
            "Basic bWVAZXhhbXBsZS5jb206c3ludGhldGlj"
        );
    }
}
