//! `work/linkedin.py`: a paste-ready LinkedIn Experience block, scrubbed.
//!
//! Models are off in this component (the fallback intro); a configured model
//! route stays Python. The plugin policy is the default (`public_metrics`
//! on): installed plugins are a later slice.

use crate::py::{self, Re};
use crate::{evidence, py_str, truthy, when, Error, Result};
use careerbrew_policy::{allow, canary, scrub, Rules};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const LIMIT: usize = 2000;
const TAG_RANK: &[(&str, i64)] = &[
    ("impact", 5),
    ("design", 4),
    ("leadership", 4),
    ("ownership", 3),
    ("delivery", 2),
    ("quality", 2),
    ("collaboration", 1),
    ("communication", 1),
    ("tooling", 1),
];
const VERBS: &[(&str, &str)] = &[
    ("feat", "Shipped"),
    ("perf", "Improved"),
    ("fix", "Fixed"),
    ("refactor", "Reworked"),
    ("docs", "Documented"),
    ("test", "Hardened"),
    ("ci", "Automated"),
];
const PAST: &[(&str, &str)] = &[
    ("add", "Added"),
    ("build", "Built"),
    ("create", "Created"),
    ("cut", "Cut"),
    ("design", "Designed"),
    ("fix", "Fixed"),
    ("implement", "Implemented"),
    ("improve", "Improved"),
    ("launch", "Launched"),
    ("lead", "Led"),
    ("migrate", "Migrated"),
    ("move", "Moved"),
    ("reduce", "Reduced"),
    ("refactor", "Refactored"),
    ("remove", "Removed"),
    ("ship", "Shipped"),
    ("speed", "Sped"),
    ("support", "Supported"),
    ("write", "Wrote"),
    ("automate", "Automated"),
    ("replace", "Replaced"),
];

fn ticket() -> Re {
    Re::new(r"\b[A-Z][A-Z0-9]{1,9}-\d+\b:?\s*", false)
}

fn prefix() -> Re {
    Re::new(
        r"^(feat|fix|perf|refactor|docs|test|ci|chore|style)(\([^)]*\))?!?:\s*",
        true,
    )
}

fn number() -> Re {
    Re::new(
        r"(\$\s?\d[\d,.]*\s?[kmb]?(?!\w)|\d[\d,.]*\s?(%|x|ms|s|hours?|minutes?|days?|weeks?|k|m)(?!\w))",
        true,
    )
}

fn rank_of(tag: &str) -> i64 {
    TAG_RANK
        .iter()
        .find(|(t, _)| *t == tag)
        .map_or(0, |(_, r)| *r)
}

fn privacy(error: allow::PyError) -> Error {
    Error::new(error.class, error.message)
}

/// `privacy.sensitive_terms()` without plugins: the term file, then the
/// profile's identity fields and `sensitive` list, longest first.
pub fn sensitive_terms() -> Result<Vec<String>> {
    let root = crate::root()?;
    let file = root.join("sensitive-terms.txt");
    let mut terms = if file.exists() {
        allow::read_terms(&crate::read_text(&file)?.unwrap_or_default())
    } else {
        Vec::new()
    };
    let profile = crate::read_yaml(&root.join("profile.yaml"), json!({}))?;
    for key in ["name", "employer", "email"] {
        if let Some(Value::String(value)) = profile.get(key) {
            if py::len(value) >= 3 && !allow::TEMPLATE_PLACEHOLDERS.contains(&value.as_str()) {
                terms.push(value.clone());
            }
        }
    }
    for extra in profile
        .get("sensitive")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Value::String(value) = extra {
            if py::len(value) >= 3 {
                terms.push(value.clone());
            }
        }
    }
    let mut seen = BTreeSet::new();
    terms.retain(|t| seen.insert(t.clone()));
    terms.sort_by_key(|t| std::cmp::Reverse(py::len(t)));
    Ok(terms)
}

/// `_clean(title, repos, terms, public_metrics)`.
pub fn clean(
    title: &str,
    repos: &BTreeSet<String>,
    terms: &[String],
    public_metrics: bool,
) -> Result<String> {
    let mut text = ticket().sub(title, "");
    let verb = prefix()
        .captures(&text)
        .and_then(|groups| groups[0].clone())
        .and_then(|head| {
            let head = py::lower(&head);
            VERBS.iter().find(|(k, _)| *k == head).map(|(_, v)| *v)
        })
        .unwrap_or("");
    text = prefix().sub(&text, "");
    let mut ordered = repos.iter().cloned().collect::<Vec<_>>();
    ordered.sort_by_key(|r| std::cmp::Reverse(py::len(r)));
    for repo in ordered {
        if py::len(&repo) >= 3 {
            let pattern = format!(r"(?:\b(?:the|a|an)\s+)?\b{}\b", py::escape(&repo));
            text = Re::new(&pattern, true).sub(&text, "an internal service");
        }
    }
    text =
        scrub::scrub_public(&Rules::python(), &text, terms, scrub::REPLACEMENT).map_err(privacy)?;
    if !public_metrics {
        text = number().sub(&text, "significantly");
    }
    text = py::strip_chars(&Re::new(r"\s+", false).sub(&text, " "), " .-:").to_owned();
    if text.is_empty() {
        return Ok(String::new());
    }
    let (first, rest) = text.split_once(' ').unwrap_or((&text, ""));
    let low = py::lower(first);
    if let Some((_, past)) = PAST.iter().find(|(k, _)| *k == low) {
        text = py::strip(&format!("{past} {rest}")).to_owned();
    } else if !verb.is_empty()
        && !low.ends_with("ed")
        && !PAST.iter().any(|(_, p)| py::lower(p) == low)
    {
        let mut chars = text.chars();
        let head = chars
            .next()
            .map(|c| py::lower(&c.to_string()))
            .unwrap_or_default();
        text = format!("{verb} {head}{}", chars.as_str());
    }
    let mut chars = text.chars();
    let head = chars
        .next()
        .map(|c| py::upper(&c.to_string()))
        .unwrap_or_default();
    Ok(format!("{head}{}.", chars.as_str()))
}

/// `_rank(item)`: (negated score, 0, at).
pub fn rank(item: &Value) -> (i64, i64, String) {
    let score = item
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|t| rank_of(&py_str(Some(t))))
        .max()
        .unwrap_or(0);
    let impact = crate::text_of(item.get("impact"));
    let title = crate::text_of(item.get("title"));
    let has_number = i64::from(number().search(&format!("{impact} {title}")));
    let impact_bonus = 3 * i64::from(truthy(item.get("impact")));
    (
        -(score + impact_bonus + 2 * has_number),
        0,
        crate::text_of(item.get("at")),
    )
}

pub struct Block {
    pub path: std::path::PathBuf,
    pub text: String,
    pub bullets: usize,
    pub public_metrics: bool,
    pub suspects: Vec<String>,
}

fn joined(intro: &str, picked: &[String]) -> String {
    let lines = picked.iter().map(|b| format!("• {b}")).collect::<Vec<_>>();
    format!("{intro}\n\n{}", lines.join("\n"))
}

/// `build(role, max_bullets, since)`.
pub fn build(role: &str, max_bullets: usize, since: Option<&str>) -> Result<Block> {
    let terms = sensitive_terms()?;
    let public_metrics = true;
    let items = evidence::load()?
        .into_iter()
        .filter(|e| match since {
            Some(since) if !since.is_empty() => crate::text_of(e.get("at")).as_str() >= since,
            _ => true,
        })
        .collect::<Vec<_>>();
    let repos: BTreeSet<String> = items
        .iter()
        .filter(|e| truthy(e.get("repo")))
        .map(|e| {
            let repo = py_str(e.get("repo"));
            repo.rsplit('/').next().unwrap_or("").to_owned()
        })
        .collect();
    let mut sorted = items.clone();
    sorted.sort_by_key(rank);
    let mut picked: Vec<String> = Vec::new();
    let mut seen = BTreeSet::new();
    for it in &sorted {
        if matches!(
            it.get("kind").and_then(Value::as_str),
            Some("slack" | "zoom")
        ) {
            continue;
        }
        let mut base = crate::text_of(it.get("title"));
        if truthy(it.get("impact")) {
            base = format!("{base}: {}", py_str(it.get("impact")));
        }
        let line = clean(&base, &repos, &terms, public_metrics)?;
        let key = py::take(&py::lower(&line), 60);
        if py::len(&line) < 12 || seen.contains(&key) {
            continue;
        }
        let bare = prefix().sub(&ticket().sub(&base, ""), "");
        if (py::len(&line) as f64) < 0.5 * py::len(&bare) as f64 {
            continue;
        }
        seen.insert(key);
        picked.push(line);
        if picked.len() >= max_bullets {
            break;
        }
    }
    let mut tags: Vec<String> = items
        .iter()
        .flat_map(|e| {
            e.get("tags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .map(|t| py_str(Some(&t)))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    tags.sort_by_key(|t| -rank_of(t));
    let focus = tags.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let focus = if focus.is_empty() {
        "delivery".into()
    } else {
        focus
    };
    let role = py::strip(role);
    let lead = if role.is_empty() {
        String::new()
    } else {
        format!("{role}. ")
    };
    let mut intro = format!("{lead}Focused on {focus}.");
    let cleaned = clean(&intro, &repos, &terms, public_metrics)?;
    if !cleaned.is_empty() {
        intro = cleaned;
    }
    let mut text = joined(&intro, &picked);
    while py::len(&text) > LIMIT && !picked.is_empty() {
        picked.pop();
        text = joined(&intro, &picked);
    }
    let mut leaks = canary::find_leaks(&text, &terms);
    if ticket().search(&text) {
        leaks.push("ticket-key".into());
    }
    for (src, icase) in &scrub::PUBLIC_PATTERNS[..4] {
        if Re::new(src, *icase).search(&text) {
            leaks.push(py::take(src, 12));
        }
    }
    if !leaks.is_empty() {
        return Err(Error::new(
            "PrivacyError",
            format!(
                "LinkedIn block blocked: {} item(s) would be published ({})",
                leaks.len(),
                leaks.join(", ")
            ),
        ));
    }
    let out = crate::sub(&["out", "linkedin.md"])?;
    std::fs::write(&out, format!("{text}\n"))?;
    let at = when::iso_seconds(&crate::now()?.with_timezone(&chrono::Utc).fixed_offset());
    let row = careerbrew_setup::value::to_canon(
        &json!({"metric": "linkedin_bullets", "value": picked.len(), "at": at}),
    );
    crate::append_jsonl(&crate::sub(&["metrics.jsonl"])?, &[row])?;
    Ok(Block {
        path: out,
        suspects: allow::public_suspects(&text),
        bullets: picked.len(),
        public_metrics,
        text,
    })
}

/// `path()`.
pub fn path() -> Result<std::path::PathBuf> {
    crate::sub(&["out", "linkedin.md"])
}
