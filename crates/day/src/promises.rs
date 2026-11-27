//! Typed seam for sent-Tomorrow promise writes (Python `day/pm_promises.py`).
//!
//! The sent-day transaction calls `PromiseRecorder::record` while it holds
//! the canonical sent-day lock; a recorder takes the project-file lock next.
//! The full promise identity, observation and decision port is a recorded
//! rust-followup item. The synthetic provider supports only texts with no
//! Tomorrow bullets: it takes the project lock like Python and writes nothing,
//! and it refuses any text whose promises it would have to write.

use crate::{splitlines, strip, words, DayError, Result};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static BULLET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:[-*•–]|\d+[.)])\s+(.+)$").expect("bullet"));
static NAMED_TOMORROW: LazyLock<Regex> = LazyLock::new(|| {
    let weekday = r"(?:mon(?:day)?|tue(?:sday)?|wed(?:nesday)?|thu(?:rsday)?|fri(?:day)?|sat(?:urday)?|sun(?:day)?)";
    Regex::new(&format!(
        r"^(?:tomorrow[ \t]+(?:\({weekday}\)|-[ \t]+{weekday}))$"
    ))
    .expect("named tomorrow")
});

fn heading(line: &str) -> String {
    let trimmed = strip(line).trim_matches(|c| matches!(c, '#' | '*' | '_' | ' ' | ':'));
    crate::casefold(trimmed)
}

/// The Tomorrow bullets of a sent PM update (Python `pm_promises.bullets`).
pub fn bullets(value: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let (mut active, mut active_plain) = (false, false);
    for line in splitlines(value) {
        let head = heading(&line);
        let matched = BULLET.captures(&line);
        if head == "tomorrow" {
            if active && !active_plain {
                break;
            }
            active = true;
            active_plain = true;
            continue;
        }
        if matched.is_none() && (head == "next week" || NAMED_TOMORROW.is_match(&head)) {
            if active {
                break;
            }
            active = true;
            continue;
        }
        if !active || strip(&line).is_empty() {
            continue;
        }
        let Some(captures) = matched else {
            if line.chars().next().is_some_and(crate::is_py_space) && !found.is_empty() {
                let extra = words(&line).join(" ");
                let last = found.last_mut().expect("found");
                last.push(' ');
                last.push_str(&extra);
                continue;
            }
            break;
        };
        let title = words(&captures[1]).join(" ");
        if !found.contains(&title) {
            found.push(title);
        }
    }
    found
}

/// A sent-day promise writer. Callers hold the sent-day lock.
pub trait PromiseRecorder {
    fn record(&self, root: &Path, day: &str, text: &str, revision: &str) -> Result<()>;
}

/// Synthetic provider: project lock only, for texts with no Tomorrow bullets.
pub struct LockOnlyPromises;

impl PromiseRecorder for LockOnlyPromises {
    fn record(&self, root: &Path, _day: &str, text: &str, _revision: &str) -> Result<()> {
        careerbrew_projects::projects::with_lock(root, || Ok(()))?;
        if !bullets(text).is_empty() {
            return Err(DayError::new(
                "PromiseSeamUnsupported",
                "sent-Tomorrow promise writes are not native yet",
            ));
        }
        Ok(())
    }
}
