//! PM update edits that teach the next draft, and sent-message receipts
//! (Python `day/pm_learn.py`).
//!
//! Lifecycle: files under the data root only. The sent-day lock
//! (`day/.pm-sent-<day>.lock`) covers a day's sent text, its exact app-send
//! receipt and the derived promises; the history lock
//! (`day/.pm-edits.jsonl.lock`) is taken after it, never before. No second
//! handle on the sent-day lock is opened while it is held.

use crate::difflib;
use crate::promises::PromiseRecorder;
use crate::{
    casefold, field, iso_day, local_day, read_json_optional, splitlines, store_read_text,
    store_write_text_if_changed, strip, text, truthy, unlink, value_error, Result,
};
use careerbrew_vault::canon::Canon;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

pub const KEEP: usize = 14;
pub const MIN_TIMES: i64 = 2;
pub const SAME_TEXT: f64 = 0.6;
pub const SENT_LIKE: f64 = 0.3;
pub const HISTORY: &str = "day/pm-edits.jsonl";
pub const RESETS: &str = "day/pm-resets.json";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static pattern")
}

static PART: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|;\s*)(next|overdue):"));
static BOLD: LazyLock<Regex> = LazyLock::new(|| re(r"[*_`]+"));
static HINTS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "announcement",
            re(
                r"(?i)\b(?:launch(?:ed|ing)?|announc\w*|now live|is live|went live|released|rolled out)\b",
            ),
        ),
        (
            "meeting",
            re(
                r"(?i)\b(?:led|hosted|ran|facilitated|meeting|sync|planning|review|1:1|workshop|kickoff)\b",
            ),
        ),
        (
            "own post",
            re(r"(?i)\b(?:checked in|check-in|posted|shared|wrote|thread|update[ds]? (?:in|on))\b"),
        ),
    ]
});
static RECEIPT_ID: LazyLock<Regex> = LazyLock::new(|| re(r"^slack:[CDG][A-Z0-9]+:\d+\.\d+$"));
static CHANNEL: LazyLock<Regex> = LazyLock::new(|| re(r"^[CDG][A-Z0-9]+$"));
static TS: LazyLock<Regex> = LazyLock::new(|| re(r"^\d+\.\d+$"));
static HEX32: LazyLock<Regex> = LazyLock::new(|| re(r"^[0-9a-f]{32}$"));
static URL: LazyLock<Regex> = LazyLock::new(|| re(r"https?://\S+"));
static NO_ACTION: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"(?i)^(?:(?:(?:status(?: check)?|checked (?:the )?(?:tickets|issues|queue|asks|tracker))\s*[:;,–-]\s*)?",
        r"(?:(?:all\s+)?(?:tickets|issues|asks)\s+(?:are\s+)?(?:closed|resolved|handled)[,;:–-]?\s*)?",
        r"(?:no action (?:is )?(?:needed|required)|nothing to do)(?: from (?:the )?(?:owner|user|customer|team)| from you)?[.!]?)$",
    ))
});
static COMMIT_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)^(?:(?:feat|fix|docs|chore|refactor|test|perf|build|ci|style|revert)(?:\([^)]*\))?!?)$",
    )
});
static WORDISH: LazyLock<Regex> = LazyLock::new(|| re(r"^[\w.-]+$"));
static IN_DRAFT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bIn draft\b"));
static UNKNOWN_LINE: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:[#>|<]|\[|`{3}|~{3}|\d+[.)]\s)"));

pub fn shown_relative(day: &str) -> String {
    format!("day/pm-shown-{day}.json")
}
pub fn sent_relative(day: &str) -> String {
    format!("day/pm-sent-{day}.md")
}
pub fn receipt_relative(day: &str) -> String {
    format!("day/pm-sent-receipt-{day}.json")
}
pub fn outcome_relative(day: &str) -> String {
    format!("day/pm-sent-outcome-{day}.json")
}

/// The exact app-send receipt id, or "" (missing, malformed or invalid).
pub fn receipt_id(root: &Path, day: &str) -> Result<String> {
    let relative = receipt_relative(day);
    if !root.join(&relative).exists() {
        return Ok(String::new());
    }
    let receipt = read_json_optional(root, &relative).ok().flatten();
    if let Some(Value::Object(map)) = &receipt {
        if map.len() == 1 {
            if let Some(Value::String(identity)) = map.get("id") {
                if RECEIPT_ID.is_match(identity) {
                    return Ok(identity.clone());
                }
            }
        }
    }
    careerbrew_feed::emit(
        root,
        "day.source.error",
        vec![
            ("source_kind", Canon::Str("pm-receipt".into())),
            ("error", Canon::Str("InvalidReceipt".into())),
            ("story", Canon::Str("us-59".into())),
        ],
    )?;
    Ok(String::new())
}

pub fn hint(line: &str) -> &'static str {
    HINTS
        .iter()
        .find(|(_, pattern)| pattern.is_match(line))
        .map(|(name, _)| *name)
        .unwrap_or("other")
}

/// Run `f` under the canonical sent-day lock.
pub fn with_sent_lock<T>(root: &Path, day: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let canonical = iso_day(day)?.format("%Y-%m-%d").to_string();
    let lock = root.join(format!("day/.pm-sent-{canonical}.lock"));
    careerbrew_vault::lock::with_exact_lock(&lock, f)?
}

pub struct Receipt<'a> {
    pub channel: &'a str,
    pub ts: &'a str,
}

pub fn save_sent(
    root: &Path,
    day: &str,
    value: &str,
    receipt: Option<&Receipt>,
    promises: &dyn PromiseRecorder,
) -> Result<()> {
    with_sent_lock(root, day, || {
        save_sent_locked(root, day, value, receipt, promises)
    })
}

/// The caller holds the sent-day lock.
pub fn save_sent_locked(
    root: &Path,
    day: &str,
    value: &str,
    receipt: Option<&Receipt>,
    promises: &dyn PromiseRecorder,
) -> Result<()> {
    let valid = !strip(value).is_empty()
        && receipt.is_some_and(|r| CHANNEL.is_match(r.channel) && TS.is_match(r.ts));
    let new_identity = match (valid, receipt) {
        (true, Some(r)) => format!("slack:{}:{}", r.channel, r.ts),
        _ => String::new(),
    };
    let old = store_read_text(root, &sent_relative(day))?.unwrap_or_default();
    if strip(&old) != strip(value) || receipt_id(root, day)? != new_identity {
        unlink(root, &outcome_relative(day))?;
    }
    if !valid {
        promises.record(root, day, value, "")?;
    }
    if !strip(value).is_empty() {
        store_write_text_if_changed(root, &sent_relative(day), &format!("{}\n", strip(value)))?;
    } else {
        unlink(root, &sent_relative(day))?;
    }
    if valid {
        careerbrew_feed::write_json_if_changed(
            root,
            &receipt_relative(day),
            &json!({"id": new_identity}),
        )?;
        let revision = record_outcome_revision(root, day, &new_identity, value)?;
        promises.record(root, day, value, &revision)?;
    } else {
        unlink(root, &receipt_relative(day))?;
        unlink(root, &outcome_relative(day))?;
    }
    Ok(())
}

fn record_outcome_revision(root: &Path, day: &str, identity: &str, value: &str) -> Result<String> {
    let digest = careerbrew_vault::helpers::digest(&[
        Canon::Str("pm-sent-binding-v2".into()),
        Canon::Str(day.into()),
        Canon::Str(identity.into()),
        Canon::Str(strip(value).into()),
    ])?;
    let old = read_json_optional(root, &outcome_relative(day))
        .ok()
        .flatten();
    let kept = old
        .as_ref()
        .filter(|old| text(field(old, "digest")) == digest)
        .and_then(|old| field(old, "revision"))
        .and_then(Value::as_str)
        .filter(|revision| HEX32.is_match(revision))
        .map(str::to_owned);
    let revision = match kept {
        Some(revision) => revision,
        None => crate::token_hex16()?,
    };
    careerbrew_feed::write_json_if_changed(
        root,
        &outcome_relative(day),
        &json!({"digest": digest, "revision": revision}),
    )?;
    Ok(revision)
}

fn settings_text(root: &Path, key: &str) -> Result<String> {
    let settings = careerbrew_feed::read_yaml(root, "settings.yaml", json!({}))?;
    Ok(text(field(&settings, key)))
}

pub fn pm_sent_text(root: &Path, day: &str) -> Result<Option<String>> {
    with_sent_lock(root, day, || pm_sent_text_locked(root, day))
}

/// Read the source and receipt pair while the caller holds the sent-day lock.
pub fn pm_sent_text_locked(root: &Path, day: &str) -> Result<Option<String>> {
    let saved = store_read_text(root, &sent_relative(day))?.unwrap_or_default();
    let identity = if saved.is_empty() {
        String::new()
    } else {
        receipt_id(root, day)?
    };
    if !saved.is_empty() && identity.is_empty() {
        let value = strip(&saved);
        return Ok((!value.is_empty()).then(|| value.to_owned()));
    }
    let target = strip(&settings_text(root, "pm_slack")?).to_owned();
    if target.is_empty() && identity.is_empty() {
        return Ok(None);
    }
    let seen = careerbrew_feed::read_json(root, &shown_relative(day), json!({}))?;
    let shown = {
        let value = text(field(&seen, "shown"));
        if value.is_empty() {
            text(field(&seen, "raw"))
        } else {
            value
        }
    };
    if shown.is_empty() && identity.is_empty() {
        return Ok(None);
    }
    let originals = careerbrew_feed::feed::originals(root)?;
    let mut ids = BTreeSet::new();
    for event in careerbrew_feed::feed::load(root)? {
        if text(field(&event, "source")) != "slack"
            || text(field(&event, "actor")) != "me"
            || truthy(field(&event, "personal"))
        {
            continue;
        }
        let id = text(field(&event, "id"));
        if !identity.is_empty() {
            if id != identity {
                continue;
            }
        } else if let Some(channel) = target.strip_prefix('#') {
            if casefold(&text(field(&event, "channel"))) != casefold(channel) {
                continue;
            }
        } else {
            let raw = originals.get(&id).and_then(|row| field(row, "original"));
            let channel = raw.and_then(|raw| field(raw, "channel"));
            let Some(channel @ Value::Object(_)) = channel else {
                continue;
            };
            let is_im = field(channel, "is_im") == Some(&Value::Bool(true));
            let not_mpim = matches!(field(channel, "is_mpim"), None | Some(Value::Bool(false)));
            if !is_im || !not_mpim || field(channel, "name") != Some(&Value::String(target.clone()))
            {
                continue;
            }
        }
        let at = text(field(&event, "at"));
        if local_day(&at).map(|d| d.format("%Y-%m-%d").to_string()) == Some(day.to_owned()) {
            ids.insert(id);
        }
    }
    let texts = originals
        .iter()
        .filter(|(id, _)| ids.contains(*id))
        .map(|(_, row)| strip(&text(field(row, "text"))).to_owned())
        .collect::<Vec<_>>();
    if !identity.is_empty() {
        if let Some(found) = texts.iter().find(|value| !value.is_empty()) {
            return Ok(Some(found.clone()));
        }
        let value = strip(&saved);
        return Ok((!value.is_empty()).then(|| value.to_owned()));
    }
    let mut best = (0.0f64, String::new());
    for value in texts.into_iter().filter(|value| !value.is_empty()) {
        let score = difflib::ratio(&value, &shown);
        if score > best.0 || (score == best.0 && value > best.1) {
            best = (score, value);
        }
    }
    Ok((best.0 >= SENT_LIKE).then_some(best.1))
}

pub fn learn_sent(root: &Path, day: &str, promises: &dyn PromiseRecorder) -> Result<bool> {
    with_sent_lock(root, day, || learn_sent_locked(root, day, promises))
}

pub fn learn_sent_locked(root: &Path, day: &str, promises: &dyn PromiseRecorder) -> Result<bool> {
    let Some(value) = pm_sent_text_locked(root, day)? else {
        return Ok(false);
    };
    let identity = receipt_id(root, day)?;
    let revision = if !identity.is_empty() && root.join(sent_relative(day)).exists() {
        let saved = store_read_text(root, &sent_relative(day))?.unwrap_or_default();
        if strip(&saved) != strip(&value) {
            unlink(root, &outcome_relative(day))?;
        }
        store_write_text_if_changed(root, &sent_relative(day), &format!("{}\n", strip(&value)))?;
        record_outcome_revision(root, day, &identity, &value)?
    } else {
        String::new()
    };
    promises.record(root, day, &value, &revision)?;
    let history = careerbrew_feed::read_jsonl(root, HISTORY)?;
    if history.iter().any(|row| {
        text(field(row, "day")) == day
            && text(field(row, "from")) == "sent"
            && field(row, "body") == Some(&Value::String(value.clone()))
    }) {
        return Ok(false);
    }
    record(root, day, &value, "sent")?;
    Ok(true)
}

pub fn norm(label: &str) -> String {
    casefold(strip(&BOLD.replace_all(label, "")))
}

pub fn split(line: &str) -> (String, String) {
    let line = strip(strip(line).trim_start_matches(['-', '•', '◦', ' ']));
    match line.split_once(':') {
        Some((label, rest)) => (
            strip(&BOLD.replace_all(label, "")).to_owned(),
            strip(rest).to_owned(),
        ),
        None => (String::new(), line.to_owned()),
    }
}

fn lines(value: &str) -> Vec<(String, String)> {
    splitlines(value)
        .iter()
        .filter(|raw| !strip(raw).is_empty())
        .map(|raw| split(raw))
        .collect()
}

fn parts(body: &str) -> BTreeSet<String> {
    PART.captures_iter(body)
        .map(|captures| captures[1].to_owned())
        .collect()
}

fn find(label: &str, value: &str, candidates: &[(String, String)]) -> Option<(String, String)> {
    let key = norm(label);
    if let Some(found) = candidates.iter().find(|(lab, _)| norm(lab) == key) {
        return Some(found.clone());
    }
    let mut best: Option<(&(String, String), f64)> = None;
    for candidate in candidates {
        let score = difflib::ratio(&candidate.1, value);
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((candidate, score));
        }
    }
    best.filter(|(_, score)| *score >= SAME_TEXT)
        .map(|(found, _)| found.clone())
}

fn added(label: &str, value: &str, candidates: &[(String, String)]) -> bool {
    if !label.is_empty() && candidates.iter().any(|(lab, _)| norm(lab) == norm(label)) {
        return false;
    }
    let mine = format!("{label} {value}");
    !candidates
        .iter()
        .any(|(lab, body)| difflib::ratio(&format!("{lab} {body}"), &mine) >= SAME_TEXT)
}

pub fn remember(root: &Path, day: &str, raw: &str, shown: &str) -> Result<()> {
    careerbrew_feed::write_json_if_changed(
        root,
        &shown_relative(day),
        &json!({"raw": raw, "shown": shown}),
    )?;
    Ok(())
}

/// Keep one day's three texts. A web save never replaces a sent text.
pub fn record(root: &Path, day: &str, body: &str, source: &str) -> Result<()> {
    let seen = careerbrew_feed::read_json(root, &shown_relative(day), json!({}))?;
    let raw = text(field(&seen, "raw"));
    let shown = {
        let value = text(field(&seen, "shown"));
        if value.is_empty() {
            raw.clone()
        } else {
            value
        }
    };
    let history = root.join(HISTORY);
    careerbrew_vault::lock::with_lock(&history, || -> Result<()> {
        let old = careerbrew_feed::read_jsonl(root, HISTORY)?;
        if source == "web"
            && old
                .iter()
                .any(|row| text(field(row, "day")) == day && text(field(row, "from")) == "sent")
        {
            return Ok(());
        }
        let mut rows = old
            .iter()
            .filter(|row| text(field(row, "day")) != day)
            .cloned()
            .collect::<Vec<_>>();
        if !raw.is_empty() && !strip(body).is_empty() && strip(body) != strip(&shown) {
            let mut row = json!({
                "day": day, "draft": raw, "shown": shown, "body": body,
                "saved": careerbrew_feed::now_utc_iso(), "from": source,
            });
            row["edit_rules"] = Value::Array(edit_rules(std::slice::from_ref(&row)));
            rows.push(row);
        }
        rows.sort_by_key(|row| text(field(row, "day")));
        let start = rows.len().saturating_sub(KEEP);
        let rows = rows.split_off(start);
        if rows != old {
            careerbrew_feed::write_jsonl(root, HISTORY, &rows)?;
        }
        Ok(())
    })??;
    let learned = rules(root, None)?;
    let count = |key: &str| match &learned[key] {
        Value::Array(items) => items.len(),
        Value::Object(items) => items.len(),
        _ => 0,
    };
    careerbrew_feed::emit(
        root,
        "day.pm.learned",
        vec![
            ("status", Canon::Str("ok".into())),
            ("source_kind", Canon::Str(source.into())),
            ("cut", Canon::Int(count("cut").into())),
            ("rename", Canon::Int(count("rename").into())),
            ("parts", Canon::Int(count("parts").into())),
            ("added", Canon::Int(count("added").into())),
            (
                "count",
                Canon::Int(learned["days"].as_i64().unwrap_or(0).into()),
            ),
        ],
    )?;
    Ok(())
}

/// Forget one label's lessons up to now. Returns the edited days it had.
pub fn reset(root: &Path, label: &str) -> Result<usize> {
    let key = norm(label);
    let had = careerbrew_feed::read_jsonl(root, HISTORY)?
        .iter()
        .filter(|row| {
            lines(&text(field(row, "draft")))
                .iter()
                .any(|(lab, _)| norm(lab) == key)
        })
        .count();
    if had == 0 {
        return Err(value_error(format!(
            "no PM lesson for {label} (see: careerbrew signals)"
        )));
    }
    let mut data = careerbrew_feed::read_json(root, RESETS, json!({}))?;
    if !data.is_object() {
        data = json!({});
    }
    data[key] = json!(careerbrew_feed::now_utc_iso());
    careerbrew_feed::write_json(root, RESETS, &data)?;
    Ok(had)
}

fn before(row: &Value, since: &str) -> bool {
    let saved = text(field(row, "saved"));
    if since.chars().count() == 10 || saved.is_empty() {
        let prefix = since.chars().take(10).collect::<String>();
        return text(field(row, "day")) <= prefix;
    }
    saved.as_str() <= since
}

fn no_action(value: &str) -> bool {
    let plain = URL.replace_all(value, "");
    let plain = strip(&plain);
    let (prefix, tail, sep) = match plain.split_once(':') {
        Some((prefix, tail)) => (prefix, tail, true),
        None => (plain, "", false),
    };
    let body = if sep && WORDISH.is_match(prefix) && !COMMIT_PREFIX.is_match(prefix) {
        strip(tail)
    } else {
        plain
    };
    NO_ACTION.is_match(body)
}

/// Candidate rules kept with each encrypted edit row.
pub fn edit_rules(rows: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for row in rows {
        let shown_text = {
            let value = text(field(row, "shown"));
            if value.is_empty() {
                text(field(row, "draft"))
            } else {
                value
            }
        };
        let shown = clean_lines(&shown_text);
        let saved = clean_lines(&text(field(row, "body")));
        for op in difflib::line_opcodes(&shown, &saved) {
            if op.tag == "equal" {
                continue;
            }
            let before_lines = &shown[op.i1..op.i2];
            let after_lines = &saved[op.j1..op.j2];
            for i in 0..before_lines.len().max(after_lines.len()) {
                let old = before_lines.get(i).cloned().unwrap_or_default();
                let new = after_lines.get(i).cloned().unwrap_or_default();
                let rule = if IN_DRAFT.is_match(&new) && !IN_DRAFT.is_match(&old) {
                    "mark unfinished work In draft; do not claim it shipped"
                } else if no_action(&old)
                    && (new.is_empty() || !casefold(&new).contains(&casefold(&old)))
                {
                    "drop a status check with no action or outcome"
                } else if old.is_empty() {
                    "include this missing work when fresh sources support it"
                } else if new.is_empty() {
                    "leave out this removed line when its context recurs"
                } else {
                    "keep the owner's replacement only when the same source facts recur"
                };
                let change = if !old.is_empty() && !new.is_empty() {
                    "replace"
                } else if !old.is_empty() {
                    "remove"
                } else {
                    "add"
                };
                out.push(json!({"day": text(field(row, "day")), "change": change, "before": old, "after": new, "rule": rule}));
            }
        }
    }
    out
}

fn clean_lines(value: &str) -> Vec<String> {
    splitlines(value)
        .iter()
        .map(|line| strip(line).to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

type Labeled = Vec<(String, String)>;

fn row_texts(row: &Value) -> (Labeled, Labeled, Labeled) {
    let draft = text(field(row, "draft"));
    let shown = text(field(row, "shown"));
    let shown = if shown.is_empty() {
        draft.clone()
    } else {
        shown
    };
    (
        lines(&draft),
        lines(&shown),
        lines(&text(field(row, "body"))),
    )
}

fn resets_map(root: &Path) -> Result<Map<String, Value>> {
    Ok(careerbrew_feed::read_json(root, RESETS, json!({}))?
        .as_object()
        .cloned()
        .unwrap_or_default())
}

/// What the owner's edits teach: cut labels, renames, cut parts, an order.
pub fn rules(root: &Path, rows: Option<&[Value]>) -> Result<Value> {
    let loaded;
    let rows = match rows {
        Some(rows) => rows,
        None => {
            loaded = careerbrew_feed::read_jsonl(root, HISTORY)?;
            &loaded[..]
        }
    };
    let resets = resets_map(root)?;
    let mut cut: Vec<(String, [i64; 2])> = Vec::new();
    let mut renames: Vec<((String, String), i64)> = Vec::new();
    let mut part_counts: Vec<((String, String), [i64; 2])> = Vec::new();
    let mut position: Vec<(String, Vec<i64>)> = Vec::new();
    let mut added_counts: BTreeMap<String, i64> = BTreeMap::new();
    fn slot<'a, K: PartialEq + Clone, V: Clone>(
        items: &'a mut Vec<(K, V)>,
        key: &K,
        default: V,
    ) -> &'a mut V {
        if let Some(index) = items.iter().position(|(k, _)| k == key) {
            return &mut items[index].1;
        }
        items.push((key.clone(), default));
        &mut items.last_mut().expect("pushed").1
    }
    for row in rows {
        let (raw, shown, body) = row_texts(row);
        let mut both = raw.clone();
        both.extend(shown.iter().cloned());
        let hints = body
            .iter()
            .filter(|(lab, value)| added(lab, value, &both))
            .map(|(lab, value)| hint(&format!("{lab} {value}")))
            .collect::<BTreeSet<_>>();
        for h in hints {
            *added_counts.entry(h.to_owned()).or_insert(0) += 1;
        }
        for (n, (label, _)) in body.iter().enumerate() {
            slot(&mut position, label, Vec::new()).push(n as i64);
        }
        for (label, value) in &raw {
            let key = norm(label);
            if let Some(Value::String(since)) = resets.get(&key) {
                if !since.is_empty() && before(row, since) {
                    continue;
                }
            }
            let seen = find(label, value, &shown);
            let got = find(label, value, &body);
            let counts = slot(&mut cut, label, [0, 0]);
            if seen.is_some() && got.is_none() {
                counts[0] += 1;
            } else if got.is_some() {
                counts[1] += if seen.is_some() { 1 } else { 2 };
            }
            if let (Some(got), Some(seen)) = (&got, &seen) {
                if norm(&got.0) != norm(&seen.0) {
                    if norm(&got.0) == key {
                        *slot(&mut renames, &(label.clone(), seen.0.clone()), 0) -= 2;
                    } else {
                        *slot(&mut renames, &(label.clone(), got.0.clone()), 0) += 1;
                    }
                }
                for part in parts(value) {
                    let counts = slot(&mut part_counts, &(label.clone(), part.clone()), [0, 0]);
                    if parts(&seen.1).contains(&part) && !parts(&got.1).contains(&part) {
                        counts[0] += 1;
                    } else if parts(&got.1).contains(&part) {
                        counts[1] += if parts(&seen.1).contains(&part) { 1 } else { 2 };
                    }
                }
            }
        }
    }
    let mut sorted_renames = renames.clone();
    sorted_renames.sort_by_key(|(_, n)| *n);
    let mut rename = Map::new();
    for ((old, new), n) in sorted_renames {
        if n >= MIN_TIMES {
            rename.insert(old, json!(new));
        }
    }
    let mut cut_labels = cut
        .iter()
        .filter(|(label, [c, kept])| *c >= MIN_TIMES && c > kept && !rename.contains_key(label))
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    cut_labels.sort();
    let mut part_list = part_counts
        .iter()
        .filter(|(_, [c, kept])| *c >= MIN_TIMES && c > kept)
        .map(|((label, part), _)| vec![label.clone(), part.clone()])
        .collect::<Vec<_>>();
    part_list.sort();
    let order = position
        .iter()
        .map(|(label, ns)| {
            let average = ns.iter().sum::<i64>() as f64 / ns.len() as f64;
            (label.clone(), json!(average))
        })
        .collect::<Map<_, _>>();
    let added_map = added_counts
        .into_iter()
        .filter(|(_, n)| *n >= MIN_TIMES)
        .map(|(h, n)| (h, json!(n)))
        .collect::<Map<_, _>>();
    Ok(json!({
        "cut": cut_labels,
        "rename": rename,
        "parts": part_list,
        "order": order,
        "added": added_map,
        "days": rows.len(),
        "edit_rules": edit_rules(rows),
    }))
}

fn learned_cut(learned: &Value) -> Vec<String> {
    learned["cut"]
        .as_array()
        .map(|items| items.iter().map(|item| text(Some(item))).collect())
        .unwrap_or_default()
}

fn split_parts(body: &str) -> Vec<String> {
    let body = body.trim_end_matches('.');
    let chars = body.char_indices().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < chars.len() {
        let (offset, ch) = chars[index];
        if ch == ';' {
            let mut end = index + 1;
            while end < chars.len() && crate::is_py_space(chars[end].1) {
                end += 1;
            }
            if end > index + 1 {
                let rest_at = chars.get(end).map(|(o, _)| *o).unwrap_or(body.len());
                let rest = &body[rest_at..];
                if rest.starts_with("next:") || rest.starts_with("overdue:") {
                    out.push(body[start..offset].to_owned());
                    start = rest_at;
                    index = end;
                    continue;
                }
            }
        }
        index += 1;
    }
    out.push(body[start..].to_owned());
    out
}

/// The draft's lines after the owner's lessons. Never empties the draft.
pub fn apply(draft: &[String], ranked: &BTreeSet<String>, learned: &Value) -> Vec<String> {
    if learned["days"].as_i64().unwrap_or(0) == 0 {
        return draft.to_vec();
    }
    let cut = learned_cut(learned);
    let drop = learned["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|pair| (text(pair.get(0)), text(pair.get(1))))
        .collect::<BTreeSet<_>>();
    let rename = learned["rename"].as_object().cloned().unwrap_or_default();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for line in draft {
        let (label, body) = split(line);
        if cut.contains(&label) {
            continue;
        }
        let kept = split_parts(&body)
            .iter()
            .map(|part| strip(part).to_owned())
            .filter(|part| {
                !part.is_empty() && {
                    let key = part.split(':').next().unwrap_or("").to_owned();
                    !drop.contains(&(label.clone(), key))
                }
            })
            .collect::<Vec<_>>();
        if kept.is_empty() {
            continue;
        }
        let written = rename
            .get(&label)
            .map(|value| text(Some(value)))
            .unwrap_or_else(|| label.clone());
        out.push((label, written, format!("{}.", kept.join("; "))));
    }
    let order = learned["order"].as_object().cloned().unwrap_or_default();
    let fallback = order.len() as f64 + 1.0;
    let rank_of = |row: &(String, String, String)| {
        order
            .get(&row.1)
            .or_else(|| order.get(&row.0))
            .and_then(Value::as_f64)
            .unwrap_or(fallback)
    };
    let mut result_rows = out
        .iter()
        .filter(|row| ranked.contains(&row.0))
        .cloned()
        .collect::<Vec<_>>();
    let mut rest = out
        .iter()
        .filter(|row| !ranked.contains(&row.0))
        .cloned()
        .collect::<Vec<_>>();
    rest.sort_by(|a, b| {
        rank_of(a)
            .partial_cmp(&rank_of(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result_rows.extend(rest);
    let result = result_rows
        .into_iter()
        .map(|(_, label, body)| {
            if label.is_empty() {
                body
            } else {
                format!("{label}: {body}")
            }
        })
        .collect::<Vec<_>>();
    if result.is_empty() {
        draft.to_vec()
    } else {
        result
    }
}

pub fn left_out(raw: &[String], learned: &Value) -> Vec<String> {
    let cut = learned_cut(learned);
    raw.iter()
        .map(|line| split(line).0)
        .filter(|label| cut.contains(label))
        .collect()
}

fn cut_date(label: &str, rows: &[Value], resets: &Map<String, Value>) -> Value {
    let key = norm(label);
    let mut dated: Vec<(
        chrono::DateTime<chrono::FixedOffset>,
        String,
        Option<String>,
    )> = Vec::new();
    let mut days: Vec<String> = Vec::new();
    for row in rows {
        if let Some(Value::String(since)) = resets.get(&key) {
            if !since.is_empty() && before(row, since) {
                continue;
            }
        }
        let (raw, shown, body) = row_texts(row);
        if !raw.iter().any(|(lab, value)| {
            norm(lab) == key
                && find(lab, value, &shown).is_some()
                && find(lab, value, &body).is_none()
        }) {
            continue;
        }
        let day = text(field(row, "day"));
        if iso_day(&day).is_ok() {
            days.push(day.clone());
        }
        let saved = text(field(row, "saved"));
        if let Some(crate::Stamp::Aware(stamp)) = crate::parse_iso(&saved) {
            dated.push((stamp, saved, days.contains(&day).then_some(day)));
        }
    }
    let mut best: Option<&(
        chrono::DateTime<chrono::FixedOffset>,
        String,
        Option<String>,
    )> = None;
    for entry in &dated {
        if best.is_none_or(|top| entry.0 > top.0) {
            best = Some(entry);
        }
    }
    if let Some((_, saved, day)) = best {
        return json!({"recorded_at": saved, "report_day": day, "date_basis": "saved"});
    }
    let latest = days.iter().max().cloned();
    let basis = if days.is_empty() {
        "unavailable"
    } else {
        "report-day"
    };
    json!({"recorded_at": null, "report_day": latest, "date_basis": basis})
}

/// Project learned cut labels against current text without changing lessons.
pub fn cut_projection(
    root: &Path,
    raw: &[String],
    body: &str,
    learned: &Value,
    rows: &[Value],
) -> Result<(Vec<String>, Value)> {
    let resets = resets_map(root)?;
    let rename = learned["rename"].as_object().cloned().unwrap_or_default();
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut spellings = BTreeSet::new();
    for label in left_out(raw, learned) {
        let key = norm(&label);
        if !seen.insert(key) {
            continue;
        }
        let renamed = rename
            .get(&label)
            .map(|value| text(Some(value)))
            .unwrap_or_else(|| label.clone());
        let mut names = vec![label.clone()];
        if renamed != label {
            names.push(renamed);
        }
        let aliases = names
            .iter()
            .map(|spelling| {
                spellings.insert(spelling.clone());
                json!({"spelling": spelling, "norm": norm(spelling)})
            })
            .collect::<Vec<_>>();
        let mut candidate = json!({"label": label, "aliases": aliases});
        if let (Some(target), Value::Object(extra)) =
            (candidate.as_object_mut(), cut_date(&label, rows, &resets))
        {
            target.extend(extra);
        }
        candidates.push(candidate);
    }
    let mut present = BTreeSet::new();
    let mut unknown = false;
    for line in splitlines(body) {
        let clean = strip(strip(&line).trim_start_matches(['-', '•', '◦', '+', ' ']));
        if clean.is_empty() {
            continue;
        }
        let (label, _) = split(clean);
        if label.is_empty() || UNKNOWN_LINE.is_match(clean) || label.contains('|') {
            unknown = true;
        } else if label.is_ascii() || spellings.contains(&label) {
            present.insert(norm(&label));
        } else {
            unknown = true;
        }
    }
    let absent = candidates
        .iter()
        .filter(|candidate| {
            !candidate["aliases"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|alias| present.contains(&text(field(alias, "norm"))))
        })
        .map(|candidate| text(field(candidate, "label")))
        .collect::<Vec<_>>();
    Ok((
        if unknown { Vec::new() } else { absent },
        json!({"schema": 1, "candidates": candidates}),
    ))
}
