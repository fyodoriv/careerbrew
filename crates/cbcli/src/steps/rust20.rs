//! Native CLI handlers owned by `rust-20` (see `steps/mod.rs`): the tutor's
//! cards, the public deck check, the Notion library and the drill item list.
//!
//! Every handler checks the files it reads first, in the vault's typed value
//! form (`Canon`), and refuses (Python then runs the command) on any shape
//! Python would read differently or crash on: a non-text field, a hidden or
//! non-file `*.yaml` entry, a malformed row, an unreadable or encrypted file
//! it cannot open, a corporate machine role, or a redaction input (all
//! output here can hold the owner's text). A refusal always comes before a
//! write.
//!
//! Left in Python, with the reason:
//! - `cards.grade`: py-fsrs fuzzes review intervals, which the Rust
//!   scheduler does not port yet (TASKS.md, rust-20 follow-up gap 1).
//! - `cards.stats`: Python orders tied topics by first card in deck-file
//!   order, and keeps microseconds in its 30-day bound; the domain crate
//!   orders by topic name and cuts to seconds (gaps 8 and 9).
//! - `drill.next` and `drill.answer`: they need `jobs.strategy.CORE_TOPICS`,
//!   the rubric layout and the attempt writers, which have no parity cases
//!   in this step yet.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust20.rs`.
pub const COMMANDS: &[&str] = &[
    "cards.next",
    "cards.add",
    "cards.unsuspend",
    "cards.public-check",
    "library.list",
    "library.show",
    "drill.list",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // The domain crates panic on a pinned clock they cannot parse; that is a
    // refusal here, and every handler reads its clock before any write.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve(name, args, ctx)))
        .ok()
        .flatten()
}

fn serve(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() {
        return None;
    }
    let vault = careerbrew_vault::transfer::Context::from_env().ok()?;
    if vault.root != ctx.root {
        return None;
    }
    let files = Files {
        root: ctx.root.clone(),
        vault: &vault,
    };
    match name {
        "cards.public-check" => public_check(),
        "cards.next" => {
            files.personal()?;
            cards_next(args, &files)
        }
        "cards.add" => {
            files.personal()?;
            cards_add(args, &files)
        }
        "cards.unsuspend" => {
            files.personal()?;
            cards_unsuspend(args, &files)
        }
        "library.list" => {
            files.personal()?;
            library_list(&files)
        }
        "library.show" => {
            files.personal()?;
            library_show(args, &files)
        }
        "drill.list" => {
            files.personal()?;
            drill_list(&files)
        }
        _ => None,
    }
}

fn ok(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

// ---------- typed value helpers ----------

/// The value of `key` in a map, as Python's dict keeps it (the last one).
fn get<'a>(value: &'a Canon, key: &str) -> Option<&'a Canon> {
    let Canon::Map(pairs) = value else {
        return None;
    };
    pairs
        .iter()
        .rev()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
        .map(|(_, v)| v)
}

fn text<'a>(value: &'a Canon, key: &str) -> Option<&'a str> {
    match get(value, key) {
        Some(Canon::Str(text)) => Some(text),
        _ => None,
    }
}

/// A map with text keys and no key twice: the only maps checked here.
fn plain_map(value: &Canon) -> bool {
    let Canon::Map(pairs) = value else {
        return false;
    };
    let mut seen = std::collections::BTreeSet::new();
    pairs
        .iter()
        .all(|(k, _)| matches!(k, Canon::Str(k) if seen.insert(k.clone())))
}

/// Absent, or a list of text.
fn texts_or_absent(value: &Canon, key: &str) -> bool {
    match get(value, key) {
        None => true,
        Some(Canon::List(items)) => items.iter().all(|item| matches!(item, Canon::Str(_))),
        _ => false,
    }
}

fn texts(value: &Canon, key: &str) -> Vec<String> {
    match get(value, key) {
        Some(Canon::List(items)) => items
            .iter()
            .filter_map(|item| match item {
                Canon::Str(text) => Some(text.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Characters Python's `str.split`/`str.strip` treat as space and Rust's
/// `split_whitespace`/`trim` do not (the four information separators).
fn odd_space(text: &str) -> bool {
    text.chars().any(|ch| ('\u{1c}'..='\u{1f}').contains(&ch))
}

/// `YYYY-MM-DDTHH:MM:SS[.ffffff]+HH:MM`: the stamp both `datetime.fromisoformat`
/// and RFC 3339 read the same way.
fn iso_stamp(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    let head = b.len() >= 25
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    if !head {
        return false;
    }
    let tail = match b.len() {
        25 => 19,
        32 if b[19] == b'.' && digits(20..26) => 26,
        _ => return false,
    };
    matches!(b[tail], b'+' | b'-')
        && digits(tail + 1..tail + 3)
        && b[tail + 3] == b':'
        && digits(tail + 4..tail + 6)
}

/// `re.sub(r"[^a-z0-9]+", "-", text.casefold()).strip("-") or "card"`.
fn slug(value: &str) -> String {
    let folded = careerbrew_tutor::casefold(value);
    let mut out = String::new();
    let mut gap = false;
    for ch in folded.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if gap {
                out.push('-');
            }
            gap = false;
            out.push(ch);
        } else {
            gap = true;
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        "card".into()
    } else {
        out
    }
}

/// Python's `f"{value:<width}"` for text.
fn left(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

/// The repository whose `knowledge/` Python reads: the one the console
/// script names, else the checkout this binary was built from.
fn repo() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_NATIVE_REPO") {
        Ok(repo) if repo.starts_with('/') => Some(PathBuf::from(repo)),
        Ok(_) => None,
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .ok(),
    }
}

/// `public.cards_dir()`: `CAREERBREW_PUBLIC_CARDS` when set and not empty.
fn public_dir() -> Option<PathBuf> {
    match std::env::var("CAREERBREW_PUBLIC_CARDS") {
        Ok(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        Ok(_) | Err(std::env::VarError::NotPresent) => Some(repo()?.join("knowledge/cards")),
        Err(_) => None,
    }
}

/// `sorted(folder.glob("*.yaml")) if folder.is_dir() else []`, or None for
/// an entry Python and the domain crate could list differently (a hidden
/// name, a non-file, a name that is not UTF-8).
fn yaml_files(folder: &Path) -> Option<Vec<PathBuf>> {
    if !folder.is_dir() {
        return Some(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(folder).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        if !name.ends_with(".yaml") {
            continue;
        }
        if name.starts_with('.') || !entry.path().is_file() {
            return None;
        }
        names.push(name);
    }
    names.sort();
    Some(names.into_iter().map(|name| folder.join(name)).collect())
}

/// A repo file Python reads with a plain YAML load (no vault).
fn repo_yaml(path: &Path) -> Option<Canon> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Canon::Null),
        Err(_) => return None,
    };
    if raw.starts_with(careerbrew_vault::MAGIC) {
        return None;
    }
    careerbrew_vault::json_yaml::yaml_read(&String::from_utf8(raw).ok()?).ok()
}

/// `(data or {}).get(key, [])` for a document that must be a map: the list
/// of maps under `key`, or None when Python would crash on it.
fn rows_of(data: &Canon, key: &str) -> Option<Vec<Canon>> {
    let empty = match data {
        Canon::Null => true,
        Canon::List(items) => items.is_empty(),
        Canon::Str(text) => text.is_empty(),
        _ => false,
    };
    if empty {
        return Some(Vec::new());
    }
    if !plain_map(data) {
        return None;
    }
    match get(data, key) {
        None => Some(Vec::new()),
        Some(Canon::List(items)) if items.iter().all(plain_map) => Some(items.clone()),
        _ => None,
    }
}

struct Files<'a> {
    root: PathBuf,
    vault: &'a careerbrew_vault::transfer::Context,
}

impl Files<'_> {
    /// A data-root file through the vault; `Null` when it is missing.
    fn yaml(&self, relative: &str) -> Option<Canon> {
        self.yaml_at(&self.root.join(relative))
    }

    fn yaml_at(&self, path: &Path) -> Option<Canon> {
        self.vault.store().read_yaml(path, Canon::Null, false).ok()
    }

    fn json(&self, relative: &str) -> Option<Canon> {
        self.vault
            .store()
            .read_json(&self.root.join(relative), Canon::Null, false)
            .ok()
    }

    fn jsonl(&self, relative: &str) -> Option<Vec<Canon>> {
        self.vault
            .store()
            .read_jsonl(&self.root.join(relative), false)
            .ok()
    }

    /// `machine.guard`: a personal-only group refuses on a corporate role.
    /// Here a corporate role (or a machine file Python could not read the
    /// same way) refuses natively, and Python prints its error.
    fn personal(&self) -> Option<()> {
        let info = self.yaml("machine.yaml")?;
        let role = match &info {
            Canon::Null => None,
            Canon::List(items) if items.is_empty() => None,
            data if plain_map(data) => get(data, "role"),
            _ => return None,
        };
        match role {
            Some(Canon::Str(role)) if role == "corporate" => None,
            Some(Canon::Str(_)) | None => Some(()),
            Some(_) => None,
        }
    }

    /// The owner's decks (`tutor/decks/*.yaml`): every card a map with text
    /// `id`, `question` and `topic`, and `tags` a list of text.
    fn decks_ok(&self) -> Option<()> {
        for path in yaml_files(&self.root.join("tutor/decks"))? {
            for card in rows_of(&self.yaml_at(&path)?, "cards")? {
                for key in ["id", "question", "topic"] {
                    text(&card, key)?;
                }
                if !texts_or_absent(&card, "tags") {
                    return None;
                }
            }
        }
        Some(())
    }

    /// The public deck and the owner's private explanations, in the shapes
    /// `all_cards` and `explanation_for` read.
    fn public_ok(&self, public: &Path) -> Option<()> {
        for path in yaml_files(public)? {
            for card in rows_of(&repo_yaml(&path)?, "cards")? {
                for key in ["id", "question", "answer", "topic"] {
                    text(&card, key)?;
                }
                if !texts_or_absent(&card, "tags") || !texts_or_absent(&card, "match") {
                    return None;
                }
            }
        }
        let private = self.yaml("tutor/private-explanations.yaml")?;
        for card in rows_of(&private, "cards")? {
            if !texts_or_absent(&card, "match") {
                return None;
            }
        }
        Some(())
    }

    /// `tutor/card-meta.json`: missing, or a map whose `suspended` is a list
    /// of text and whose `lapse_base` is a map.
    fn meta(&self) -> Option<Canon> {
        let meta = self.json("tutor/card-meta.json")?;
        if meta == Canon::Null {
            return Some(Canon::Map(Vec::new()));
        }
        if !plain_map(&meta) || !texts_or_absent(&meta, "suspended") {
            return None;
        }
        match get(&meta, "lapse_base") {
            None => {}
            Some(base) if plain_map(base) => {}
            Some(_) => return None,
        }
        Some(meta)
    }

    /// `tutor/card-reviews.jsonl`: maps with a text `card`, a boolean (or
    /// absent) `first` and `lapse`, and an ISO stamp wherever `first` is set.
    fn reviews_ok(&self) -> Option<Vec<Canon>> {
        let rows = self.jsonl("tutor/card-reviews.jsonl")?;
        for row in &rows {
            if !plain_map(row) {
                return None;
            }
            text(row, "card")?;
            for key in ["first", "lapse"] {
                match get(row, key) {
                    None | Some(Canon::Bool(_)) => {}
                    Some(_) => return None,
                }
            }
            if matches!(get(row, "first"), Some(Canon::Bool(true))) && !iso_stamp(text(row, "at")?)
            {
                return None;
            }
        }
        Some(rows)
    }

    /// `settings.yaml`'s `cards_new_per_day`: absent or a whole number.
    fn settings_ok(&self) -> Option<()> {
        let settings = self.yaml("settings.yaml")?;
        let empty = matches!(&settings, Canon::Null)
            || matches!(&settings, Canon::List(items) if items.is_empty());
        if empty {
            return Some(());
        }
        if !plain_map(&settings) {
            return None;
        }
        match get(&settings, "cards_new_per_day") {
            None => Some(()),
            Some(Canon::Int(value)) => value.to_string().parse::<i64>().ok().map(|_| ()),
            Some(_) => None,
        }
    }
}

// ---------- cards ----------

fn cards_next(args: &Parsed, files: &Files) -> Option<Outcome> {
    let count = usize::try_from(args.int("count")?).ok()?;
    let public = public_dir()?;
    files.decks_ok()?;
    files.public_ok(&public)?;
    files.meta()?;
    files.reviews_ok()?;
    files.settings_ok()?;
    let at = careerbrew_tutor::now();
    let queue = careerbrew_tutor::cards::queue(&files.root, &public, at, None, count, &[]).ok()?;
    let mut rows = Vec::new();
    for card in &queue {
        let get = |key: &str| match careerbrew_tutor::field(card, key) {
            Some(value) if value.is_string() => Some(careerbrew_tutor::text(Some(value))),
            _ => None,
        };
        rows.push([get("id")?, get("topic")?, get("mode")?, get("question")?]);
    }
    let mut out = String::new();
    if args.flag("json") {
        if rows.is_empty() {
            out.push_str("[]");
        } else {
            out.push_str("[\n");
            for (at, row) in rows.iter().enumerate() {
                out.push_str("  {\n");
                for (index, key) in ["id", "topic", "mode", "question"].iter().enumerate() {
                    out.push_str("    ");
                    crate::render::py_string(key, &mut out);
                    out.push_str(": ");
                    crate::render::py_string(&row[index], &mut out);
                    out.push_str(if index < 3 { ",\n" } else { "\n" });
                }
                out.push_str(if at + 1 < rows.len() {
                    "  },\n"
                } else {
                    "  }\n"
                });
            }
            out.push(']');
        }
        out.push('\n');
    } else {
        for [id, topic, mode, question] in &rows {
            out.push_str(&format!("[{mode}] {topic}  ({id})  {question}\n"));
        }
        out.push_str(&format!(
            "{} card(s) queued. Review: careerbrew cards review\n",
            rows.len()
        ));
    }
    ok(out, 0)
}

struct NowClock;

impl careerbrew_vault::events::Clock for NowClock {
    fn stamp(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        crate::events::stamp(secs)
    }

    fn version(&self) -> String {
        crate::events::version().to_owned()
    }
}

/// `events.emit(event, status=...)`; a failed log write is ignored, as the
/// command's own write already happened.
fn emit(root: &Path, event: &str, status: &str) {
    let fields = Canon::Map(vec![(
        Canon::Str("status".into()),
        Canon::Str(status.into()),
    )]);
    let _ = careerbrew_vault::events::emit(
        root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        event,
        &fields,
        &NowClock,
    );
}

fn cards_add(args: &Parsed, files: &Files) -> Option<Outcome> {
    let question = args.text("question")?;
    let answer = args.text("answer")?;
    let tags: Vec<String> = args
        .text("tags")?
        .split(',')
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect();
    let deck = args.text("deck")?;
    // An empty question or answer is Python's `ValueError` (exit 2 and a
    // `cli.error` row): Python prints it.
    if [question, answer]
        .iter()
        .any(|text| odd_space(text) || text.trim().is_empty())
    {
        return None;
    }
    let name = slug(deck);
    let path = files.root.join(format!("tutor/decks/{name}.yaml"));
    let data = files.yaml_at(&path)?;
    let mut pairs = match &data {
        Canon::Null => vec![(Canon::Str("cards".into()), Canon::List(Vec::new()))],
        Canon::List(items) if items.is_empty() => {
            vec![(Canon::Str("cards".into()), Canon::List(Vec::new()))]
        }
        Canon::Map(pairs) if plain_map(&data) => pairs.clone(),
        _ => return None,
    };
    let id = format!("{name}-{}", &careerbrew_tutor::digest(question)[..10]);
    let old = match get(&Canon::Map(pairs.clone()), "cards") {
        None => Vec::new(),
        Some(Canon::List(items)) => items.clone(),
        Some(_) => return None,
    };
    let mut cards = Vec::new();
    for card in old {
        if !plain_map(&card) {
            return None;
        }
        match get(&card, "id")? {
            Canon::Str(old_id) if *old_id == id => {}
            _ => cards.push(card),
        }
    }
    let topic = careerbrew_tutor::casefold(tags.first().map_or("general", String::as_str));
    cards.push(Canon::Map(vec![
        (Canon::Str("id".into()), Canon::Str(id.clone())),
        (
            Canon::Str("question".into()),
            Canon::Str(question.trim().into()),
        ),
        (
            Canon::Str("answer".into()),
            Canon::Str(answer.trim().into()),
        ),
        (Canon::Str("topic".into()), Canon::Str(topic)),
        (
            Canon::Str("tags".into()),
            Canon::List(tags.into_iter().map(Canon::Str).collect()),
        ),
    ]));
    match pairs
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == "cards"))
    {
        Some((_, value)) => *value = Canon::List(cards),
        None => pairs.push((Canon::Str("cards".into()), Canon::List(cards))),
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    files
        .vault
        .store()
        .write_yaml(&path, &Canon::Map(pairs))
        .ok()?;
    emit(&files.root, "cards.add", "ok");
    ok(format!("added card {id} to deck {name}\n"), 0)
}

fn cards_unsuspend(args: &Parsed, files: &Files) -> Option<Outcome> {
    let id = args.text("id")?;
    let public = public_dir()?;
    files.decks_ok()?;
    files.public_ok(&public)?;
    let meta = files.meta()?;
    let reviews = files.reviews_ok()?;
    // An unknown id is Python's `KeyError` (exit 2): Python prints it.
    let cards = careerbrew_tutor::cards::all_cards(&files.root, &public).ok()?;
    if !cards.contains_key(id) {
        return None;
    }
    let suspended: std::collections::BTreeSet<String> = texts(&meta, "suspended")
        .into_iter()
        .filter(|known| known != id)
        .collect();
    let lapses = reviews
        .iter()
        .filter(|row| text(row, "card") == Some(id))
        .filter(|row| matches!(get(row, "lapse"), Some(Canon::Bool(true))))
        .count();
    let Canon::Map(mut pairs) = meta else {
        return None;
    };
    let list = Canon::List(suspended.into_iter().map(Canon::Str).collect());
    set(&mut pairs, "suspended", list);
    let mut base = match get(&Canon::Map(pairs.clone()), "lapse_base") {
        Some(Canon::Map(base)) => base.clone(),
        _ => Vec::new(),
    };
    set(&mut base, id, Canon::Int(lapses.into()));
    set(&mut pairs, "lapse_base", Canon::Map(base));
    files
        .vault
        .store()
        .write_json(&files.root.join("tutor/card-meta.json"), &Canon::Map(pairs))
        .ok()?;
    ok(format!("{id} is back in the queue\n"), 0)
}

/// `mapping[key] = value`: replace in place, else append.
fn set(pairs: &mut Vec<(Canon, Canon)>, key: &str, value: Canon) {
    match pairs
        .iter_mut()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == key))
    {
        Some((_, old)) => *old = value,
        None => pairs.push((Canon::Str(key.into()), value)),
    }
}

/// `public.check()`: every committed card passes lint and ids are unique.
fn public_check() -> Option<Outcome> {
    let public = public_dir()?;
    for path in yaml_files(&public)? {
        for card in rows_of(&repo_yaml(&path)?, "cards")? {
            text(&card, "id")?;
            for key in ["id", "question", "answer", "explanation", "topic"] {
                match get(&card, key) {
                    None | Some(Canon::Null) => {}
                    Some(Canon::Str(value)) if !odd_space(value) => {}
                    Some(_) => return None,
                }
            }
        }
    }
    let problems = careerbrew_tutor::public::check(&public).ok()?;
    let total = careerbrew_tutor::public::load(&public).ok()?.len();
    let mut out = String::new();
    for problem in &problems {
        out.push_str(problem);
        out.push('\n');
    }
    out.push_str(&format!(
        "public cards: {total} card(s), {} problem(s)\n",
        problems.len()
    ));
    ok(out, i32::from(!problems.is_empty()))
}

// ---------- library ----------

/// `library/notion/index.json`: a list of maps with text `id`, `section`,
/// `title` and `file`.
fn library_entries(files: &Files) -> Option<Vec<Canon>> {
    let entries = match files.json("library/notion/index.json")? {
        Canon::Null => return Some(Vec::new()),
        Canon::List(items) => items,
        _ => return None,
    };
    for entry in &entries {
        if !plain_map(entry) {
            return None;
        }
        for key in ["id", "section", "title", "file"] {
            text(entry, key)?;
        }
    }
    Some(entries)
}

fn library_list(files: &Files) -> Option<Outcome> {
    let mut out = String::new();
    for entry in library_entries(files)? {
        let id: String = text(&entry, "id")?.chars().take(8).collect();
        out.push_str(&format!(
            "{} {id}  {}\n",
            left(text(&entry, "section")?, 9),
            text(&entry, "title")?
        ));
    }
    ok(out, 0)
}

fn library_show(args: &Parsed, files: &Files) -> Option<Outcome> {
    let id = args.text("id")?;
    let entries = library_entries(files)?;
    // `notion_import.page(id)` takes the first entry with an id: two entries
    // with one id could pick another file than the prefix match found.
    let mut ids = std::collections::BTreeSet::new();
    if !entries.iter().all(|entry| ids.insert(text(entry, "id"))) {
        return None;
    }
    let entry = entries
        .iter()
        .find(|entry| text(entry, "id") == Some(id))
        .or_else(|| {
            entries
                .iter()
                .find(|entry| text(entry, "id").is_some_and(|known| known.starts_with(id)))
        })?;
    let file = Path::new(text(entry, "file")?);
    let plain = !file.as_os_str().is_empty()
        && file
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
    if !plain {
        return None;
    }
    let markdown = files
        .vault
        .store()
        .read_text(
            &files.root.join("library/notion").join(file),
            Some(""),
            false,
        )
        .ok()?
        .unwrap_or_default();
    ok(format!("# {}\n\n{markdown}\n", text(entry, "title")?), 0)
}

// ---------- drill ----------

/// `tutor.items()`: base topics, then generated gap items, each a map with
/// text `id`, `topic` and `round`. A plugin knowledge folder (or an
/// installed plugin, a redaction input) goes to Python.
fn drill_list(files: &Files) -> Option<Outcome> {
    let repo = repo()?;
    if repo.join("knowledge/knowledge").exists() {
        return None;
    }
    let mut items = std::collections::BTreeMap::new();
    let sources = [
        (
            rows_of(&repo_yaml(&repo.join("knowledge/topics.yaml"))?, "items")?,
            "base",
        ),
        (
            rows_of(&files.yaml("tutor/gap-items.yaml")?, "items")?,
            "generated",
        ),
    ];
    for (rows, origin) in sources {
        for item in rows {
            let id = text(&item, "id")?.to_owned();
            let topic = text(&item, "topic")?.to_owned();
            let round = text(&item, "round")?.to_owned();
            items.insert(id, (topic, round, origin));
        }
    }
    let mut out = String::new();
    for (id, (topic, round, origin)) in items {
        out.push_str(&format!(
            "{} {} {} {origin}\n",
            left(&id, 28),
            left(&topic, 18),
            left(&round, 14)
        ));
    }
    ok(out, 0)
}
