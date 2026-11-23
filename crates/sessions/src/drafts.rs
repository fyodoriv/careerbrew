//! Newest-thread reply decisions and clear-save-read-back draft transactions.

use crate::{digest, error, field, read_text, sessions, text, write_text, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_ACK: &str = "Looks good, approved. Thanks for shipping this.";
const LOCK_WAIT: Duration = Duration::from_secs(10);

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("draft regex")
}

pub fn check_target(target: &str) -> Result<String> {
    let target = target.trim();
    if !regex(r"^[A-Za-z0-9][A-Za-z0-9._:/#@-]{0,127}$").is_match(target) || target.contains("..") {
        return Err(error("DraftError", format!("draft target {:?}: use a short id such as C123/1700000000.000100 (letters, digits, . _ : / # @ -)", careerbrew_projects::chars(target, 40))));
    }
    Ok(target.to_owned())
}

pub fn uses_entry(target: &str) -> Result<String> {
    Ok(format!("slack-draft:{}", check_target(target)?))
}

fn target_digest(target: &str) -> String {
    digest(target)[..24].to_owned()
}

pub trait Backend {
    fn root(&self) -> &Path;
    fn clear(&self, target: &str) -> Result<()>;
    fn save(&self, target: &str, text: &str) -> Result<()>;
    fn read(&self, target: &str) -> Result<Option<String>>;
}

pub struct LocalBackend {
    root: PathBuf,
}

impl LocalBackend {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.join("drafts"),
        }
    }

    pub fn path(&self, target: &str) -> PathBuf {
        self.root.join(format!("{}.txt", target_digest(target)))
    }
}

impl Backend for LocalBackend {
    fn root(&self) -> &Path {
        &self.root
    }

    fn clear(&self, target: &str) -> Result<()> {
        match std::fs::remove_file(self.path(target)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn save(&self, target: &str, text: &str) -> Result<()> {
        let relative = self.path(target);
        let relative = relative
            .strip_prefix(self.root.parent().expect("data root"))
            .expect("draft path");
        write_text(
            self.root.parent().expect("data root"),
            &relative.to_string_lossy(),
            text,
        )
    }

    fn read(&self, target: &str) -> Result<Option<String>> {
        let relative = self.path(target);
        let relative = relative
            .strip_prefix(self.root.parent().expect("data root"))
            .expect("draft path");
        read_text(
            self.root.parent().expect("data root"),
            &relative.to_string_lossy(),
        )
    }
}

pub fn normalize(text: Option<&str>) -> String {
    let text = text.unwrap_or("").replace("\r\n", "\n").replace('\r', "\n");
    text.split('\n')
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
}

pub fn compare(sent: &str, read_back: Option<&str>) -> Value {
    let left = normalize(Some(sent));
    let right = normalize(read_back);
    if left == right {
        return json!({"same": true, "line": 0, "sent_chars": left.chars().count(), "read_chars": right.chars().count()});
    }
    let a = left.split('\n').collect::<Vec<_>>();
    let b = right.split('\n').collect::<Vec<_>>();
    let mut line = a.len().min(b.len()) + 1;
    for (index, (x, y)) in a.iter().zip(&b).enumerate() {
        if x != y {
            line = index + 1;
            break;
        }
    }
    json!({"same": false, "line": line, "sent_chars": left.chars().count(), "read_chars": right.chars().count()})
}

pub struct TargetLock {
    _file: File,
}

pub fn locked(target: &str, wait: Duration, root: &Path) -> Result<TargetLock> {
    let lock = root.join(format!(".{}.lock", target_digest(target)));
    if let Some(parent) = lock.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().append(true).create(true).open(lock)?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(TargetLock { _file: file }),
            Err(_) if Instant::now() >= deadline => {
                return Err(error("DraftBusy", format!("another step is writing this draft; declare `uses: {}` on both steps so they run in different stages", uses_entry(target)?)));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn claim(target: &str, session: &str) -> Result<()> {
    let resource = uses_entry(target)?;
    if !session.is_empty() {
        return sessions::take(&sessions::root()?, &resource, session, None).map(|_| ());
    }
    if let Some(held) = sessions::holder(&sessions::root()?, &resource)? {
        return Err(error(
            "DraftClaimed",
            format!(
                "{} is claimed by session {} until {}: skip it, or pass that session's --session",
                sessions::shown(&resource),
                text(field(&held, "session")),
                &text(field(&held, "until"))[11..16]
            ),
        ));
    }
    Ok(())
}

pub fn replace_at(
    root: &Path,
    target: &str,
    text: &str,
    backend: &dyn Backend,
    wait: Duration,
    if_empty: bool,
    session: &str,
) -> Result<Value> {
    let target = check_target(target)?;
    if normalize(Some(text)).is_empty() {
        return Err(error("DraftError", "the draft is empty: write the reply first (an emoji suggestion is not saved as a draft)"));
    }
    claim(&target, session).map_err(|failure| {
        if failure.class == "ClaimHeld" {
            crate::error("DraftClaimed", failure.message)
        } else {
            failure
        }
    })?;
    let location = if backend.root() == root.join("drafts") {
        "local draft store"
    } else {
        "draft target"
    };
    let result = {
        let _lock = locked(&target, wait, backend.root())?;
        if if_empty && !normalize(backend.read(&target)?.as_deref()).is_empty() {
            return Ok(json!({"target": target, "kept": true}));
        }
        backend.clear(&target)?;
        if !normalize(backend.read(&target)?.as_deref()).is_empty() {
            return Err(error("DraftMismatch", format!("the old draft is still there after clearing it; nothing was saved (fix: check the {location} before retrying)")));
        }
        backend.save(&target, text)?;
        compare(text, backend.read(&target)?.as_deref())
    };
    if result["same"] != true {
        return Err(error("DraftMismatch", format!("the saved draft differs from the text you wrote at line {} ({} vs {} characters); check the {location} before anyone sends it", result["line"], result["read_chars"], result["sent_chars"])));
    }
    Ok(
        json!({"target": target, "kept": false, "same": true, "line": result["line"], "sent_chars": result["sent_chars"], "read_chars": result["read_chars"]}),
    )
}

pub fn replace(
    root: &Path,
    target: &str,
    text: &str,
    if_empty: bool,
    session: &str,
) -> Result<Value> {
    replace_at(
        root,
        target,
        text,
        &LocalBackend::new(root),
        LOCK_WAIT,
        if_empty,
        session,
    )
}

fn words(text: &str) -> String {
    let lowercase = text.to_lowercase();
    let text = regex(r"<[^>]*>|:[a-z0-9_+-]+:|@\w+").replace_all(&lowercase, " ");
    regex(r"[^a-z0-9+' ]+")
        .replace_all(&text, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn small_ack(text: &str) -> Option<&'static str> {
    let mut words = words(text);
    if words.is_empty() {
        return if text.trim().is_empty() {
            None
        } else {
            Some("+1")
        };
    }
    if words.split_whitespace().count() > 4 {
        return None;
    }
    words = regex(r"^(ok|okay|great|cool|perfect|awesome) (thanks|thank you|thx|ty)$")
        .replace(&words, "$2")
        .into_owned();
    words = regex(r" (all|team|everyone|guys|folks|so much|again|man|mate)$")
        .replace(&words, "")
        .into_owned();
    const SMALL: &[(&str, &[&str])] = &[
        (
            "pray",
            &[
                "thanks",
                "thank you",
                "thx",
                "ty",
                "tysm",
                "thanks a lot",
                "thank you so much",
                "much appreciated",
                "cheers",
            ],
        ),
        (
            "+1",
            &[
                "+1",
                "ok",
                "okay",
                "k",
                "kk",
                "got it",
                "will do",
                "sounds good",
                "sg",
                "sure",
                "noted",
                "ack",
                "roger",
                "yes",
                "yep",
            ],
        ),
        (
            "white_check_mark",
            &[
                "done", "fixed", "merged", "lgtm", "approved", "shipped", "resolved",
            ],
        ),
        (
            "raised_hands",
            &[
                "great",
                "nice",
                "cool",
                "perfect",
                "awesome",
                "amazing",
                "great work",
                "nice work",
                "well done",
            ],
        ),
    ];
    SMALL
        .iter()
        .find(|(_, phrases)| phrases.contains(&words.as_str()))
        .map(|(emoji, _)| *emoji)
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    for (index, ch) in chars.iter().enumerate() {
        current.push(*ch);
        let boundary = *ch == '\n'
            || (matches!(ch, '.' | '!' | '?')
                && chars
                    .get(index + 1)
                    .is_some_and(|next| next.is_whitespace()));
        if boundary {
            if !current.trim().is_empty() {
                out.push(current.trim().to_owned());
            }
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_owned());
    }
    out
}

fn asks(sentence: &str) -> bool {
    sentence.trim_end().ends_with('?')
        && regex(r"(?i)^(did|has|have|is|are|was|were|can|could|should|when|will|shall|do|does|any|anyone)\b")
            .is_match(sentence.trim())
}

pub fn deploy_started(text: &str) -> bool {
    let future = regex(
        r"(?i)\b(will|tomorrow|later|tonight|planning|plan to|scheduled?|going to|before|once|when|should|shall|can we|could we|ready to|about to|not|never|yet to|isn't|hasn't|haven't|didn't|wasn't|won't)\b",
    );
    let started = [
        regex(
            r"(?i)\b(deploying|releasing|rolling out|rolling it out|pushing to prod(uction)?|shipping it)\b",
        ),
        regex(
            r"(?i)\b(deploy|deployment|release|rollout|roll-out)\b[^\n]{0,40}?\b(start(ed|ing)?|began|begun|kicked off|in progress|underway|running|is out|went out|live|done|finished|completed?)\b",
        ),
        regex(
            r"(?i)\b(start(ed|ing)?|began|kicked off|triggered)\b[^\n]{0,30}?\b(deploy|deployment|release|rollout)",
        ),
        regex(r"(?i)\b(deployed|released|shipped)\b"),
    ];
    for sentence in sentences(text) {
        if asks(&sentence) {
            continue;
        }
        for pattern in &started {
            if let Some(found) = pattern.find(&sentence) {
                if !future.is_match(&sentence[..found.end()]) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn deploy_stopped(text: &str) -> bool {
    let stopped = regex(
        r"(?i)\b(rolled back|rolling back|rollback|reverted|reverting|aborted|halted|cancell?ed|paused|stopped|(deploy(ment)?|release|rollout) failed)\b",
    );
    sentences(text)
        .into_iter()
        .any(|sentence| !asks(&sentence) && stopped.is_match(&sentence))
}

fn ts_key(value: &Value) -> (u8, f64, String) {
    match value {
        Value::Number(number) => (0, number.as_f64().unwrap_or(0.0), String::new()),
        Value::String(text) => match text.parse::<f64>() {
            Ok(number) => (0, number, String::new()),
            Err(_) => (1, 0.0, text.clone()),
        },
        _ => (1, 0.0, value.to_string()),
    }
}

pub fn messages(thread: &Value) -> Result<Vec<Value>> {
    let raw = if thread.is_object() {
        thread.get("messages")
    } else {
        Some(thread)
    };
    let Some(raw) = raw
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
    else {
        return Err(error("DraftError", "the thread has no messages: pass {\"messages\": [{\"ts\": ..., \"text\": ..., \"mine\": false}, ...]}"));
    };
    let mut out = Vec::new();
    for (index, row) in raw.iter().enumerate() {
        if !row.is_object() || !row.get("text").is_none_or(Value::is_string) {
            return Err(error(
                "DraftError",
                format!(
                    "message {}: use an object with text, ts, and mine",
                    index + 1
                ),
            ));
        }
        out.push(json!({
            "ts": row.get("ts").cloned().unwrap_or_else(|| json!(index)),
            "text": text(row.get("text")),
            "mine": row.get("mine").and_then(Value::as_bool).unwrap_or(false) || row.get("author") == Some(&json!("me")),
            "n": index,
        }));
    }
    if raw.iter().all(|row| row.get("ts").is_some()) {
        out.sort_by(|left, right| {
            ts_key(field(left, "ts").expect("ts"))
                .partial_cmp(&ts_key(field(right, "ts").expect("ts")))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| {
                    field(left, "n")
                        .and_then(Value::as_u64)
                        .cmp(&field(right, "n").and_then(Value::as_u64))
                })
        });
    }
    Ok(out)
}

pub fn fit(
    thread: &[Value],
    draft: Option<&str>,
    seen: Option<&Value>,
    major: Option<&str>,
) -> Result<Value> {
    let major = major.map(one_line_major).transpose()?;
    let newest = thread
        .last()
        .ok_or_else(|| error("DraftError", "the thread has no messages"))?;
    let newer = seen.map_or(0, |seen| {
        let key = ts_key(seen);
        thread
            .iter()
            .filter(|row| ts_key(field(row, "ts").expect("ts")) > key)
            .count()
    });
    let mut out = json!({
        "kind": "reply", "emoji": Value::Null, "text": Value::Null,
        "keep_draft": draft.is_some(), "newest_ts": field(newest, "ts").cloned().unwrap_or(Value::Null),
        "newer": newer, "stale": newer > 0,
    });
    let mut started = false;
    for row in thread {
        let row_text = text(field(row, "text"));
        if deploy_stopped(&row_text) {
            started = false;
        } else if deploy_started(&row_text) {
            started = true;
        }
    }
    out["deploy_started"] = json!(started);
    let change = draft.is_some_and(|draft| {
        regex(r"(?i)\b(could you|can you|would you|please (change|update|fix|rename|add|remove|move|revert)|should (we|be|use|have)|instead of|nit|before (we|you|this|it) (merge|ship|deploy|release)|let'?s (change|hold|wait)|request(ing)? changes|needs? (a |to )?(change|fix))\b").is_match(draft)
            || draft.trim().ends_with('?')
    });
    if field(newest, "mine")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        out["kind"] = json!("none");
        out["keep_draft"] = json!(false);
        out["reason"] = json!("your message is the newest; wait for a reply");
    } else if let Some(emoji) =
        small_ack(&text(field(newest, "text"))).filter(|_| !(started && major.is_some()))
    {
        out["kind"] = json!("emoji");
        out["emoji"] = json!(emoji);
        out["keep_draft"] = json!(false);
        out["reason"] = json!("the newest message is a small acknowledgement: react, do not reply");
    } else if let Some(major) = major.as_deref().filter(|_| started) {
        out["kind"] = json!("stop");
        out["text"] = json!(format!("Please hold the release: {major}"));
        out["keep_draft"] = json!(false);
        out["reason"] = json!("a release has started and you named a major issue");
    } else if started {
        let keep = draft.is_some_and(|draft| {
            regex(r"(?i)\b(lgtm|approved?|ship it|looks good|go ahead|nice work|thanks|thank you|great work)\b|:shipit:").is_match(draft) && !change
        });
        out["kind"] = json!("ack");
        out["text"] = if keep {
            Value::Null
        } else {
            json!(DEFAULT_ACK)
        };
        out["keep_draft"] = json!(keep);
        out["reason"] = json!(format!(
            "a release has started: approve or acknowledge{}",
            if change {
                "; your draft asked for changes, so it was replaced"
            } else {
                ""
            }
        ));
    } else if let Some(emoji) = draft.and_then(small_ack) {
        out["kind"] = json!("emoji");
        out["emoji"] = json!(emoji);
        out["keep_draft"] = json!(false);
        out["reason"] = json!("your draft is a small acknowledgement: react instead");
    } else {
        out["reason"] = json!(if draft.is_none() {
            "write the reply to the newest message"
        } else {
            "your draft fits; keep it"
        });
    }
    if newer > 0 && out["kind"] == "reply" && draft.is_some() {
        out["keep_draft"] = json!(false);
        out["reason"] = json!(format!(
            "{newer} newer message(s) since your draft: re-read them and redraft"
        ));
    }
    Ok(out)
}

fn one_line_major(value: &str) -> Result<String> {
    let value = crate::one_line(value);
    if value.is_empty() || value.chars().count() > 200 {
        return Err(error(
            "DraftError",
            "--major: say why in one line (1 to 200 characters)",
        ));
    }
    Ok(value)
}
