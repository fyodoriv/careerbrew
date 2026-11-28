//! Native CLI handlers owned by `rust-12` (see `steps/mod.rs`): `learn list`,
//! `learn note`, and the text forms of `learn status` and `learn next`,
//! through the `careerbrew-learn` crate.
//!
//! Python's `cmd_learn_list` and `cmd_learn_note` (`src/careerbrew/cli.py`)
//! are the reference. Each handler checks every input it would read first
//! and refuses (Python then runs the command) on any form whose Python
//! result it cannot repeat exactly: a malformed record, an unknown clock
//! form, a missing key, a symlink on the note path, or any read error.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_learn::project as lp;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust12.rs`.
pub const COMMANDS: &[&str] = &["learn.list", "learn.note", "learn.status", "learn.next"];

const BLOOM: &[&str] = &[
    "remember",
    "understand",
    "apply",
    "analyze",
    "evaluate",
    "create",
];
const CONFIDENCE: &[&str] = &["low", "medium", "high"];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // Python prints both through `privacy.redact`.
    if ctx.redact_inputs_present() || !ctx.root.is_absolute() {
        return None;
    }
    match name {
        "learn.list" => list(ctx),
        "learn.note" => note(args, ctx),
        "learn.status" => status(args, ctx),
        "learn.next" => next(args, ctx),
        _ => None,
    }
}

fn done(stdout: String) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    })
}

/// `paths.data_root()`: Python makes the root and sets its mode on every
/// call; `paths.sub` makes each parent folder.
fn touch_dirs(root: &Path, dirs: &[PathBuf]) -> Option<()> {
    std::fs::create_dir_all(root).ok()?;
    let _ = std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700));
    for dir in dirs {
        std::fs::create_dir_all(dir).ok()?;
    }
    Some(())
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn key_present() -> bool {
    careerbrew_vault::transfer::Context::from_env().is_ok_and(|ctx| ctx.key_file.is_file())
}

/// `calendar.now()` reads `CAREERBREW_NOW` with `fromisoformat`, the learn
/// crate with RFC 3339; serve only the form both read as the same instant
/// at whole seconds (`YYYY-MM-DDTHH:MM:SS` then `Z` or `+HH:MM`).
fn clock_ok() -> bool {
    let Ok(value) = std::env::var("CAREERBREW_NOW") else {
        return true;
    };
    if value.is_empty() {
        return true;
    }
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let base = bytes.len() >= 19
        && digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19);
    let zone = match &bytes[19.min(bytes.len())..] {
        b"Z" => true,
        [sign, h1, h2, b':', m1, m2] => {
            matches!(sign, b'+' | b'-') && [h1, h2, m1, m2].iter().all(|b| b.is_ascii_digit())
        }
        _ => false,
    };
    base && zone && careerbrew_learn::parse_at(&value).is_ok()
}

/// True when every record `learn list` reads in this project has the shape
/// the Python `status` reads without an error, and the learn crate reads
/// the same way (one `concepts` deck, string ids, known levels).
fn project_ok(root: &Path, name: &str) -> bool {
    project_shape(root, name).is_some()
}

fn project_shape(root: &Path, name: &str) -> Option<()> {
    let base = format!("learn/{name}");
    let manifest =
        careerbrew_learn::read_json(root, &format!("{base}/manifest.json"), Default::default())
            .ok()?;
    let fields = manifest.as_object().filter(|rows| !rows.is_empty())?;
    if fields.get("sources").is_some_and(|value| !value.is_array()) {
        return None;
    }
    let doc =
        careerbrew_learn::read_yaml(root, &format!("{base}/concepts.yaml"), Default::default())
            .ok()?;
    if !doc.is_null() {
        let rows = doc.as_object()?;
        if let Some(list) = rows.get("concepts") {
            let strings = list.as_array()?.iter().all(|concept| {
                ["id", "section"]
                    .iter()
                    .all(|key| concept.get(*key).is_some_and(|value| value.is_string()))
            });
            if !strings {
                return None;
            }
        }
    }
    let model =
        careerbrew_learn::read_json(root, &format!("{base}/model.json"), Default::default())
            .ok()?;
    if !model.is_null() {
        let rows = model.as_object()?;
        if let Some(retests) = rows.get("retests") {
            for list in retests.as_object()?.values() {
                for row in list.as_array()? {
                    let fields = row.as_object()?;
                    let result = fields.get("result");
                    if !fields.get("kind").is_some_and(|value| value.is_string())
                        || result.is_some_and(|value| !value.is_string())
                        || (result.is_none()
                            && !fields.get("due").is_some_and(|value| value.is_string()))
                    {
                        return None;
                    }
                }
            }
        }
        if let Some(rows) = rows.get("retest_log") {
            for row in rows.as_array()? {
                if !row.get("kind").is_some_and(|value| value.is_string()) {
                    return None;
                }
            }
        }
    }
    for row in careerbrew_learn::read_jsonl(root, &format!("{base}/log.jsonl")).ok()? {
        let text = |key: &str| row.get(key).and_then(|value| value.as_str());
        if text("concept").is_none() || text("mode").is_none() || text("result").is_none() {
            return None;
        }
        if !text("confidence").is_some_and(|value| CONFIDENCE.contains(&value))
            || !text("level").is_some_and(|value| BLOOM.contains(&value))
        {
            return None;
        }
    }
    // Python's card calls read every `*.yaml` deck in `srs/decks/`; the
    // learn crate reads only `concepts.yaml`.
    let decks = root.join(&base).join("srs/decks");
    if decks.is_dir() {
        for entry in std::fs::read_dir(&decks).ok()? {
            let file = entry.ok()?.file_name();
            let file = file.to_str()?;
            if file.ends_with(".yaml") && file != "concepts.yaml" {
                return None;
            }
        }
    }
    Some(())
}

fn list(ctx: &Ctx) -> Option<Outcome> {
    let root = &ctx.root;
    let base = root.join("learn");
    if base.exists() && !base.is_dir() {
        return None;
    }
    let mut names = Vec::new();
    if base.is_dir() {
        for entry in std::fs::read_dir(&base).ok()? {
            let entry = entry.ok()?;
            if entry.path().join("manifest.json").exists() {
                let name = entry.file_name().into_string().ok()?;
                // Python reads `learn/<slug(name)>`: a folder whose name is
                // not its own slug is another project's folder.
                if lp::slug(&name) != name {
                    return None;
                }
                names.push(name);
            }
        }
    }
    if !names.is_empty() && !(clock_ok() && key_present()) {
        return None;
    }
    if !names.iter().all(|name| project_ok(root, name)) {
        return None;
    }
    let out = careerbrew_learn::cli::learn_list(root).ok()?;
    if out.exit != 0 {
        return None;
    }
    let mut dirs = vec![base.clone()];
    dirs.extend(names.iter().map(|name| base.join(name).join("srs")));
    touch_dirs(root, &dirs)?;
    done(out.stdout)
}

/// The checks `project.note` makes before it reads or writes, so that a
/// refusal never follows a write: the project exists, and the note path is
/// plain (no empty, `.` or `..` part, ends in `.md`).
fn note(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let project = args.text("project")?;
    let given = args.text("path")?;
    // Python's `if a.set:` treats an empty value as no value.
    let set = args.text("set").filter(|value| !value.is_empty());
    let consume = args.flag("consume");
    let root = &ctx.root;
    if !key_present() {
        return None;
    }
    let slug = lp::slug(project);
    let manifest = careerbrew_learn::read_json(
        root,
        &format!("learn/{slug}/manifest.json"),
        Default::default(),
    )
    .ok()?;
    manifest.as_object().filter(|rows| !rows.is_empty())?;
    let rel = given.trim_matches('/');
    if rel.is_empty()
        || !rel.ends_with(".md")
        || rel.contains('\0')
        || rel
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    // The vault seals by the resolved path; a symlink anywhere on the way
    // could resolve outside the data root, so it goes to Python.
    let folder = root.join("learn").join(&slug);
    let mut chain = vec![root.join("learn"), folder.clone(), folder.join("notes")];
    let mut at = folder.join("notes");
    for part in rel.split('/') {
        at = at.join(part);
        chain.push(at.clone());
    }
    let target = at;
    if chain.iter().any(|path| is_symlink(path)) {
        return None;
    }
    let parents = &chain[..chain.len() - 1];
    if parents.iter().any(|path| path.exists() && !path.is_dir()) {
        return None;
    }
    match set {
        Some(source) => {
            if target.exists() && !target.is_file() {
                return None;
            }
            let meta = std::fs::metadata(source).ok()?;
            if !meta.is_file() {
                return None;
            }
            let bytes = std::fs::read(source).ok()?;
            // `Path.read_text` turns `\r\n` and `\r` into `\n`.
            if bytes.contains(&b'\r') {
                return None;
            }
            let text = String::from_utf8(bytes).ok()?;
            lp::note(root, project, rel, Some(&text)).ok()?;
            if consume {
                // After the write: a failure here sends the run to Python,
                // which writes the same note again and prints its error.
                std::fs::remove_file(source).ok()?;
            }
            touch_dirs(root, &[])?;
            done(format!("saved note {given}\n"))
        }
        None => {
            if !target.is_file() {
                return None;
            }
            let text = lp::note(root, project, rel, None).ok()?;
            touch_dirs(root, &[])?;
            done(text + "\n")
        }
    }
}

// ---------- learn status and learn next (text forms only) ----------

/// `serde_json::Value`, named through a public alias of a domain crate:
/// this crate does not depend on `serde_json` directly (a private copy of
/// the alias in `rust16`).
type Value = <<careerbrew_sources::connectors::Fetch<'static> as ReturnOf>::R as OkOf>::T;

trait ReturnOf {
    type R;
}

impl<R> ReturnOf for &'static mut dyn FnMut(&str) -> R {
    type R = R;
}

trait OkOf {
    type T;
}

impl<T, E> OkOf for Result<T, E> {
    type T = T;
}

fn str_of<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key)?.as_str()
}

/// A stamp both `datetime.fromisoformat` and RFC 3339 read as the same
/// instant: `YYYY-MM-DDTHH:MM:SS`, an optional `.` and 1 to 6 digits, then
/// `Z` or `+HH:MM`.
fn stamp_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let base = digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19);
    let mut rest = &bytes[19..];
    if rest.first() == Some(&b'.') {
        let count = rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if !(1..=6).contains(&count) {
            return false;
        }
        rest = &rest[1 + count..];
    }
    let zone = match rest {
        b"Z" => true,
        [sign, h1, h2, b':', m1, m2] => {
            matches!(sign, b'+' | b'-') && [h1, h2, m1, m2].iter().all(|b| b.is_ascii_digit())
        }
        _ => false,
    };
    base && zone && careerbrew_learn::parse_at(value).is_ok()
}

/// The concepts in file order, as Python's `concepts()` reads them.
fn concept_rows(root: &Path, slug: &str) -> Option<Vec<Value>> {
    let doc = careerbrew_learn::read_yaml(
        root,
        &format!("learn/{slug}/concepts.yaml"),
        Default::default(),
    )
    .ok()?;
    if doc.is_null() {
        return Some(Vec::new());
    }
    match doc.as_object()?.get("concepts") {
        None => Some(Vec::new()),
        Some(list) => Some(list.as_array()?.clone()),
    }
}

/// Each section once, in the order its first concept appears: the order of
/// Python's `sections` dict. The learn crate keeps them in a sorted map.
fn section_order(concepts: &[Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for concept in concepts {
        if let Some(section) = str_of(concept, "section") {
            if !out.iter().any(|known| known == section) {
                out.push(section.to_owned());
            }
        }
    }
    out
}

/// True for an absent value, or a list of strings.
fn string_list_or_absent(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(value) => value
            .as_array()
            .is_some_and(|items| items.iter().all(|item| item.is_string())),
    }
}

/// The card files (`srs/`) in the shape Python's card calls read without an
/// error, with every card id a concept id.
fn cards_ok(root: &Path, slug: &str, ids: &[&str]) -> Option<()> {
    let base = format!("learn/{slug}/srs");
    let deck = careerbrew_learn::read_yaml(
        root,
        &format!("{base}/decks/concepts.yaml"),
        Default::default(),
    )
    .ok()?;
    if !deck.is_null() {
        if let Some(cards) = deck.as_object()?.get("cards") {
            for card in cards.as_array()? {
                let id = str_of(card, "id")?;
                if !ids.contains(&id) || str_of(card, "topic").is_none() {
                    return None;
                }
                if card
                    .get("tags")
                    .is_some_and(|tags| !(tags.is_null() || tags.is_array()))
                {
                    return None;
                }
            }
        }
    }
    let meta =
        careerbrew_learn::read_json(root, &format!("{base}/card-meta.json"), Default::default())
            .ok()?;
    if !meta.is_null() && !string_list_or_absent(meta.as_object()?.get("suspended")) {
        return None;
    }
    let states =
        careerbrew_learn::read_json(root, &format!("{base}/card-state.json"), Default::default())
            .ok()?;
    if !states.is_null() {
        for state in states.as_object()?.values() {
            let fields = state.as_object()?;
            for key in ["card_id", "step", "stability", "difficulty"] {
                fields.get(key)?;
            }
            if !fields
                .get("state")
                .and_then(|value| value.as_u64())
                .is_some_and(|value| (1..=3).contains(&value))
                || !str_of(state, "due").is_some_and(stamp_ok)
                || !fields
                    .get("last_review")
                    .is_some_and(|last| last.is_null() || last.as_str().is_some_and(stamp_ok))
            {
                return None;
            }
        }
    }
    for row in careerbrew_learn::read_jsonl(root, &format!("{base}/card-reviews.jsonl")).ok()? {
        if !str_of(&row, "at").is_some_and(stamp_ok)
            || ["card", "rating", "topic"]
                .iter()
                .any(|key| str_of(&row, key).is_none())
            || row.get("first").is_some_and(|first| !first.is_boolean())
        {
            return None;
        }
    }
    Some(())
}

/// The checks `learn status` and `learn next` share: the clock and key, the
/// records `project_shape` checks, a plain `stale` list, and the card files.
/// Returns the concepts in file order.
fn study_shape(root: &Path, slug: &str) -> Option<Vec<Value>> {
    if !(clock_ok() && key_present()) {
        return None;
    }
    project_shape(root, slug)?;
    let manifest = careerbrew_learn::read_json(
        root,
        &format!("learn/{slug}/manifest.json"),
        Default::default(),
    )
    .ok()?;
    let stale = manifest.as_object()?.get("stale");
    if !(stale.is_some_and(|value| value.is_null()) || string_list_or_absent(stale)) {
        return None;
    }
    let concepts = concept_rows(root, slug)?;
    let ids = concepts
        .iter()
        .map(|concept| str_of(concept, "id"))
        .collect::<Option<Vec<_>>>()?;
    cards_ok(root, slug, &ids)?;
    Some(concepts)
}

/// `cmd_learn_status` without `--json`: sections in Python's order, then the
/// summary line and any stale sources.
fn status(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if args.flag("json") {
        return None;
    }
    let project = args.text("project")?;
    let root = &ctx.root;
    let slug = lp::slug(project);
    let concepts = study_shape(root, &slug)?;
    let st = lp::status(root, project, None).ok()?;
    let sections = st.get("sections")?.as_object()?;
    let mut out = String::new();
    for name in section_order(&concepts) {
        let row = sections.get(&name)?;
        out.push_str(&format!(
            "  {:>3}%  {name}  ({} left)\n",
            row.get("known_pct")?.as_u64()?,
            row.get("remaining")?.as_array()?.len()
        ));
    }
    let count = |key: &str| st.get(key).and_then(|value| value.as_u64());
    let optional = |key: &str| match st.get(key) {
        Some(value) if value.is_null() => Some("n/a".to_owned()),
        Some(value) => value.as_f64().map(careerbrew_projects::python_float),
        None => None,
    };
    out.push_str(&format!(
        "{project}: {}/{} mastered · due: {} review(s), {} retest(s) · 7-day retest kept: {} · calibration (Brier): {}\n",
        count("mastered")?,
        count("concepts")?,
        count("due_reviews")?,
        count("due_retests")?,
        optional("retest_retained_7d")?,
        optional("brier")?,
    ));
    let stale = match st.get("stale")? {
        value if value.is_null() => Vec::new(),
        value => value
            .as_array()?
            .iter()
            .map(|item| item.as_str())
            .collect::<Option<Vec<_>>>()?,
    };
    if !stale.is_empty() {
        out.push_str(&format!(
            "changed sources to re-teach: {}\n",
            stale.join(", ")
        ));
    }
    touch_dirs(root, &[root.join("learn").join(&slug).join("srs")])?;
    done(out)
}

/// The concept, question and log shapes Python's `next_step` reads without
/// an error, and the cases where the learn crate picks the same step: unique
/// ids, and sections whose first appearance is already in sorted order (the
/// crate breaks ties between equally weak sections by name, Python by that
/// order).
fn next_shape(root: &Path, slug: &str, concepts: &[Value]) -> Option<()> {
    let mut ids: Vec<&str> = Vec::new();
    for concept in concepts {
        let id = str_of(concept, "id")?;
        if ids.contains(&id) {
            return None;
        }
        ids.push(id);
        str_of(concept, "title")?;
        str_of(concept, "section")?;
        concept.get("cites")?;
        let questions = concept.get("questions")?.as_array()?;
        if questions.is_empty() {
            return None;
        }
        for question in questions {
            if !str_of(question, "bloom").is_some_and(|level| BLOOM.contains(&level)) {
                return None;
            }
            str_of(question, "prompt")?;
        }
        if !string_list_or_absent(concept.get("prereqs")) {
            return None;
        }
    }
    let order = section_order(concepts);
    let mut sorted = order.clone();
    sorted.sort();
    if order != sorted {
        return None;
    }
    for row in careerbrew_learn::read_jsonl(root, &format!("learn/{slug}/log.jsonl")).ok()? {
        str_of(&row, "section")?;
    }
    Some(())
}

/// `cmd_learn_next` without `--json`, through the learn crate's text form.
fn next(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if args.flag("json") {
        return None;
    }
    let project = args.text("project")?;
    let root = &ctx.root;
    let slug = lp::slug(project);
    let concepts = study_shape(root, &slug)?;
    next_shape(root, &slug, &concepts)?;
    let out = careerbrew_learn::cli::learn_next(root, project).ok()?;
    if out.exit != 0 {
        return None;
    }
    // Python reaches the card queue (which makes `srs/`) on every path but
    // the first two: no concepts yet, or a retest due.
    let folder = root.join("learn").join(&slug);
    let mut dirs = vec![folder.clone()];
    if !(out.stdout.starts_with("build: ") || out.stdout.starts_with("retest: ")) {
        dirs.push(folder.join("srs"));
    }
    touch_dirs(root, &dirs)?;
    done(out.stdout)
}
