//! Access-ask decisions (rust-04e) from `src/careerbrew/work/access.py`
//! over a supplied directory: `validate_directory`, `blocker`, `_topic`,
//! `owner`, `existing`, `route` (key insertion order kept), `mentions` and
//! `judge`. Reading the directory file stays with rust-05.

use crate::allow::PyError;
use crate::approvals::key_error;
use crate::canary::Rules;
use crate::pyre::Regex;
use crate::text;
use crate::vectors::Out;

/// `access.BLOCKER` (`re.I`).
const BLOCKER: &str = r"\b(permission|permissions|access|denied|forbidden|unauthori[sz]ed|not allowed|policy|role|group|entitlement)\b";
/// `access.ASKS` (`re.I`).
const ASKS: &str = r"\b(need|needs|missing|request|requesting|grant|blocked|can't|cannot|no)\b";
/// `access.MENTION`.
pub const MENTION: &str = r"<@([A-Z0-9]+)>|(?<![\w<])@([A-Za-z0-9][\w.-]*)";
/// `MENTION` with the `<` removed from the lookbehind (a planted defect).
const MENTION_DEFECT: &str = r"<@([A-Z0-9]+)>|(?<![\w])@([A-Za-z0-9][\w.-]*)";

fn access_error(message: impl Into<String>) -> PyError {
    PyError::new("AccessError", message)
}

fn unsupported() -> PyError {
    PyError::new("TypeError", "unsupported input shape")
}

fn regex(rules: &Rules, src: &str, icase: bool) -> Result<Regex, PyError> {
    Regex::new(src, icase, rules).map_err(|e| PyError::new("error", e))
}

/// `d.get(key)` of a dict.
pub fn get<'a>(d: &'a Out, key: &str) -> Option<&'a Out> {
    match d {
        Out::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn is_strings(v: &Out) -> bool {
    matches!(v, Out::List(items) if items.iter().all(|x| matches!(x, Out::Str(_))))
}

/// `access.validate_directory`: deny by default; the directory unchanged when valid.
pub fn validate_directory(d: &Out) -> Result<Out, PyError> {
    if !matches!(d, Out::Dict(_)) {
        return Err(access_error("directory: not an object"));
    }
    let Some(Out::List(channels)) = get(d, "channels") else {
        return Err(access_error("directory: channels is not a list"));
    };
    if get(d, "threads").is_some_and(|t| !matches!(t, Out::List(_))) {
        return Err(access_error("directory: threads is not a list"));
    }
    let mut seen: Vec<&str> = Vec::new();
    for (i, c) in channels.iter().enumerate() {
        if !matches!(c, Out::Dict(_)) {
            return Err(access_error(format!(
                "directory channel {i}: not an object"
            )));
        }
        let Some(Out::Str(id)) = get(c, "id") else {
            return Err(access_error(format!("directory channel {i}: missing id")));
        };
        for key in ["owns", "members"] {
            if get(c, key).is_some_and(|v| !is_strings(v)) {
                return Err(access_error(format!(
                    "directory channel {i}: {key} is not a list of strings"
                )));
            }
        }
        if seen.contains(&id.as_str()) {
            return Err(access_error(format!("directory channel {i}: duplicate id")));
        }
        seen.push(id);
    }
    if let Some(Out::List(threads)) = get(d, "threads") {
        for (i, t) in threads.iter().enumerate() {
            if !matches!(t, Out::Dict(_)) {
                return Err(access_error(format!("directory thread {i}: not an object")));
            }
        }
    }
    Ok(d.clone())
}

/// `access.blocker`.
pub fn blocker(rules: &Rules, text_in: &str) -> Result<bool, PyError> {
    Ok(regex(rules, BLOCKER, true)?.is_match(text_in)
        && regex(rules, ASKS, true)?.is_match(text_in))
}

/// Python `str()` of a scalar; other shapes are out of contract.
fn py_str(v: &Out) -> Result<String, PyError> {
    Ok(match v {
        Out::Str(s) => s.clone(),
        Out::Null => "None".into(),
        Out::Bool(b) => (if *b { "True" } else { "False" }).into(),
        Out::Int(n) => n.to_string(),
        _ => return Err(unsupported()),
    })
}

fn list(v: Option<&Out>) -> Result<&[Out], PyError> {
    match v {
        None => Ok(&[]),
        Some(Out::List(items)) => Ok(items),
        Some(_) => Err(unsupported()),
    }
}

/// `access._topic`: the longest owned topic the text names (first on a tie), else `""`.
pub fn topic(text_in: &str, channel: &Out) -> Result<Out, PyError> {
    let low = text::lower(text_in);
    let mut best: Option<(&Out, usize)> = None;
    for t in list(get(channel, "owns"))? {
        let word = text::lower(&py_str(t)?);
        if low.contains(&word) {
            let n = match t {
                Out::Str(s) => text::len(s),
                _ => return Err(unsupported()),
            };
            if best.is_none_or(|(_, m)| n > m) {
                best = Some((t, n));
            }
        }
    }
    Ok(best.map_or(Out::Str(String::new()), |(t, _)| t.clone()))
}

fn topic_str(text_in: &str, channel: &Out) -> Result<String, PyError> {
    match topic(text_in, channel)? {
        Out::Str(s) => Ok(s),
        _ => Err(unsupported()),
    }
}

/// `access.owner`: the channel whose owned topic the text names (the
/// longest match wins; the first channel on a tie) and that topic.
pub fn owner(text_in: &str, directory: &Out) -> Result<(Option<Out>, String), PyError> {
    let channels = list(Some(
        get(directory, "channels").ok_or_else(|| key_error("channels"))?,
    ))?;
    let mut best: (Option<Out>, String) = (None, String::new());
    for c in channels {
        let t = topic_str(text_in, c)?;
        if text::len(&t) > text::len(&best.1) {
            best = (Some(c.clone()), t);
        }
    }
    Ok(best)
}

/// `access.existing`: the first open thread in the channel that names the topic.
pub fn existing(channel: &Out, topic_in: &str, directory: &Out) -> Result<Option<Out>, PyError> {
    let threads = list(Some(
        get(directory, "threads").ok_or_else(|| key_error("threads"))?,
    ))?;
    let want = text::lower(topic_in);
    for t in threads {
        if get(t, "channel") != get(channel, "id") {
            continue;
        }
        let body = match get(t, "text") {
            None => String::new(),
            Some(v) => py_str(v)?,
        };
        if text::lower(&body).contains(&want) {
            return Ok(Some(t.clone()));
        }
    }
    Ok(None)
}

fn s(v: &str) -> Out {
    Out::Str(v.to_owned())
}

/// `access.route`: where the ask goes, keys in Python's insertion order.
pub fn route(
    rules: &Rules,
    text_in: &str,
    directory: &Out,
    teammate: &str,
) -> Result<Out, PyError> {
    validate_directory(directory)?;
    if !blocker(rules, text_in)? {
        return Ok(Out::Dict(vec![
            ("action".into(), s("none")),
            ("reason".into(), s("not an access ask")),
        ]));
    }
    let (channel, topic_in) = owner(text_in, directory)?;
    let Some(c) = channel else {
        return Ok(Out::Dict(vec![
            ("action".into(), s("unknown")),
            (
                "reason".into(),
                s("no channel in the directory owns this topic: add it to `owns`"),
            ),
        ]));
    };
    let mut r = vec![
        (
            "channel".into(),
            get(&c, "id").cloned().unwrap_or(Out::Null),
        ),
        ("topic".into(), s(&topic_in)),
    ];
    match existing(&c, &topic_in, directory)? {
        Some(t) => {
            r.push(("action".into(), s("link")));
            r.push((
                "thread".into(),
                get(&t, "url").cloned().unwrap_or_else(|| s("")),
            ));
            r.push((
                "reason".into(),
                s("an open thread already asks this: link it, never open a second one"),
            ));
        }
        None => {
            r.push(("action".into(), s("open")));
            r.push((
                "reason".into(),
                s("no open thread: draft the ask in the owning channel"),
            ));
        }
    }
    if !teammate.is_empty() {
        let member = list(get(&c, "members"))?.contains(&s(teammate));
        let tag = if member {
            "in thread"
        } else {
            "link from the original thread"
        };
        r.push(("tag".into(), s(tag)));
    }
    Ok(Out::Dict(r))
}

/// `access.mentions`: each Slack id or `@name`, in order.
pub fn mentions(rules: &Rules, text_in: &str) -> Result<Vec<String>, PyError> {
    let src = if rules.mention_angle {
        MENTION
    } else {
        MENTION_DEFECT
    };
    Ok(regex(rules, src, false)?
        .findall_groups(text_in)
        .into_iter()
        .map(|g| {
            if g[0].is_empty() {
                g[1].clone()
            } else {
                g[0].clone()
            }
        })
        .collect())
}

/// `access.judge`: every reason the draft may not be saved in the channel.
pub fn judge(
    rules: &Rules,
    draft: &str,
    channel_id: &str,
    directory: &Out,
    new_thread: bool,
) -> Result<Vec<String>, PyError> {
    validate_directory(directory)?;
    let channels = list(get(directory, "channels"))?;
    let Some(c) = channels
        .iter()
        .find(|c| get(c, "id") == Some(&s(channel_id)))
    else {
        return Ok(vec![format!(
            "channel {channel_id} is not in the directory"
        )]);
    };
    let mut fails = Vec::new();
    let t = topic_str(draft, c)?;
    if t.is_empty() {
        fails.push(format!(
            "channel {channel_id} does not own the draft's topic"
        ));
    } else if new_thread {
        if let Some(thread) = existing(c, &t, directory)? {
            let url = match get(&thread, "url") {
                None => String::new(),
                Some(v) => py_str(v)?,
            };
            fails.push(format!("duplicates the open thread {url}: link it instead"));
        }
    }
    let members = list(get(c, "members"))?;
    for m in mentions(rules, draft)? {
        if !members.contains(&s(&m)) {
            fails.push(format!("mentions {m}, who is not a member of {channel_id}"));
        }
    }
    Ok(fails)
}
