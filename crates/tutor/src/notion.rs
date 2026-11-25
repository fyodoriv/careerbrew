//! Validated, idempotent Notion bundle imports with explicit write boundaries.

use crate::{
    casefold, chars, emit, field, now, read_json, read_json_abs, read_jsonl, read_text, read_yaml,
    stamp_at, text, truthy, write_json, write_jsonl, write_text, write_yaml, Result, TutorError,
};
use careerbrew_vault::canon::Canon;
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const KINDS: &[&str] = &[
    "pages",
    "jobs",
    "flashcards",
    "plan",
    "skills",
    "stories",
    "past_job",
];
pub const NO_ANSWER: &str =
    "No answer written in Notion yet. Say your answer out loud, then write it on the card.";

fn required(kind: &str) -> &'static [&'static str] {
    match kind {
        "pages" | "jobs" | "stories" | "past_job" => &["id", "title"],
        "plan" => &["id", "goal"],
        "skills" => &["id", "name"],
        "flashcards" => &["id"],
        _ => &[],
    }
}

fn stage_map(stage: &str) -> Option<&'static str> {
    match stage {
        "Resume stage" | "No: Resume" => Some("applied"),
        "HR stage" | "Quiz stage" => Some("screen"),
        "No: HR" | "No: Quiz" | "No: Onsite" | "No: Negotiation" => Some("rejected"),
        "Onsite stage" => Some("onsite"),
        "Negotiation stage" | "Hired" => Some("offer"),
        _ => None,
    }
}

fn known_stage(stage: &str) -> bool {
    stage == "Not started" || stage_map(stage).is_some()
}

pub fn load(folder: &Path) -> Result<BTreeMap<String, Vec<Value>>> {
    let manifest = read_json_abs(&folder.join("manifest.json"), Value::Null)?;
    let Some(manifest) = manifest.as_object() else {
        return Err(TutorError::import(format!(
            "no manifest.json in {}/ (the notion-import skill writes it)",
            folder.file_name().unwrap_or_default().to_string_lossy()
        )));
    };
    let counts = manifest
        .get("counts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let missing = KINDS
        .iter()
        .filter(|kind| !counts.contains_key(**kind))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(TutorError::import(format!(
            "manifest counts must list every kind (missing: {}); write 0 for a kind with no rows",
            missing.join(", ")
        )));
    }
    let mut data = BTreeMap::new();
    for kind in KINDS {
        let path = folder.join(format!("{kind}.json"));
        let count = &counts[*kind];
        if truthy(Some(count)) && !path.exists() {
            return Err(TutorError::import(format!(
                "{kind}.json is missing but the manifest counts {} rows",
                text(Some(count))
            )));
        }
        let rows = read_json_abs(&path, json!([]))?;
        let Some(rows) = rows.as_array() else {
            return Err(TutorError::import(format!("{kind}.json must be a list")));
        };
        for (index, row) in rows.iter().enumerate() {
            let lacking = required(kind)
                .iter()
                .filter(|key| row.get(**key).is_none())
                .copied()
                .collect::<Vec<_>>();
            if !lacking.is_empty() {
                return Err(TutorError::import(format!(
                    "{kind}.json row {} lacks {} (rerun the notion-import skill)",
                    index + 1,
                    lacking.join(", ")
                )));
            }
        }
        let expected = count.as_u64().map(|number| number as usize);
        if expected != Some(rows.len()) {
            return Err(TutorError::import(format!(
                "{kind}: manifest says {}, file has {} (the fetch is incomplete; rerun the notion-import skill)",
                text(Some(count)),
                rows.len()
            )));
        }
        data.insert((*kind).to_owned(), rows.clone());
    }
    Ok(data)
}

fn years_to_rating(value: &Value) -> u64 {
    let years = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or(0.0);
    if years < 1.0 {
        1
    } else if years < 2.0 {
        2
    } else if years < 4.0 {
        3
    } else if years < 7.0 {
        4
    } else {
        5
    }
}

fn iso(value: &Value) -> String {
    let raw = text(Some(value));
    if raw.is_empty() {
        return String::new();
    }
    let normalized = raw.replace('Z', "+00:00");
    let parsed = DateTime::parse_from_rfc3339(&normalized)
        .ok()
        .map(|at| at.with_timezone(&Utc))
        .or_else(|| {
            NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|naive| naive.and_utc())
        })
        .or_else(|| {
            NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
                .ok()
                .map(|date| date.and_hms_opt(0, 0, 0).expect("midnight").and_utc())
        });
    parsed
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false))
        .unwrap_or_default()
}

pub fn plain(value: &str) -> String {
    let markers = Regex::new(r"(\*\*|__|`)").expect("Notion marker regex");
    let escaped = Regex::new(r"\\([\\`*_{}\[\]()#+\-.!^~|>])").expect("Notion escape regex");
    escaped
        .replace_all(&markers.replace_all(value, ""), "$1")
        .trim()
        .to_owned()
}

fn slug(value: &str) -> String {
    let slug = Regex::new(r"[^a-z0-9]+")
        .expect("Notion slug regex")
        .replace_all(&casefold(value), "-")
        .trim_matches('-')
        .to_owned();
    let slug = chars(&slug, 60);
    if slug.is_empty() {
        "page".into()
    } else {
        slug
    }
}

fn company(title: &str) -> (String, String) {
    let (company, role) = title.split_once(':').unwrap_or((title, ""));
    (company.trim().to_owned(), role.trim().to_owned())
}

fn library_dir() -> &'static str {
    "library/notion"
}

fn board_path() -> &'static str {
    "jobs/notion-board.json"
}

fn applications_path() -> &'static str {
    "jobs/applications.jsonl"
}

fn stories_path() -> &'static str {
    "prep/stories.yaml"
}

fn evidence_path() -> &'static str {
    "work/evidence.jsonl"
}

fn write_library(root: &Path, entries: &[Value]) -> Result<()> {
    let folder = root.join(library_dir());
    std::fs::create_dir_all(&folder)?;
    for path in std::fs::read_dir(&folder)?
        .flatten()
        .map(|entry| entry.path())
    {
        if path.extension().is_some_and(|extension| extension == "md") {
            std::fs::remove_file(path)?;
        }
    }
    let mut index = Vec::new();
    for entry in entries {
        let name = format!(
            "{}-{}.md",
            slug(&text(field(entry, "title"))),
            chars(&text(field(entry, "id")), 8)
        );
        write_text(
            root,
            &format!("library/notion/{name}"),
            &text(field(entry, "markdown")),
        )?;
        let mut row = Map::new();
        for key in [
            "id",
            "title",
            "section",
            "path",
            "updated",
            "status",
            "truncated",
        ] {
            row.insert(
                key.into(),
                field(entry, key).cloned().unwrap_or(Value::Null),
            );
        }
        row.insert("file".into(), json!(name));
        index.push(Value::Object(row));
    }
    write_json(root, "library/notion/index.json", &Value::Array(index))
}

fn role_guard() -> Result<()> {
    let role = careerbrew_setup::machine::role()?;
    if let Some(message) = careerbrew_setup::machine::guard_decision("import", &role) {
        return Err(TutorError::new("RoleError", message));
    }
    Ok(())
}

/// Run the import. `stop_before_deck` exists only for the synthetic fault control.
pub fn run(root: &Path, folder: &Path, repo_root: &Path, stop_before_deck: bool) -> Result<Value> {
    role_guard()?;
    let data = load(folder)?;
    let mut library = data["pages"]
        .iter()
        .map(|page| {
            let mut page = page.clone();
            if text(field(&page, "section")).is_empty() {
                page["section"] = json!("career");
            }
            page
        })
        .collect::<Vec<_>>();
    library.extend(data["plan"].iter().map(|page| {
        json!({"id": text(field(page, "id")), "title": text(field(page, "goal")), "section": "plan", "status": field(page, "status"), "markdown": text(field(page, "markdown"))})
    }));
    library.extend(data["past_job"].iter().map(|page| {
        let mut page = page.clone();
        page["section"] = json!("past-job");
        page
    }));
    if !data["skills"].is_empty() {
        let lines = data["skills"]
            .iter()
            .map(|skill| {
                format!(
                    "- **{}**: {} years. {}",
                    text(field(skill, "name")),
                    field(skill, "years")
                        .filter(|value| truthy(Some(value)))
                        .map(|value| text(Some(value)))
                        .unwrap_or_else(|| "0".into()),
                    text(field(skill, "highlight"))
                )
                .trim_end()
                .to_owned()
            })
            .collect::<Vec<_>>();
        library.push(json!({"id": "skills", "title": "My skills", "section": "career", "markdown": lines.join("\n")}));
    }
    library.extend(data["stories"].iter().map(|story| {
        json!({"id": text(field(story, "id")), "title": text(field(story, "title")), "section": "story", "markdown": text(field(story, "markdown"))})
    }));
    library.extend(data["jobs"].iter().filter(|job| !text(field(job, "markdown")).trim().is_empty()).map(|job| {
        json!({"id": text(field(job, "id")), "title": text(field(job, "title")), "section": "job", "status": field(job, "stage"), "markdown": text(field(job, "markdown"))})
    }));
    write_library(root, &library)?;

    let board = data["jobs"]
        .iter()
        .map(|job| {
            let (company, role) = company(&text(field(job, "title")));
            let mut row = json!({"id": text(field(job, "id")), "company": company, "role": role});
            for key in [
                "stage",
                "want",
                "salary",
                "location",
                "remote",
                "url",
                "reason_no",
                "last_my_contact",
                "last_their_contact",
                "next_follow_up",
                "difficulty",
                "created",
                "markdown",
            ] {
                row[key] = field(job, key).cloned().unwrap_or(Value::Null);
            }
            row
        })
        .collect::<Vec<_>>();
    write_json(root, board_path(), &Value::Array(board.clone()))?;

    let keep = read_jsonl(root, applications_path())?
        .into_iter()
        .filter(|row| text(field(row, "origin")) != "notion")
        .collect::<Vec<_>>();
    let unmapped = board
        .iter()
        .map(|job| text(field(job, "stage")))
        .filter(|stage| !known_stage(stage))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut add = Vec::new();
    for job in &board {
        let stage = text(field(job, "stage"));
        if let Some(mapped) = stage_map(&stage) {
            let at = ["last_their_contact", "last_my_contact", "created"]
                .iter()
                .map(|key| field(job, key).cloned().unwrap_or(Value::Null))
                .find(|value| truthy(Some(value)))
                .unwrap_or(Value::Null);
            add.push(json!({
                "company": text(field(job, "company")),
                "tier": "history",
                "stage": mapped,
                "at": iso(&at),
                "origin": "notion",
                "notion_id": text(field(job, "id")),
            }));
        }
    }
    write_jsonl(
        root,
        applications_path(),
        &keep.iter().chain(add.iter()).cloned().collect::<Vec<_>>(),
    )?;

    if stop_before_deck {
        return Err(TutorError::new("RuntimeError", "synthetic import stop"));
    }
    let blank = data["flashcards"]
        .iter()
        .filter(|card| text(field(card, "question")).trim().is_empty())
        .count();
    let deck = data["flashcards"]
        .iter()
        .map(|card| {
            let answer = text(field(card, "answer"));
            let markdown = text(field(card, "markdown"));
            json!({
                "id": format!("notion-{}", text(field(card, "id"))),
                "question": plain(&text(field(card, "question"))),
                "answer": if !answer.is_empty() { answer } else if !markdown.is_empty() { markdown } else { NO_ANSWER.into() },
                "tags": crate::strings(field(card, "tags")),
                "source": "notion",
            })
        })
        .collect::<Vec<_>>();
    let card_count = crate::cards::write_deck(root, "notion", &deck)?;

    let profile_path = root.join("profile.yaml");
    if !profile_path.exists() {
        let example = std::fs::read_to_string(repo_root.join("profile/examples/profile.yaml"))?;
        let example = if careerbrew_setup::machine::role()? == "corporate" {
            careerbrew_setup::machine::work_profile(&example)
        } else {
            example
        };
        write_text(root, "profile.yaml", &example)?;
    }
    let profile = read_yaml(root, "profile.yaml", json!({}))?;
    let mut skills = field(&profile, "skills")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let have = skills
        .keys()
        .map(|skill| casefold(skill))
        .collect::<BTreeSet<_>>();
    let added_skills = data["skills"]
        .iter()
        .filter(|skill| !have.contains(&casefold(&text(field(skill, "name")))))
        .collect::<Vec<_>>();
    for skill in &added_skills {
        skills.insert(
            casefold(&text(field(skill, "name"))),
            json!(years_to_rating(
                field(skill, "years").unwrap_or(&Value::Null)
            )),
        );
    }
    if !added_skills.is_empty() {
        let mut updated = profile;
        updated["skills"] = Value::Object(skills);
        write_yaml(root, "profile.yaml", &updated)?;
    }

    let old_stories = read_yaml(root, stories_path(), json!({}))?;
    let mut stories = field(&old_stories, "stories")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|story| text(field(story, "origin")) != "notion")
        .collect::<Vec<_>>();
    for story in &data["stories"] {
        stories.push(json!({
            "id": format!("notion-{}", text(field(story, "id"))),
            "title": text(field(story, "title")),
            "competencies": field(story, "tags").cloned().filter(|value| truthy(Some(value))).unwrap_or_else(|| json!(["behavioral"])),
            "situation": text(field(story, "markdown")),
            "action": "",
            "result": "",
            "added": now().date_naive().to_string(),
            "origin": "notion",
        }));
    }
    write_yaml(root, stories_path(), &json!({"stories": stories}))?;

    let evidence_keep = read_jsonl(root, evidence_path())?
        .into_iter()
        .filter(|row| text(field(row, "origin")) != "notion")
        .collect::<Vec<_>>();
    let evidence_add = data["past_job"]
        .iter()
        .map(|page| {
            let at = iso(field(page, "updated").unwrap_or(&Value::Null));
            json!({
                "id": format!("notion:{}", text(field(page, "id"))),
                "kind": "note",
                "title": format!("1:1 notes with {} ({})", if text(field(page, "person")).is_empty() { "a report".into() } else { text(field(page, "person")) }, if text(field(page, "employer")).is_empty() { "past job".into() } else { text(field(page, "employer")) }),
                "impact": "",
                "at": if at.is_empty() { stamp_at(now()) } else { at },
                "tags": ["leadership", "mentoring"],
                "origin": "notion",
            })
        })
        .collect::<Vec<_>>();
    write_jsonl(
        root,
        evidence_path(),
        &evidence_keep
            .iter()
            .chain(evidence_add.iter())
            .cloned()
            .collect::<Vec<_>>(),
    )?;

    let existing = read_text(root, "sensitive-terms.txt")?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let existing_set = existing.iter().cloned().collect::<BTreeSet<_>>();
    let new_terms = data["past_job"]
        .iter()
        .flat_map(|page| [text(field(page, "person")), text(field(page, "employer"))])
        .filter(|value| !value.is_empty() && !existing_set.contains(value))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if !new_terms.is_empty() {
        write_text(
            root,
            "sensitive-terms.txt",
            &format!(
                "{}\n",
                existing
                    .iter()
                    .chain(new_terms.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )?;
    }

    let mut targets = std::fs::read_dir(folder)?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    targets.sort();
    careerbrew_vault::bulk::seal_files(&crate::context()?.store(), &targets)?;
    let counts = KINDS
        .iter()
        .map(|kind| ((*kind).to_owned(), json!(data[*kind].len())))
        .collect::<Map<_, _>>();
    emit(
        root,
        "notion.import",
        vec![
            ("status", Canon::Str("ok".into())),
            (
                "items",
                Canon::Int(
                    counts
                        .values()
                        .filter_map(Value::as_i64)
                        .sum::<i64>()
                        .into(),
                ),
            ),
        ],
    )?;
    Ok(json!({
        "counts": counts,
        "unmapped_stages": unmapped,
        "skipped_empty": blank,
        "cards": card_count,
        "applications": add.len(),
        "skills_added": added_skills.len(),
        "leak_terms_added": new_terms.len(),
    }))
}

pub fn library(root: &Path) -> Result<Vec<Value>> {
    Ok(read_json(root, "library/notion/index.json", json!([]))?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

pub fn page(root: &Path, page_id: &str) -> Result<Option<Value>> {
    for entry in library(root)? {
        if text(field(&entry, "id")) == page_id {
            let mut entry = entry;
            entry["markdown"] = json!(read_text(
                root,
                &format!("library/notion/{}", text(field(&entry, "file")))
            )?);
            return Ok(Some(entry));
        }
    }
    Ok(None)
}

pub fn jobs_board(root: &Path) -> Result<Vec<Value>> {
    Ok(read_json(root, board_path(), json!([]))?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

pub fn check(root: &Path, folder: &Path, public_dir: &Path) -> Result<Value> {
    let data = load(folder)?;
    let lib_ids = library(root)?
        .iter()
        .map(|entry| text(field(entry, "id")))
        .collect::<BTreeSet<_>>();
    let deck = crate::cards::all_cards(root, public_dir)?
        .values()
        .filter(|card| text(field(card, "deck")) == "notion")
        .map(|card| text(field(card, "id")))
        .collect::<BTreeSet<_>>();
    let board = jobs_board(root)?
        .iter()
        .map(|job| text(field(job, "id")))
        .collect::<BTreeSet<_>>();
    let profile = read_yaml(root, "profile.yaml", json!({}))?;
    let skills = field(&profile, "skills")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let profile_skills = skills
        .keys()
        .map(|skill| casefold(skill))
        .collect::<BTreeSet<_>>();
    let story_data = read_yaml(root, stories_path(), json!({}))?;
    let story_ids = field(&story_data, "stories")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter(|story| text(field(story, "origin")) == "notion")
        .map(|story| text(field(story, "id")))
        .collect::<BTreeSet<_>>();
    let mut present = BTreeMap::<&str, usize>::new();
    present.insert(
        "pages",
        data["pages"]
            .iter()
            .filter(|page| lib_ids.contains(&text(field(page, "id"))))
            .count(),
    );
    present.insert(
        "plan",
        data["plan"]
            .iter()
            .filter(|page| lib_ids.contains(&text(field(page, "id"))))
            .count(),
    );
    present.insert(
        "past_job",
        data["past_job"]
            .iter()
            .filter(|page| lib_ids.contains(&text(field(page, "id"))))
            .count(),
    );
    present.insert(
        "jobs",
        data["jobs"]
            .iter()
            .filter(|job| board.contains(&text(field(job, "id"))))
            .count(),
    );
    present.insert(
        "flashcards",
        data["flashcards"]
            .iter()
            .filter(|card| {
                !text(field(card, "question")).trim().is_empty()
                    && deck.contains(&format!("notion-{}", text(field(card, "id"))))
            })
            .count(),
    );
    present.insert(
        "skills",
        data["skills"]
            .iter()
            .filter(|skill| profile_skills.contains(&casefold(&text(field(skill, "name")))))
            .count(),
    );
    present.insert(
        "stories",
        data["stories"]
            .iter()
            .filter(|story| story_ids.contains(&format!("notion-{}", text(field(story, "id")))))
            .count(),
    );
    let blank = data["flashcards"]
        .iter()
        .filter(|card| text(field(card, "question")).trim().is_empty())
        .count();
    let expected = KINDS
        .iter()
        .map(|kind| {
            let mut count = data[*kind].len();
            if *kind == "flashcards" {
                count -= blank;
            }
            (*kind, count)
        })
        .collect::<BTreeMap<_, _>>();
    let imported = present.values().sum::<usize>();
    let expected_total = expected.values().sum::<usize>();
    let by_kind = KINDS
        .iter()
        .map(|kind| {
            (
                (*kind).to_owned(),
                json!(format!("{}/{}", present[kind], expected[kind])),
            )
        })
        .collect::<Map<_, _>>();
    Ok(json!({
        "ok": imported == expected_total,
        "imported": imported,
        "expected": expected_total,
        "skipped_empty": blank,
        "by_kind": by_kind,
    }))
}
