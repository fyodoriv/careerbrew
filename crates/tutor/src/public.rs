//! Public study cards, private explanations, and the generation queue.

use crate::{
    casefold, chars, digest, field, now, read_json, read_text, read_text_abs, read_yaml,
    read_yaml_abs, stamp_at, strings, text, truthy, write_json, write_text_abs, write_yaml, Result,
    TutorError,
};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const SECTIONS: &[&str] = &[
    "## In plain words",
    "## How it works",
    "## Example",
    "## Common mistakes",
    "## In the interview",
];
pub const MIN_WORDS: usize = 150;
pub const MAX_WORDS: usize = 700;
pub const LAPSES_TO_REWRITE: u64 = 3;
const PERSONAL_TOPICS: &[&str] = &[
    "behavioural",
    "behavioral",
    "leadership",
    "tech lead",
    "business/product",
];

fn publish_error(message: impl Into<String>) -> TutorError {
    TutorError::new("PublishError", message)
}

pub fn normalize(question: &str) -> String {
    Regex::new(r"[^a-z0-9]+")
        .expect("public normalize regex")
        .replace_all(&casefold(question), " ")
        .trim()
        .to_owned()
}

pub fn qhash(question: &str) -> String {
    digest(&normalize(question))[..12].to_owned()
}

fn topic_file(public_dir: &Path, topic: &str) -> PathBuf {
    let slug = Regex::new(r"[^a-z0-9]+")
        .expect("public topic regex")
        .replace_all(&casefold(topic), "-")
        .trim_matches('-')
        .to_owned();
    public_dir.join(format!(
        "{}.yaml",
        if slug.is_empty() {
            "general".into()
        } else {
            slug
        }
    ))
}

pub fn prompt_version(repo_root: &Path) -> Result<String> {
    Ok(digest(&std::fs::read_to_string(
        repo_root.join("prompts/card-explainer.md"),
    )?)[..8]
        .to_owned())
}

pub fn load(public_dir: &Path) -> Result<Vec<Value>> {
    let mut files = std::fs::read_dir(public_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect::<Vec<_>>();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let data = read_yaml_abs(&path, json!({}))?;
        out.extend(
            field(&data, "cards")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
    }
    Ok(out)
}

pub fn private_cards(root: &Path) -> Result<Vec<Value>> {
    let data = read_yaml(root, "tutor/private-explanations.yaml", json!({}))?;
    Ok(field(&data, "cards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

fn by_match(public_dir: &Path) -> Result<BTreeMap<String, Value>> {
    let mut out = BTreeMap::new();
    for card in load(public_dir)? {
        for hash in strings(field(&card, "match")) {
            out.insert(hash, card.clone());
        }
    }
    Ok(out)
}

pub fn explanation_for(root: &Path, public_dir: &Path, question: &str) -> Result<Option<Value>> {
    let hash = qhash(question);
    if let Some(card) = private_cards(root)?
        .into_iter()
        .find(|card| strings(field(card, "match")).contains(&hash))
    {
        return Ok(Some(card));
    }
    if let Some(card) = by_match(public_dir)?.get(&hash) {
        return Ok(Some(card.clone()));
    }
    Ok(load(public_dir)?
        .into_iter()
        .find(|card| qhash(&text(field(card, "question"))) == hash))
}

pub fn lint(card: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    for key in ["id", "question", "answer", "explanation", "topic"] {
        if text(field(card, key)).trim().is_empty() {
            problems.push(format!("missing {key}"));
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let question = text(field(card, "question"));
    let answer = text(field(card, "answer"));
    let explanation = text(field(card, "explanation"));
    if question.chars().count() > 220 {
        problems.push("question longer than 220 characters".into());
    }
    if !(20..=600).contains(&answer.chars().count()) {
        problems.push("answer must be 20-600 characters".into());
    }
    let words = explanation.split_whitespace().count();
    if !(MIN_WORDS..=MAX_WORDS).contains(&words) {
        problems.push(format!(
            "explanation has {words} words (bar: {MIN_WORDS}-{MAX_WORDS})"
        ));
    }
    let positions = SECTIONS
        .iter()
        .map(|section| {
            explanation
                .find(section)
                .map(|index| index as i64)
                .unwrap_or(-1)
        })
        .collect::<Vec<_>>();
    if positions.iter().any(|position| *position < 0) {
        let missing = SECTIONS
            .iter()
            .zip(&positions)
            .filter(|(_, position)| **position < 0)
            .map(|(section, _)| section[3..].to_owned())
            .collect::<Vec<_>>()
            .join(", ");
        problems.push(format!("explanation lacks a section: {missing}"));
    } else if !positions.windows(2).all(|pair| pair[0] <= pair[1]) {
        problems.push("explanation sections are out of order".into());
    }
    if !explanation.matches("```").count().is_multiple_of(2) {
        problems.push("unclosed code fence".into());
    }
    let link = Regex::new(r"http://").expect("http regex");
    let xml = Regex::new(r#"xmlns(?::\w{1,3})?=\"$"#).expect("xml namespace regex");
    let host = Regex::new(
        r"^(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|[\w.-]*example\.(?:com|org|net))\b",
    )
    .expect("local HTTP host regex");
    for found in link.find_iter(&explanation) {
        let prefix = &explanation[..found.start()];
        let suffix = &explanation[found.end()..];
        if !xml.is_match(prefix) && !host.is_match(suffix) {
            problems.push("plain http link (use https; http is fine only for localhost or example.com in code)".into());
            break;
        }
    }
    problems
}

fn installed_plugin_terms(root: &Path) -> Result<Vec<String>> {
    let lock = read_json(root, "plugins.lock.json", json!({}))?;
    let mut out = Vec::new();
    for name in field(&lock, "plugins")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|plugins| plugins.keys())
    {
        let manifest = read_yaml(
            root,
            &format!("plugins/{name}/careerbrew-plugin.yaml"),
            json!({}),
        )?;
        if text(field(&manifest, "name")) != *name || field(&manifest, "version").is_none() {
            return Err(TutorError::new(
                "PluginError",
                "installed plugin manifest is invalid",
            ));
        }
        let relative = text(field(&manifest, "sensitive_terms"));
        if relative.is_empty() {
            continue;
        }
        let relative_path = Path::new(&relative);
        if relative_path.is_absolute()
            || relative_path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(TutorError::new(
                "PluginError",
                "installed plugin sensitive terms path is invalid",
            ));
        }
        let text = read_text_abs(&root.join("plugins").join(name).join(relative_path))?;
        out.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty() && !line.starts_with('#') && line.chars().count() >= 3
                })
                .map(str::to_owned),
        );
    }
    Ok(out)
}

fn personal_terms(root: &Path, plugin_terms: &[String]) -> Result<Vec<String>> {
    let profile = read_yaml(root, "profile.yaml", json!({}))?;
    let mut base = read_text(root, "sensitive-terms.txt")?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && line.chars().count() >= 3)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for key in ["name", "employer", "email"] {
        let value = text(field(&profile, key));
        if value.chars().count() >= 3 && !matches!(value.as_str(), "Your Name" | "Your Employer") {
            base.push(value);
        }
    }
    base.extend(
        strings(field(&profile, "sensitive"))
            .into_iter()
            .filter(|value| value.chars().count() >= 3),
    );
    base.extend(installed_plugin_terms(root)?);
    base.extend_from_slice(plugin_terms);
    let mut unique = Vec::new();
    for term in base {
        if !unique.contains(&term) {
            unique.push(term);
        }
    }
    let mut terms = unique;
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    // public._personal_terms appends these two after privacy.sensitive_terms.
    for key in ["name", "employer"] {
        let value = text(field(&profile, key)).trim().to_owned();
        if value.chars().count() >= 3 && !matches!(value.as_str(), "Your Name" | "Your Employer") {
            terms.push(value);
        }
    }
    Ok(terms)
}

pub fn personal_hits(root: &Path, card: &Value) -> Result<Vec<String>> {
    personal_hits_with_terms(root, card, &[])
}

/// Provider seam for installed-plugin terms; core and profile terms are added here.
pub fn personal_hits_with_terms(
    root: &Path,
    card: &Value,
    plugin_terms: &[String],
) -> Result<Vec<String>> {
    let terms = personal_terms(root, plugin_terms)?;
    let text = ["question", "answer", "explanation", "tags"]
        .iter()
        .map(|key| match field(card, key) {
            Some(Value::Array(_) | Value::Object(_)) => {
                serde_json::to_string(&card[key]).unwrap_or_default()
            }
            _ => text(field(card, key)),
        })
        .collect::<Vec<_>>()
        .join(" ");
    Ok(careerbrew_policy::find_leaks(&text, &terms))
}

pub fn flags(root: &Path) -> Result<Value> {
    read_json(root, "tutor/explanation-flags.json", json!({}))
}

pub fn flag(root: &Path, public_dir: &Path, card_id: &str, note: &str) -> Result<Value> {
    let known = load(public_dir)?
        .into_iter()
        .chain(private_cards(root)?)
        .map(|card| text(field(&card, "id")))
        .collect::<BTreeSet<_>>();
    if !known.contains(card_id) {
        return Err(TutorError::new(
            "KeyError",
            format!("unknown explanation {card_id}"),
        ));
    }
    let cleaned = note.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = chars(&cleaned, 500);
    let entry = json!({"note": if cleaned.is_empty() { "unclear or wrong".into() } else { cleaned }, "at": stamp_at(now())});
    let mut all = flags(root)?;
    all[card_id] = entry.clone();
    write_json(root, "tutor/explanation-flags.json", &all)?;
    Ok(entry)
}

pub fn skipped(root: &Path) -> Result<Value> {
    read_json(root, "tutor/public-skipped.json", json!({}))
}

fn lapses(root: &Path) -> Result<BTreeMap<String, u64>> {
    let mut out = BTreeMap::new();
    for review in crate::cards::reviews(root)? {
        if text(field(&review, "rating")) == "again" {
            let id = text(field(&review, "card"));
            *out.entry(id).or_default() += 1;
        }
    }
    Ok(out)
}

pub fn explain_queue(
    root: &Path,
    public_dir: &Path,
    repo_root: &Path,
    limit: Option<usize>,
) -> Result<Vec<Value>> {
    let public = load(public_dir)?;
    let mut covered = by_match(public_dir)?;
    for card in &public {
        covered.insert(qhash(&text(field(card, "question"))), card.clone());
    }
    let skip = skipped(root)?;
    let version = prompt_version(repo_root)?;
    let lapses = lapses(root)?;
    let private = private_cards(root)?;
    let private_done = private
        .iter()
        .flat_map(|card| strings(field(card, "match")))
        .collect::<BTreeSet<_>>();
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    for card in crate::cards::all_cards(root, public_dir)?.values() {
        if text(field(card, "deck")) == "public" {
            continue;
        }
        let question = text(field(card, "question"));
        let hash = qhash(&question);
        if !seen.insert(hash.clone()) {
            continue;
        }
        if skip.get(&hash).is_some() {
            if !private_done.contains(&hash) {
                items.push(json!({
                    "source_id": hash,
                    "question": question,
                    "topic": field(card, "topic").cloned().unwrap_or_else(|| json!("general")),
                    "hint": "",
                    "reason": "personal: explain privately",
                    "private": true,
                }));
            }
            continue;
        }
        match covered.get(&hash) {
            None => {
                let personal =
                    PERSONAL_TOPICS.contains(&casefold(&text(field(card, "topic"))).as_str());
                let hint = if personal {
                    String::new()
                } else {
                    text(field(card, "answer"))
                };
                items.push(json!({
                    "source_id": hash,
                    "question": question,
                    "topic": field(card, "topic").cloned().unwrap_or_else(|| json!("general")),
                    "hint": chars(&hint, 1500),
                    "reason": "new",
                }));
            }
            Some(pub_card)
                if lapses
                    .get(&text(field(card, "id")))
                    .copied()
                    .unwrap_or_default()
                    >= LAPSES_TO_REWRITE =>
            {
                items.push(json!({
                    "source_id": hash,
                    "question": text(field(pub_card, "question")),
                    "topic": text(field(pub_card, "topic")),
                    "hint": "",
                    "reason": "learners keep failing it: make it clearer",
                    "rewrite": text(field(pub_card, "id")),
                }));
            }
            _ => {}
        }
    }
    let flagged = flags(root)?;
    for card in public.iter().chain(&private) {
        let id = text(field(card, "id"));
        if flagged.get(&id).is_some()
            && !items.iter().any(|item| text(field(item, "rewrite")) == id)
        {
            let is_private = id.starts_with("me-");
            let matches = strings(field(card, "match"));
            let source_id = matches
                .first()
                .cloned()
                .unwrap_or_else(|| qhash(&text(field(card, "question"))));
            let mut item = json!({
                "source_id": source_id,
                "question": text(field(card, "question")),
                "topic": text(field(card, "topic")),
                "hint": "",
                "reason": format!("a reader reported: {}", text(field(&flagged[&id], "note"))),
                "rewrite": id,
            });
            if is_private {
                item["private"] = json!(true);
            }
            items.push(item);
        }
    }
    for card in &public {
        let id = text(field(card, "id"));
        let mut why = if text(field(card, "prompt")) == version {
            String::new()
        } else {
            "written with an older prompt".into()
        };
        if why.is_empty() {
            let problems = lint(card);
            if !problems.is_empty() {
                why = format!("fails lint: {}", problems.join("; "));
            }
        }
        if !why.is_empty() && !items.iter().any(|item| text(field(item, "rewrite")) == id) {
            let matches = strings(field(card, "match"));
            let source_id = matches
                .first()
                .cloned()
                .unwrap_or_else(|| qhash(&text(field(card, "question"))));
            items.push(json!({
                "source_id": source_id,
                "question": text(field(card, "question")),
                "topic": text(field(card, "topic")),
                "hint": "",
                "reason": why,
                "rewrite": id,
            }));
        }
    }
    items.sort_by(|left, right| {
        let order = |item: &Value| {
            if text(field(item, "reason")) == "new" {
                2
            } else {
                0
            }
        };
        order(left)
            .cmp(&order(right))
            .then(text(field(left, "topic")).cmp(&text(field(right, "topic"))))
            .then(text(field(left, "question")).cmp(&text(field(right, "question"))))
    });
    Ok(items
        .into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .collect())
}

pub fn publish(
    root: &Path,
    public_dir: &Path,
    repo_root: &Path,
    rows: &[Value],
    dry_run: bool,
) -> Result<Value> {
    let version = prompt_version(repo_root)?;
    let public = load(public_dir)?
        .into_iter()
        .map(|card| (text(field(&card, "id")), card))
        .collect::<BTreeMap<_, _>>();
    let by_q = public
        .values()
        .map(|card| {
            (
                qhash(&text(field(card, "question"))),
                text(field(card, "id")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut skip = skipped(root)?;
    let mut new = BTreeMap::<String, Value>::new();
    let mine = private_cards(root)?
        .into_iter()
        .map(|card| (text(field(&card, "id")), card))
        .collect::<BTreeMap<_, _>>();
    let mut new_private = BTreeMap::<String, Value>::new();
    let mut problems = Vec::new();
    let mut skipped_count = 0;
    for (index, row) in rows.iter().enumerate() {
        let source = text(field(row, "source_id"));
        if truthy(field(row, "skip")) {
            if !source.is_empty() {
                skip[&source] = json!(if text(field(row, "reason")).is_empty() {
                    "personal".into()
                } else {
                    text(field(row, "reason"))
                });
                skipped_count += 1;
            }
            continue;
        }
        let question = text(field(row, "question"));
        let hash = qhash(&question);
        let rewrite = text(field(row, "rewrite"));
        let id = if !rewrite.is_empty() {
            rewrite.clone()
        } else {
            by_q.get(&hash)
                .cloned()
                .unwrap_or_else(|| format!("kb-{hash}"))
        };
        let mut matches = strings(
            public
                .get(&id)
                .map(|card| field(card, "match"))
                .unwrap_or(None),
        );
        matches.extend(strings(
            new.get(&id)
                .map(|card| field(card, "match"))
                .unwrap_or(None),
        ));
        if !source.is_empty() {
            matches.push(source.clone());
        }
        matches.sort();
        matches.dedup();
        let topic = text(field(row, "topic"));
        let topic = if topic.is_empty() {
            "general".to_owned()
        } else {
            topic
        };
        let mut card = json!({
            "id": id,
            "question": question.trim(),
            "answer": text(field(row, "answer")).trim(),
            "topic": casefold(&topic),
            "tags": strings(field(row, "tags")).iter().map(|tag| casefold(tag)).take(5).collect::<Vec<_>>(),
            "explanation": format!("{}\n", text(field(row, "explanation")).trim()),
            "match": matches,
            "prompt": version,
        });
        if truthy(field(row, "private")) {
            let private_id = if !rewrite.is_empty() {
                rewrite
            } else {
                format!(
                    "me-{}",
                    if source.is_empty() {
                        qhash(&question)
                    } else {
                        source.clone()
                    }
                )
            };
            card["id"] = json!(private_id);
            card["match"] = json!(if source.is_empty() {
                Vec::<String>::new()
            } else {
                vec![source]
            });
            let bad = lint(&card);
            if bad.is_empty() {
                new_private.insert(text(field(&card, "id")), card);
            } else {
                problems.push(format!("card {}: {}", index + 1, bad.join("; ")));
            }
            continue;
        }
        let mut bad = lint(&card);
        let hits = personal_hits(root, &card)?;
        if !hits.is_empty() {
            bad.push(format!("personal data ({})", hits.join(", ")));
        }
        if bad.is_empty() {
            new.insert(text(field(&card, "id")), card);
        } else {
            problems.push(format!("card {}: {}", index + 1, bad.join("; ")));
        }
    }
    if !problems.is_empty() {
        return Err(publish_error(format!(
            "{} card(s) not published; fix them and publish again:\n{}",
            problems.len(),
            problems.join("\n")
        )));
    }
    let merged = public
        .iter()
        .chain(new.iter())
        .map(|(id, card)| (id.clone(), card.clone()))
        .collect::<BTreeMap<_, _>>();
    let result = json!({"published": new.len(), "skipped": skipped_count, "total": merged.len(), "private": new_private.len()});
    if dry_run {
        return Ok(result);
    }
    let done = new
        .keys()
        .chain(new_private.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let flagged = flags(root)?;
    if done.iter().any(|id| flagged.get(id).is_some()) {
        let remaining = flagged
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(id, _)| !done.contains(*id))
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect::<Map<_, _>>();
        write_json(
            root,
            "tutor/explanation-flags.json",
            &Value::Object(remaining),
        )?;
    }
    if !new_private.is_empty() {
        let mut by_id = mine.clone();
        by_id.extend(
            new_private
                .iter()
                .map(|(id, card)| (id.clone(), card.clone())),
        );
        let mut cards = by_id.into_values().collect::<Vec<_>>();
        cards.sort_by_key(|card| casefold(&text(field(card, "question"))));
        write_yaml(
            root,
            "tutor/private-explanations.yaml",
            &json!({"cards": cards}),
        )?;
    }
    let mut files = BTreeMap::<PathBuf, Vec<Value>>::new();
    for card in merged.values() {
        files
            .entry(topic_file(public_dir, &text(field(card, "topic"))))
            .or_default()
            .push(card.clone());
    }
    std::fs::create_dir_all(public_dir)?;
    for path in std::fs::read_dir(public_dir)?
        .flatten()
        .map(|entry| entry.path())
    {
        if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
            && !files.contains_key(&path)
        {
            std::fs::remove_file(path)?;
        }
    }
    for (path, mut cards) in files {
        cards.sort_by_key(|card| casefold(&text(field(card, "question"))));
        let body = careerbrew_vault::json_yaml::yaml_write(&careerbrew_setup::value::to_canon(
            &json!({"cards": cards}),
        ))
        .map_err(|_| publish_error("cannot encode public cards"))?;
        write_text_abs(
            &path,
            &format!("# Public study cards. Generated by the card-explainer skill; edit through `careerbrew cards publish`.\n{body}"),
        )?;
    }
    write_json(root, "tutor/public-skipped.json", &skip)?;
    Ok(result)
}

pub fn check(public_dir: &Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for card in load(public_dir)? {
        let id = text(field(&card, "id"));
        if !seen.insert(id.clone()) {
            problems.push(format!("{id}: duplicate id"));
        }
        problems.extend(
            lint(&card)
                .into_iter()
                .map(|problem| format!("{id}: {problem}")),
        );
    }
    Ok(problems)
}

fn page(card: &Value, section: &str) -> Value {
    json!({
        "id": text(field(card, "id")),
        "title": text(field(card, "question")),
        "section": section,
        "markdown": format!("**Short answer.** {}\n\n{}", text(field(card, "answer")), text(field(card, "explanation"))),
    })
}

pub fn library_entries(root: &Path, public_dir: &Path) -> Result<Vec<Value>> {
    let mut public = load(public_dir)?;
    public.sort_by(|left, right| {
        text(field(left, "topic"))
            .cmp(&text(field(right, "topic")))
            .then(
                casefold(&text(field(left, "question")))
                    .cmp(&casefold(&text(field(right, "question")))),
            )
    });
    let mut pages = public
        .iter()
        .map(|card| page(card, &format!("knowledge:{}", text(field(card, "topic")))))
        .collect::<Vec<_>>();
    pages.extend(
        private_cards(root)?
            .iter()
            .map(|card| page(card, "my-explanations")),
    );
    Ok(pages)
}
