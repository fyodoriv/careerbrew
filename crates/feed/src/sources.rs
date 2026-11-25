//! Recorded source-reader adapters used by the feed component.
//! Every request goes through careerbrew-sources' bounded fixture transport.

use crate::{emit, field, now_utc, read_yaml, text, truthy, ComponentProvider, FeedError, Result};
use careerbrew_sources::{connectors as source_connectors, http};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, TimeDelta, Utc};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixture_root() -> Result<PathBuf> {
    std::env::var_os("CAREERBREW_HTTP_FIXTURES")
        .map(PathBuf::from)
        .ok_or_else(|| FeedError::value("CAREERBREW_HTTP_FIXTURES is required"))
}

fn get(root: &Path, url: &str) -> Result<Value> {
    let allowed = careerbrew_sources::allowed_hosts()?;
    Ok(http::get_json(root, url, allowed.as_deref())?)
}

fn window(cfg: &Value) -> (DateTime<Utc>, DateTime<Utc>) {
    let end = now_utc();
    let days = field(cfg, "feed_days").and_then(Value::as_i64).unwrap_or(2);
    (end - TimeDelta::days(days), end)
}

fn iso(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn object<'a>(value: &'a Value, key: &str) -> &'a Value {
    field(value, key).unwrap_or(&Value::Null)
}

fn rows<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    field(value, key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn slack_item(message: &Value, me: &str, thread: &str) -> Result<Value> {
    let channel = object(message, "channel");
    let value = text(field(message, "text"));
    let user = text(field(message, "user"));
    let actor = if !me.is_empty() && user == me {
        "me"
    } else if !text(field(message, "bot_id")).is_empty()
        || text(field(message, "subtype")) == "bot_message"
        || user.is_empty()
    {
        "bot"
    } else {
        "other"
    };
    let dm = truthy(field(channel, "is_im")) || truthy(field(channel, "is_mpim"));
    let mention = !me.is_empty() && value.contains(&format!("<@{me}>"));
    let name = {
        let name = text(field(message, "username"));
        if name.is_empty() {
            if user.is_empty() {
                "someone".into()
            } else {
                user.clone()
            }
        } else {
            name
        }
    };
    let where_ = if dm {
        "a DM".to_owned()
    } else {
        let name = text(field(channel, "name"));
        format!(
            "#{}",
            if name.is_empty() {
                "channel".to_owned()
            } else {
                name
            }
        )
    };
    let title = if actor == "me" {
        format!("You wrote in {where_}")
    } else if dm {
        format!("DM from {name}")
    } else if mention {
        format!("Mention from {name} in {where_}")
    } else {
        format!("{name} in {where_}")
    };
    let permalink = text(field(message, "permalink"));
    let thread_ts = permalink
        .split_once("thread_ts=")
        .and_then(|(_, tail)| tail.split(['&', '#']).next())
        .unwrap_or_default()
        .to_owned();
    let ts = text(field(message, "ts"));
    let root = if thread_ts.is_empty() {
        ts.clone()
    } else {
        thread_ts
    };
    let cid = {
        let id = text(field(channel, "id"));
        if id.is_empty() {
            text(field(channel, "name"))
        } else {
            id
        }
    };
    let seconds = ts
        .parse::<f64>()
        .map_err(|_| FeedError::value("invalid Slack timestamp"))?;
    if !seconds.is_finite() {
        return Err(FeedError::value("invalid Slack timestamp"));
    }
    let at = DateTime::from_timestamp(seconds as i64, 0)
        .ok_or_else(|| FeedError::value("invalid Slack timestamp"))?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    Ok(json!({
        "id": format!("slack:{cid}:{ts}"),
        "source": "slack",
        "kind": if dm { "dm" } else if mention { "mention" } else { "message" },
        "at": at,
        "title": title,
        "actor": actor,
        "actor_id": user,
        "to_me": (dm || mention) && actor != "me",
        "channel": if dm { "".to_owned() } else { text(field(channel, "name")) },
        "from": name,
        "url": permalink,
        "thread": if !thread.is_empty() { thread.to_owned() } else if dm { format!("slack:{cid}") } else { format!("slack:{cid}:{root}") },
        "text": value,
        "original": message,
    }))
}

fn keep_slack(
    found: &mut Vec<Value>,
    roots: &mut BTreeMap<String, Value>,
    data: &Value,
    me: &str,
) -> Result<()> {
    for message in rows(object(data, "messages"), "matches") {
        if text(field(message, "ts")).is_empty() {
            continue;
        }
        let item = slack_item(message, me, "")?;
        let id = text(field(&item, "id"));
        if let Some(old) = found.iter_mut().find(|old| text(field(old, "id")) == id) {
            *old = item.clone();
        } else {
            found.push(item.clone());
        }
        if field(&item, "thread") == field(&item, "id") {
            roots.insert(id, message.clone());
        }
    }
    Ok(())
}

fn slack_code(data: &Value) -> String {
    let code = text(field(data, "error"));
    let code = if code.is_empty() {
        "unknown".to_owned()
    } else {
        code
    };
    if Regex::new(r"^[a-z_]{1,40}$")
        .expect("Slack code")
        .is_match(&code)
    {
        code
    } else {
        "unknown".into()
    }
}

use regex::Regex;

fn slack_replies(
    root: &Path,
    me: &str,
    roots: &BTreeMap<String, Value>,
    base: &str,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut ordered = roots.iter().collect::<Vec<_>>();
    ordered.sort_by(|(_, left), (_, right)| {
        text(field(right, "ts"))
            .parse::<f64>()
            .unwrap_or_default()
            .total_cmp(&text(field(left, "ts")).parse::<f64>().unwrap_or_default())
    });
    for (parent_id, message) in ordered.into_iter().take(30) {
        let channel = object(message, "channel");
        if truthy(field(channel, "is_im")) || truthy(field(channel, "is_mpim")) {
            continue;
        }
        let cid = text(field(channel, "id"));
        if cid.is_empty() {
            continue;
        }
        let ts = text(field(message, "ts"));
        let mut cursor = String::new();
        for _ in 0..source_connectors::REPLY_PAGES {
            let mut url = format!(
                "https://slack.com/api/conversations.replies?channel={}&ts={}&limit=100",
                source_connectors::urlencode(&cid),
                source_connectors::urlencode(&ts)
            );
            if !cursor.is_empty() {
                url.push_str(&format!(
                    "&cursor={}",
                    source_connectors::urlencode(&cursor)
                ));
            }
            let data = get(root, &url)?;
            if !truthy(field(&data, "ok")) {
                let code = slack_code(&data);
                if [
                    "ratelimited",
                    "invalid_auth",
                    "not_authed",
                    "token_revoked",
                    "account_inactive",
                ]
                .contains(&code.as_str())
                    || (code == "missing_scope" && !truthy(field(channel, "is_private")))
                {
                    return Ok(out);
                }
                break;
            }
            for reply in rows(&data, "messages") {
                let reply_ts = text(field(reply, "ts"));
                if reply_ts.is_empty() || reply_ts == ts {
                    continue;
                }
                let mut reply = reply.as_object().cloned().unwrap_or_default();
                reply.insert("channel".into(), channel.clone());
                reply.insert(
                    "permalink".into(),
                    json!(if base.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "{}/archives/{}/p{}?thread_ts={}&cid={}",
                            base.trim_end_matches('/'),
                            cid,
                            reply_ts.replace('.', ""),
                            ts,
                            cid
                        )
                    }),
                );
                out.push(slack_item(&Value::Object(reply), me, parent_id)?);
            }
            cursor = text(field(object(&data, "response_metadata"), "next_cursor"));
            if !truthy(field(&data, "has_more")) || cursor.is_empty() {
                break;
            }
        }
    }
    Ok(out)
}

pub fn slack(root: &Path, cfg: &Value) -> Result<Vec<Value>> {
    let _token = careerbrew_sources::resolve(&text(field(cfg, "token")))?;
    let who = get(root, "https://slack.com/api/auth.test")?;
    if !truthy(field(&who, "ok")) {
        return Err(FeedError::new(
            "ConnectorError",
            "slack authentication failed",
        ));
    }
    let me = text(field(&who, "user_id"));
    let (start, _) = window(cfg);
    let after = (start - TimeDelta::days(1)).date_naive().format("%Y-%m-%d");
    let mut found = Vec::new();
    let mut roots = BTreeMap::new();
    let queries = field(cfg, "feed_queries")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| text(Some(item)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["to:me".into(), "from:me".into()]);
    for query in queries {
        let data = get(
            root,
            &format!(
                "https://slack.com/api/search.messages?query={}&count=100&sort=timestamp",
                source_connectors::urlencode(&format!("{query} after:{after}"))
            ),
        )?;
        if !truthy(field(&data, "ok")) {
            return Err(FeedError::new("ConnectorError", "slack search failed"));
        }
        keep_slack(&mut found, &mut roots, &data, &me)?;
    }
    let roster = careerbrew_projects::team::load(root)?;
    let ids = roster
        .into_iter()
        .map(|row| text(field(&row, "chat")))
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        let turn = now_utc().timestamp().div_euclid(900);
        let groups = ids.len().div_ceil(5).max(1) as i64;
        let first = (turn.rem_euclid(groups) * 5) as usize;
        for id in ids.iter().skip(first).take(5) {
            for query in [format!("from:<@{id}>"), format!("<@{id}>")] {
                let data = get(
                    root,
                    &format!(
                        "https://slack.com/api/search.messages?query={}&count=100&sort=timestamp",
                        source_connectors::urlencode(&format!("{query} after:{after}"))
                    ),
                )?;
                if !truthy(field(&data, "ok")) {
                    break;
                }
                keep_slack(&mut found, &mut roots, &data, &me)?;
            }
        }
    }
    let base = text(field(&who, "url")).trim_end_matches('/').to_owned();
    for item in slack_replies(root, &me, &roots, &base)? {
        let id = text(field(&item, "id"));
        if let Some(selected) = found.iter_mut().find(|old| text(field(old, "id")) == id) {
            if field(selected, "thread") == field(selected, "id")
                && field(selected, "thread") != field(&item, "thread")
                && !text(field(selected, "url")).contains("thread_ts=")
            {
                selected["thread"] = item["thread"].clone();
            }
        } else {
            found.push(item);
        }
    }
    Ok(found)
}

fn pagerduty_actions(
    root: &Path,
    me: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for _ in 0..source_connectors::PD_MAX_PAGES {
        let data = get(root, &format!("https://api.pagerduty.com/users/{me}/log_entries?since={}&until={}&time_zone=UTC&is_overview=true&limit=100&offset={offset}", iso(start), iso(end)))?;
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
            let summary = {
                let value = text(field(incident, "summary"));
                if value.is_empty() {
                    "an incident".into()
                } else {
                    value
                }
            };
            out.push(json!({
                "id": format!("pagerduty:{}", text(field(entry, "id"))),
                "source": "pagerduty", "kind": "incident-action", "at": text(field(entry, "created_at")),
                "title": format!("{title}: {summary}"), "actor": "me", "status": status,
                "url": text(field(incident, "html_url")), "thread": format!("pagerduty:{}", text(field(incident, "id"))),
                "tags": ["ownership"], "text": summary, "original": entry,
            }));
        }
        if !truthy(field(&data, "more")) {
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

pub fn pagerduty(root: &Path, cfg: &Value) -> Result<Vec<Value>> {
    let _token = careerbrew_sources::resolve(&text(field(cfg, "token")))?;
    let who = get(root, "https://api.pagerduty.com/users/me")?;
    let me = text(field(object(&who, "user"), "id"));
    if me.is_empty() {
        return Err(FeedError::new(
            "ConnectorError",
            "pagerduty: /users/me has no user",
        ));
    }
    let (start, end) = window(cfg);
    let data = get(root, &format!("https://api.pagerduty.com/incidents?user_ids%5B%5D={me}&since={}&until={}&time_zone=UTC&limit=100", iso(start), iso(end)))?;
    let mut out = Vec::new();
    for incident in rows(&data, "incidents") {
        let id = text(field(incident, "id"));
        let title = {
            let title = text(field(incident, "title"));
            let summary = text(field(incident, "summary"));
            if !title.is_empty() {
                title
            } else if !summary.is_empty() {
                summary
            } else {
                "an incident".into()
            }
        };
        out.push(json!({
            "id": format!("pagerduty:{id}"), "source": "pagerduty", "kind": "page",
            "at": text(field(incident, "created_at")), "title": format!("Page: {title}"),
            "actor": "other", "to_me": true, "channel": text(field(object(incident, "service"), "summary")),
            "status": text(field(incident, "status")), "importance": text(field(incident, "urgency")),
            "url": text(field(incident, "html_url")), "thread": format!("pagerduty:{id}"),
            "tags": ["ownership"], "text": text(field(incident, "title")), "original": incident,
        }));
    }
    out.extend(pagerduty_actions(root, &me, start, end)?);
    Ok(out)
}

pub fn outlook(root: &Path, cfg: &Value) -> Result<Vec<Value>> {
    let _token = careerbrew_sources::resolve(&text(field(cfg, "token")))?;
    let me = get(root, "https://graph.microsoft.com/v1.0/me")?;
    let mine = ["mail", "userPrincipalName"]
        .into_iter()
        .map(|key| text(field(&me, key)).to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let (start, end) = window(cfg);
    let calendar = get(root, &format!("https://graph.microsoft.com/v1.0/me/calendarView?startDateTime={}&endDateTime={}&$select=id,subject,start,isOrganizer,isCancelled,sensitivity,organizer,webLink&$top=200", iso(start), iso(end)))?;
    let mut out = Vec::new();
    for event in rows(&calendar, "value") {
        let id = text(field(event, "id"));
        if truthy(field(event, "isCancelled")) || id.is_empty() {
            continue;
        }
        let subject = {
            let value = text(field(event, "subject"));
            if value.is_empty() {
                "Meeting".into()
            } else {
                value
            }
        };
        let organizer = truthy(field(event, "isOrganizer"));
        let at = text(field(object(event, "start"), "dateTime"))
            .chars()
            .take(19)
            .collect::<String>()
            + "Z";
        out.push(json!({
            "id": format!("outlook:{id}"), "source": "outlook", "kind": "meeting", "at": at,
            "title": if organizer { format!("Organized: {subject}") } else { format!("Meeting: {subject}") },
            "actor": if organizer { "me" } else { "other" }, "to_me": !organizer,
            "from": text(field(object(object(event, "organizer"), "emailAddress"), "name")),
            "url": text(field(event, "webLink")), "thread": format!("outlook:{id}"),
            "personal": matches!(text(field(event, "sensitivity")).as_str(), "personal" | "private"),
            "tags": source_connectors::meeting_tags(&subject), "text": subject, "original": event,
        }));
    }
    let mail = get(root, &format!("https://graph.microsoft.com/v1.0/me/messages?$select=id,conversationId,subject,from,toRecipients,receivedDateTime,importance,flag,webLink&$top=100&$orderby=receivedDateTime%20desc&$filter=receivedDateTime%20ge%20{}", iso(start)))?;
    let no_reply = Regex::new(r"^(?:no-?reply|do-?not-?reply|notifications?|alerts?|mailer-daemon|bounces?)(?:[+._-].*)?$").expect("no-reply regex");
    for message in rows(&mail, "value") {
        let sender = object(object(message, "from"), "emailAddress");
        let address = text(field(sender, "address")).to_lowercase();
        let subject = {
            let value = text(field(message, "subject"));
            if value.is_empty() {
                "(no subject)".into()
            } else {
                value
            }
        };
        let actor = if mine.contains(&address) {
            "me"
        } else if no_reply.is_match(address.split('@').next().unwrap_or_default()) {
            "bot"
        } else {
            "other"
        };
        let recipients = rows(message, "toRecipients")
            .iter()
            .map(|row| text(field(object(row, "emailAddress"), "address")).to_lowercase())
            .collect::<Vec<_>>();
        let id = text(field(message, "id"));
        let conversation = {
            let value = text(field(message, "conversationId"));
            if value.is_empty() {
                id.clone()
            } else {
                value
            }
        };
        out.push(json!({
            "id": format!("outlook:mail:{id}"), "source": "outlook", "kind": "mail",
            "at": text(field(message, "receivedDateTime")),
            "title": if actor == "me" { format!("You sent: {subject}") } else { format!("Mail from {}: {subject}", { let name = text(field(sender, "name")); if name.is_empty() { address.clone() } else { name } }) },
            "actor": actor, "to_me": recipients.iter().any(|to| mine.contains(to)) && actor != "me",
            "from": address, "status": if text(field(object(message, "flag"), "flagStatus")) == "flagged" { "flagged" } else { "" },
            "importance": text(field(message, "importance")), "url": text(field(message, "webLink")),
            "thread": format!("outlook:conv:{conversation}"), "text": subject, "original": message,
        }));
    }
    Ok(out)
}

pub fn read(root: &Path, cfg: &Value) -> Result<Vec<Value>> {
    let kind = text(field(cfg, "kind"));
    if std::env::var("CAREERBREW_RUST13_SOURCE_FAILURE").as_deref() == Ok("1") {
        return Err(FeedError::new(
            "RuntimeError",
            format!("CANARYSOURCE {}", "x".repeat(250)),
        ));
    }
    let policy = careerbrew_setup::plugins::effective_policy()?;
    if let Some(allowed) = field(&policy, "allowed_connectors").and_then(Value::as_array) {
        if !allowed.iter().any(|allowed| text(Some(allowed)) == kind) {
            return Err(FeedError::new(
                "ConnectorError",
                format!("connector {kind} is not allowed by plugin policy"),
            ));
        }
    }
    let items = match kind.as_str() {
        "slack" => slack(root, cfg),
        "pagerduty" => pagerduty(root, cfg),
        "outlook" => outlook(root, cfg),
        _ => Err(FeedError::new(
            "KeyError",
            format!("unknown connector {kind}"),
        )),
    }?;
    emit(
        root,
        "connector.feed",
        vec![
            ("source_kind", Canon::Str(kind)),
            ("status", Canon::Str("ok".into())),
            ("items", Canon::Int(items.len().into())),
        ],
    )?;
    Ok(items)
}

fn sensitive_terms(root: &Path) -> Result<Vec<String>> {
    let mut terms = Vec::new();
    let context = crate::context()?;
    let stored = context.store();
    if let Some(text) = stored.read_text(&root.join("sensitive-terms.txt"), None, true)? {
        terms.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty() && !line.starts_with('#') && line.chars().count() >= 3
                })
                .map(str::to_owned),
        );
    }
    let profile = read_yaml(root, "profile.yaml", json!({}))?;
    for key in ["name", "employer", "email"] {
        let value = text(field(&profile, key));
        if value.chars().count() >= 3 && !["Your Name", "Your Employer"].contains(&value.as_str()) {
            terms.push(value);
        }
    }
    terms.extend(
        field(&profile, "sensitive")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|value| text(Some(value)))
            .filter(|value| value.chars().count() >= 3),
    );
    for plugin in careerbrew_setup::plugins::installed()? {
        let path = text(field(&plugin.entry, "sensitive_terms_path"));
        if !path.is_empty() {
            if let Some(text) = stored.read_text(Path::new(&path), None, true)? {
                terms.extend(
                    text.lines()
                        .map(str::trim)
                        .filter(|line| {
                            !line.is_empty() && !line.starts_with('#') && line.chars().count() >= 3
                        })
                        .map(str::to_owned),
                );
            }
        }
    }
    let mut terms = terms
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));
    Ok(terms)
}

fn redacted_reason(root: &Path, error: &FeedError) -> Result<String> {
    let terms = sensitive_terms(root)?;
    Ok(careerbrew_policy::redact(&error.message, &terms)
        .chars()
        .take(200)
        .collect())
}

pub fn pull(root: &Path, components: &dyn ComponentProvider) -> Result<Value> {
    let config = read_yaml(root, "work/connectors.yaml", json!({}))?;
    let fixtures = fixture_root()?;
    let mut stats = json!({"sources": 0, "new": 0, "updated": 0, "errors": 0, "reasons": []});
    for cfg in rows(&config, "connectors") {
        let kind = text(field(cfg, "kind"));
        if !["slack", "pagerduty", "outlook"].contains(&kind.as_str()) {
            continue;
        }
        stats["sources"] = json!(stats["sources"].as_i64().unwrap_or_default() + 1);
        match read(&fixtures, cfg).and_then(|items| {
            crate::feed::ingest(
                root,
                &items,
                &text(field(cfg, "origin")).pipe_empty("user"),
                &kind,
                std::slice::from_ref(&kind),
                components,
            )
        }) {
            Ok(result) => {
                stats["new"] = json!(
                    stats["new"].as_i64().unwrap_or_default()
                        + result["new"].as_i64().unwrap_or_default()
                );
                stats["updated"] = json!(
                    stats["updated"].as_i64().unwrap_or_default()
                        + result["updated"].as_i64().unwrap_or_default()
                );
            }
            Err(error) => {
                crate::feed::note_failure(root, &kind)?;
                stats["errors"] = json!(stats["errors"].as_i64().unwrap_or_default() + 1);
                let reason = redacted_reason(root, &error)?;
                stats["reasons"]
                    .as_array_mut()
                    .expect("reason list")
                    .push(json!(format!("{kind}: {reason}")));
                emit(
                    root,
                    "work.feed.error",
                    vec![
                        ("source_kind", Canon::Str(kind)),
                        ("error", Canon::Str(error.class.into())),
                        ("story", Canon::Str("us-61".into())),
                    ],
                )?;
            }
        }
    }
    emit(
        root,
        "work.feed.pull",
        vec![
            ("status", Canon::Str("ok".into())),
            (
                "sources",
                Canon::Int(stats["sources"].as_i64().unwrap_or_default().into()),
            ),
            (
                "new",
                Canon::Int(stats["new"].as_i64().unwrap_or_default().into()),
            ),
            (
                "errors",
                Canon::Int(stats["errors"].as_i64().unwrap_or_default().into()),
            ),
        ],
    )?;
    Ok(stats)
}

trait EmptyDefault {
    fn pipe_empty(self, default: &str) -> String;
}

impl EmptyDefault for String {
    fn pipe_empty(self, default: &str) -> String {
        if self.is_empty() {
            default.into()
        } else {
            self
        }
    }
}
