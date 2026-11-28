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
//! - `cards.stats` is served here, not through the domain crate's `stats`:
//!   Python orders tied topics by first card in deck-file order and keeps
//!   microseconds in its 30-day bound (gaps 8 and 9), so this module counts
//!   in Python's order itself.
//! - `drill.next` is served only while no item has a card or an attempt
//!   (no `tutor/cards.json` or `tutor/attempts.jsonl`): then every mastery
//!   is 0 and no FSRS state is read. Any other state goes to Python.
//! - `drill.answer`: it writes FSRS cards through py-fsrs's scheduler, and
//!   `drill.speak` runs speech tools; no parity cases in this step yet.
//! - `cards.explain-queue` is served here, not through the domain crate's
//!   `explain_queue`: that one keeps cards in id order and JSON keys sorted,
//!   while Python keeps deck-file order and insertion key order. A prompt
//!   file with a carriage return (Python's universal newlines) refuses.
//! - `import.notion`: it writes many files and the sensitive terms list.
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
    "cards.stats",
    "cards.unsuspend",
    "cards.public-check",
    "cards.explain-queue",
    "library.list",
    "library.show",
    "drill.list",
    "drill.next",
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
        "cards.stats" => {
            files.personal()?;
            cards_stats(&files)
        }
        "cards.explain-queue" => {
            files.personal()?;
            explain_queue_cmd(args, &files)
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
        "drill.next" => {
            files.personal()?;
            drill_next(args, &files)
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
    // `store.write_yaml` is `yaml.safe_dump(sort_keys=False,
    // allow_unicode=True)`; a value the port cannot prove identical refuses.
    // A sealed write differs on every run, so an encrypted folder refuses.
    let yaml = crate::pyyaml::safe_dump(&Canon::Map(pairs))?;
    if files.vault.store().encryption_enabled() {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    files.vault.store().write_text(&path, &yaml).ok()?;
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

/// Python's `datetime.isoformat()` of a UTC instant in microseconds.
fn utc_iso(micros: i64) -> String {
    let secs = micros.div_euclid(1_000_000);
    let frac = micros.rem_euclid(1_000_000);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let mut out = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    if frac != 0 {
        out.push_str(&format!(".{frac:06}"));
    }
    out.push_str("+00:00");
    out
}

/// An instant in microseconds since the epoch.
fn micros_of(stamp: &str) -> Option<i64> {
    let at = careerbrew_tutor::parse_at(stamp).ok()?;
    Some(at.timestamp() * 1_000_000 + i64::from(at.timestamp_subsec_micros()))
}

/// `cards.stats()` as `cmd_cards_stats` prints it. Every row Python reads
/// is checked first; a shape where Python would crash or read differently
/// refuses.
fn cards_stats(files: &Files) -> Option<Outcome> {
    let public = public_dir()?;
    files.decks_ok()?;
    files.public_ok(&public)?;
    let meta = files.meta()?;
    let reviews = files.reviews_ok()?;
    files.settings_ok()?;
    clock_ok(files)?;

    // `all_cards()` in dict order: a later card with a known id replaces the
    // value but keeps the first position.
    type Cards = std::collections::HashMap<String, (String, String)>;
    fn put(ids: &mut Vec<String>, cards: &mut Cards, id: String, topic: String, question: String) {
        if !cards.contains_key(&id) {
            ids.push(id.clone());
        }
        cards.insert(id, (topic, question));
    }
    let mut ids: Vec<String> = Vec::new();
    let mut cards = Cards::new();
    for path in yaml_files(&files.root.join("tutor/decks"))? {
        for card in rows_of(&files.yaml_at(&path)?, "cards")? {
            put(
                &mut ids,
                &mut cards,
                text(&card, "id")?.to_owned(),
                text(&card, "topic")?.to_owned(),
                text(&card, "question")?.to_owned(),
            );
        }
    }
    let mine: std::collections::BTreeSet<String> = cards
        .values()
        .map(|(_, question)| careerbrew_tutor::public::qhash(question))
        .collect();
    let mut public_cards = Vec::new();
    for path in yaml_files(&public)? {
        public_cards.extend(rows_of(&repo_yaml(&path)?, "cards")?);
    }
    for card in public_cards {
        let question = text(&card, "question")?;
        let mut hashes = texts(&card, "match");
        hashes.push(careerbrew_tutor::public::qhash(question));
        if hashes.iter().all(|hash| !mine.contains(hash)) {
            put(
                &mut ids,
                &mut cards,
                text(&card, "id")?.to_owned(),
                text(&card, "topic")?.to_owned(),
                question.to_owned(),
            );
        }
    }

    // `_states()`: falsy is empty, else a map of FSRS card maps whose `due`
    // (and any `last_review`) is an aware ISO stamp; the domain crate must
    // read every state too, as `Card.from_dict` does.
    let raw = files.json("tutor/card-state.json")?;
    let mut due: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    if !falsy(&raw) {
        if !plain_map(&raw) {
            return None;
        }
        let Canon::Map(pairs) = &raw else {
            return None;
        };
        for (key, state) in pairs {
            let Canon::Str(id) = key else {
                return None;
            };
            if !plain_map(state) {
                return None;
            }
            let stamp = text(state, "due")?;
            if !iso_stamp(stamp) {
                return None;
            }
            match get(state, "last_review")? {
                Canon::Str(last) if iso_stamp(last) => {}
                value if falsy(value) => {}
                _ => return None,
            }
            due.insert(id.clone(), micros_of(stamp)?);
        }
        let parsed = careerbrew_tutor::cards::states(&files.root).ok()?;
        if parsed.len() != due.len() {
            return None;
        }
    }
    let suspended: std::collections::BTreeSet<String> =
        texts(&meta, "suspended").into_iter().collect();

    let now = careerbrew_tutor::now();
    let at = now.timestamp() * 1_000_000 + i64::from(now.timestamp_subsec_micros());
    let zone = careerbrew_tutor::system_zone();
    let local = now.with_timezone(&zone);
    let today = local.date_naive();
    let offset = (local.naive_local() - now.naive_utc()).num_seconds();
    let end_of_day = today
        .and_hms_micro_opt(23, 59, 59, 999_999)?
        .and_utc()
        .timestamp_micros()
        - offset * 1_000_000;
    let since = utc_iso(at - 30 * 86_400 * 1_000_000);

    // Every review row: a text `at` in an aware ISO shape (`_day` parses it);
    // a recent row also needs `rating` and `topic`, which Python indexes.
    let mut days = std::collections::BTreeSet::new();
    let mut reviewed_today = 0usize;
    let mut recent = Vec::new();
    for row in &reviews {
        let stamp = text(row, "at")?;
        if !iso_stamp(stamp) {
            return None;
        }
        let day = careerbrew_tutor::parse_at(stamp)
            .ok()?
            .with_timezone(&zone)
            .date_naive();
        reviewed_today += usize::from(day == today);
        days.insert(day);
        if stamp >= since.as_str() && cards.contains_key(text(row, "card")?) {
            get(row, "topic")?;
            recent.push(get(row, "rating")?);
        }
    }
    let retention = if recent.is_empty() {
        "n/a".to_owned()
    } else {
        let kept = recent
            .iter()
            .filter(|rating| !matches!(rating, Canon::Str(r) if r == "again"))
            .count();
        let value = careerbrew_projects::python_round(kept as f64 / recent.len() as f64, 3);
        format!("{:.0}%", value * 100.0)
    };
    let mut streak = 0usize;
    let mut day = if days.contains(&today) {
        today
    } else {
        today.pred_opt()?
    };
    while days.contains(&day) {
        streak += 1;
        day = day.pred_opt()?;
    }

    let is_due = |id: &str, bound: i64| {
        due.get(id)
            .is_some_and(|when| !suspended.contains(id) && *when <= bound)
    };
    let mut topics: Vec<(String, usize, usize, usize)> = Vec::new();
    for id in &ids {
        let topic = &cards[id].0;
        let at_row = match topics.iter().position(|row| &row.0 == topic) {
            Some(index) => index,
            None => {
                topics.push((topic.clone(), 0, 0, 0));
                topics.len() - 1
            }
        };
        let row = &mut topics[at_row];
        row.1 += 1;
        row.2 += usize::from(is_due(id, at));
        row.3 += usize::from(!due.contains_key(id));
    }
    let new = ids.iter().filter(|id| !due.contains_key(*id)).count();
    let due_now = ids.iter().filter(|id| is_due(id, at)).count();
    let due_today = ids.iter().filter(|id| is_due(id, end_of_day)).count();
    let leeches = suspended
        .iter()
        .filter(|id| cards.contains_key(*id))
        .count();
    let mut out = format!(
        "cards {}  new {new}  due now {due_now}  due today {due_today}  reviewed today {reviewed_today}  retention (30 d) {retention}  streak {streak} d  leeches {leeches}\n",
        ids.len()
    );
    topics.sort_by_key(|row| std::cmp::Reverse(row.1));
    for (topic, total, due, new) in topics.iter().take(12) {
        out.push_str(&format!(
            "  {} {total:>4} cards  {due:>3} due  {new:>4} new\n",
            left(topic, 24)
        ));
    }
    ok(out, 0)
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

// ---------- explain queue ----------

/// `public.PERSONAL_TOPICS`: behavioural answers never leave as hints.
const PERSONAL_TOPICS: &[&str] = &[
    "behavioural",
    "behavioral",
    "leadership",
    "tech lead",
    "business/product",
];

/// One explain-queue item, its keys in Python's insertion order.
struct Item {
    source_id: String,
    question: String,
    topic: String,
    hint: String,
    reason: String,
    rewrite: Option<String>,
    private: bool,
}

/// Python's `json.dumps` string form with `ensure_ascii=False`.
fn json_text(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{0}'..='\u{1f}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out.push('"');
}

/// `json.dumps(items, indent=1, ensure_ascii=False)`.
fn items_json(items: &[&Item]) -> String {
    if items.is_empty() {
        return "[]".into();
    }
    let mut out = String::from("[\n");
    for (at, item) in items.iter().enumerate() {
        let mut pairs = vec![
            ("source_id", Some(item.source_id.as_str())),
            ("question", Some(item.question.as_str())),
            ("topic", Some(item.topic.as_str())),
            ("hint", Some(item.hint.as_str())),
            ("reason", Some(item.reason.as_str())),
        ];
        if let Some(rewrite) = &item.rewrite {
            pairs.push(("rewrite", Some(rewrite.as_str())));
        }
        if item.private {
            pairs.push(("private", None));
        }
        out.push_str(" {\n");
        for (index, (key, value)) in pairs.iter().enumerate() {
            out.push_str("  ");
            json_text(key, &mut out);
            out.push_str(": ");
            match value {
                Some(value) => json_text(value, &mut out),
                None => out.push_str("true"),
            }
            out.push_str(if index + 1 < pairs.len() { ",\n" } else { "\n" });
        }
        out.push_str(if at + 1 < items.len() {
            " },\n"
        } else {
            " }\n"
        });
    }
    out.push(']');
    out
}

/// `store.read_json(path, {}) or {}` that the queue only looks keys up in:
/// a falsy value is empty, a plain map is kept, anything else refuses.
fn lookup_map(value: Canon) -> Option<Canon> {
    if falsy(&value) {
        return Some(Canon::Map(Vec::new()));
    }
    plain_map(&value).then_some(value)
}

fn has_key(map: &Canon, key: &str) -> bool {
    get(map, key).is_some()
}

/// `p.get("match") or [qhash(p["question"])]`, first item.
fn source_of(card: &Canon) -> String {
    texts(card, "match")
        .into_iter()
        .next()
        .unwrap_or_else(|| careerbrew_tutor::public::qhash(text(card, "question").unwrap_or("")))
}

/// `public.explain_queue()` in Python's order, or None when Python could
/// read a file differently or crash on it.
fn explain_queue(files: &Files) -> Option<(Vec<Item>, String)> {
    use careerbrew_tutor::public::qhash;
    use std::collections::{HashMap, HashSet};
    let repo = repo()?;
    let public_dir = public_dir()?;
    // The public deck: the shapes `load`, `by_match`, `all_cards` and `lint`
    // read, with no character Python and Rust split or strip differently.
    let mut public = Vec::new();
    for path in yaml_files(&public_dir)? {
        for card in rows_of(&repo_yaml(&path)?, "cards")? {
            for key in ["id", "question", "answer", "topic"] {
                text(&card, key)?;
            }
            for key in ["id", "question", "answer", "explanation", "topic"] {
                match get(&card, key) {
                    None | Some(Canon::Null) => {}
                    Some(Canon::Str(value)) if !odd_space(value) => {}
                    Some(_) => return None,
                }
            }
            if !texts_or_absent(&card, "tags") || !texts_or_absent(&card, "match") {
                return None;
            }
            public.push(card);
        }
    }
    let linted = careerbrew_tutor::public::load(&public_dir).ok()?;
    if linted.len() != public.len()
        || linted.iter().zip(&public).any(|(value, card)| {
            careerbrew_tutor::text(careerbrew_tutor::field(value, "id"))
                != text(card, "id").unwrap_or("")
        })
    {
        return None;
    }
    // `prompt_version()`: Python's text read turns `\r` into `\n`.
    let prompt = std::fs::read_to_string(repo.join("prompts/card-explainer.md")).ok()?;
    if prompt.contains('\r') {
        return None;
    }
    let version = careerbrew_tutor::digest(&prompt)[..8].to_owned();
    // Private explanations.
    let private = rows_of(&files.yaml("tutor/private-explanations.yaml")?, "cards")?;
    for card in &private {
        for key in ["id", "question", "topic"] {
            text(card, key)?;
        }
        if !texts_or_absent(card, "match") {
            return None;
        }
    }
    let skip = lookup_map(files.json("tutor/public-skipped.json")?)?;
    let flagged = lookup_map(files.json("tutor/explanation-flags.json")?)?;
    // `_lapses()`: "again" reviews per card id.
    let mut lapses: HashMap<String, usize> = HashMap::new();
    for row in files.jsonl("tutor/card-reviews.jsonl")? {
        if !plain_map(&row) {
            return None;
        }
        if matches!(get(&row, "rating"), Some(Canon::Str(rating)) if rating == "again") {
            *lapses.entry(text(&row, "card")?.to_owned()).or_default() += 1;
        }
    }
    // `cards.all_cards()`: deck files in name order, a later id replacing
    // an earlier one in place, then public cards the owner lacks.
    let mut all: Vec<(String, Canon, String)> = Vec::new();
    for path in yaml_files(&files.root.join("tutor/decks"))? {
        let deck = path.file_stem()?.to_str()?.to_owned();
        for card in rows_of(&files.yaml_at(&path)?, "cards")? {
            for key in ["id", "question", "topic"] {
                text(&card, key)?;
            }
            match get(&card, "answer") {
                None | Some(Canon::Str(_)) => {}
                Some(_) => return None,
            }
            let id = text(&card, "id")?.to_owned();
            match all.iter_mut().find(|(known, _, _)| *known == id) {
                Some((_, old, old_deck)) => {
                    *old = card;
                    old_deck.clone_from(&deck);
                }
                None => all.push((id, card, deck.clone())),
            }
        }
    }
    let mine: HashSet<String> = all
        .iter()
        .map(|(_, card, _)| qhash(text(card, "question").unwrap_or("")))
        .collect();
    for card in &public {
        let mut hashes = texts(card, "match");
        hashes.push(qhash(text(card, "question")?));
        if hashes.iter().all(|hash| !mine.contains(hash)) {
            let id = text(card, "id")?.to_owned();
            match all.iter_mut().find(|(known, _, _)| *known == id) {
                Some((_, old, deck)) => {
                    *old = card.clone();
                    *deck = "public".into();
                }
                None => all.push((id, card.clone(), "public".into())),
            }
        }
    }
    // `by_match() | {qhash(question): card}`.
    let mut covered: HashMap<String, usize> = HashMap::new();
    for (at, card) in public.iter().enumerate() {
        for hash in texts(card, "match") {
            covered.insert(hash, at);
        }
    }
    for (at, card) in public.iter().enumerate() {
        covered.insert(qhash(text(card, "question")?), at);
    }
    let private_done: HashSet<String> = private
        .iter()
        .flat_map(|card| texts(card, "match"))
        .collect();
    let mut items: Vec<Item> = Vec::new();
    let mut seen = HashSet::new();
    for (_, card, deck) in &all {
        if deck == "public" {
            continue;
        }
        let question = text(card, "question")?;
        let topic = text(card, "topic")?;
        let hash = qhash(question);
        if !seen.insert(hash.clone()) {
            continue;
        }
        if has_key(&skip, &hash) {
            if !private_done.contains(&hash) {
                items.push(Item {
                    source_id: hash,
                    question: question.to_owned(),
                    topic: topic.to_owned(),
                    hint: String::new(),
                    reason: "personal: explain privately".into(),
                    rewrite: None,
                    private: true,
                });
            }
            continue;
        }
        match covered.get(&hash) {
            None => {
                let personal =
                    PERSONAL_TOPICS.contains(&careerbrew_tutor::casefold(topic).as_str());
                let hint = if personal {
                    String::new()
                } else {
                    text(card, "answer")
                        .unwrap_or("")
                        .chars()
                        .take(1500)
                        .collect()
                };
                items.push(Item {
                    source_id: hash,
                    question: question.to_owned(),
                    topic: topic.to_owned(),
                    hint,
                    reason: "new".into(),
                    rewrite: None,
                    private: false,
                });
            }
            Some(&at) if lapses.get(text(card, "id")?).copied().unwrap_or(0) >= 3 => {
                let pub_card = &public[at];
                items.push(Item {
                    source_id: hash,
                    question: text(pub_card, "question")?.to_owned(),
                    topic: text(pub_card, "topic")?.to_owned(),
                    hint: String::new(),
                    reason: "learners keep failing it: make it clearer".into(),
                    rewrite: Some(text(pub_card, "id")?.to_owned()),
                    private: false,
                });
            }
            Some(_) => {}
        }
    }
    let rewrites =
        |items: &[Item], id: &str| items.iter().any(|item| item.rewrite.as_deref() == Some(id));
    for card in public.iter().chain(&private) {
        let id = text(card, "id")?;
        let Some(flag) = get(&flagged, id) else {
            continue;
        };
        if rewrites(&items, id) {
            continue;
        }
        if !plain_map(flag) {
            return None;
        }
        let note = text(flag, "note")?;
        items.push(Item {
            source_id: source_of(card),
            question: text(card, "question")?.to_owned(),
            topic: text(card, "topic")?.to_owned(),
            hint: String::new(),
            reason: format!("a reader reported: {note}"),
            rewrite: Some(id.to_owned()),
            private: id.starts_with("me-"),
        });
    }
    for (card, value) in public.iter().zip(&linted) {
        let id = text(card, "id")?;
        let current = matches!(get(card, "prompt"), Some(Canon::Str(prompt)) if *prompt == version);
        let why = if current {
            let problems = careerbrew_tutor::public::lint(value);
            if problems.is_empty() {
                String::new()
            } else {
                format!("fails lint: {}", problems.join("; "))
            }
        } else {
            "written with an older prompt".into()
        };
        if !why.is_empty() && !rewrites(&items, id) {
            items.push(Item {
                source_id: source_of(card),
                question: text(card, "question")?.to_owned(),
                topic: text(card, "topic")?.to_owned(),
                hint: String::new(),
                reason: why,
                rewrite: Some(id.to_owned()),
                private: false,
            });
        }
    }
    let order = |item: &Item| u8::from(item.reason == "new") * 2;
    items.sort_by(|a, b| {
        order(a)
            .cmp(&order(b))
            .then_with(|| a.topic.cmp(&b.topic))
            .then_with(|| a.question.cmp(&b.question))
    });
    Some((items, version))
}

/// `cmd_cards_explain_queue`.
fn explain_queue_cmd(args: &Parsed, files: &Files) -> Option<Outcome> {
    use crate::args::Value;
    let count = args.int("count")?;
    let offset = match args.get("offset") {
        Value::Missing => 0,
        Value::Int(offset) => *offset,
        _ => return None,
    };
    let topic = match args.get("topic") {
        Value::Missing => "",
        Value::Text(topic) => topic.as_str(),
        _ => return None,
    };
    // Python's negative slice bounds count from the end: not served.
    let start = usize::try_from(offset).ok()?;
    let count = usize::try_from(count).ok()?;
    let (items, version) = explain_queue(files)?;
    let mut out = String::new();
    if args.flag("json") {
        let folded = careerbrew_tutor::casefold(topic);
        let shown: Vec<&Item> = items
            .iter()
            .filter(|item| topic.is_empty() || item.topic == folded)
            .skip(start)
            .take(count)
            .collect();
        out.push_str(&items_json(&shown));
    } else {
        let mut by: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for item in &items {
            *by.entry(item.reason.as_str()).or_default() += 1;
        }
        let parts: Vec<String> = by
            .iter()
            .map(|(reason, n)| format!("{reason}: {n}"))
            .collect();
        out.push_str(&format!(
            "explain queue: {} card(s) {} · prompt {version}",
            items.len(),
            parts.join(", ")
        ));
    }
    out.push('\n');
    ok(out, 0)
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

// ---------- drill next ----------

/// `jobs.strategy.CORE_TOPICS`.
const CORE_TOPICS: &[&str] = &["data structures", "system design", "behavioral"];

/// Python's truth value for a plain value.
fn falsy(value: &Canon) -> bool {
    match value {
        Canon::Null => true,
        Canon::Bool(value) => !value,
        Canon::Int(value) => value.to_string() == "0",
        Canon::Float(value) => *value == 0.0,
        Canon::Str(text) => text.is_empty(),
        Canon::List(items) => items.is_empty(),
        Canon::Map(pairs) => pairs.is_empty(),
        Canon::Date(_) | Canon::DateTime(_) => false,
    }
}

/// `YYYY-MM-DDTHH:MM[:SS]`, with `Z` or `+HH:MM` only after seconds: the
/// pinned clock forms Python and the domain crate read the same way.
/// Returns whether it carries an offset (then Python reads no zone).
fn pin_shape(pin: &str) -> Option<bool> {
    let b = pin.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    if b.len() < 16
        || !(digits(0..4)
            && b[4] == b'-'
            && digits(5..7)
            && b[7] == b'-'
            && digits(8..10)
            && b[10] == b'T'
            && digits(11..13)
            && b[13] == b':'
            && digits(14..16))
    {
        return None;
    }
    let seconds = b.len() >= 19 && b[16] == b':' && digits(17..19);
    let tail = &b[if seconds { 19 } else { 16 }..];
    match tail {
        [] => Some(false),
        _ if !seconds => None,
        [b'Z'] => Some(true),
        [sign, h1, h2, b':', m1, m2]
            if matches!(sign, b'+' | b'-')
                && [h1, h2, m1, m2].iter().all(|d| d.is_ascii_digit()) =>
        {
            Some(true)
        }
        _ => None,
    }
}

/// A zone name that Python's `ZoneInfo` (the system zone files) and the
/// domain crate both resolve to the same zone.
fn zone_ok(name: &str, resolved: &str) -> bool {
    let plain = !name.is_empty()
        && !name.starts_with('/')
        && name.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "_-+".contains(ch))
        });
    plain && resolved == name && Path::new("/usr/share/zoneinfo").join(name).is_file()
}

/// `calendar.now()` reads without an error: a pinned clock in a form both
/// sides read, and a zone setting Python's `ZoneInfo` loads. None refuses.
fn clock_ok(files: &Files) -> Option<()> {
    let zone_read = match std::env::var("CAREERBREW_NOW") {
        Err(std::env::VarError::NotPresent) => true,
        Ok(pin) => !pin_shape(&pin)?,
        Err(_) => return None,
    };
    if zone_read {
        let settings = files.yaml("settings.yaml")?;
        let configured = if falsy(&settings) {
            None
        } else if plain_map(&settings) {
            match get(&settings, "timezone") {
                None | Some(Canon::Null) => None,
                Some(Canon::Str(name)) if name.is_empty() => None,
                Some(Canon::Str(name)) => Some(name.clone()),
                Some(_) => return None,
            }
        } else {
            return None;
        };
        match configured {
            Some(name) => {
                if !zone_ok(&name, careerbrew_tutor::local_zone(&files.root).name()) {
                    return None;
                }
            }
            None => {
                let tz = std::env::var("TZ").unwrap_or_default();
                let name = tz.trim_start_matches(':');
                // An empty `TZ` reads `/etc/localtime` on both sides.
                if !name.is_empty() && !zone_ok(name, careerbrew_tutor::system_zone().name()) {
                    return None;
                }
            }
        }
    }
    // A pin the domain crate cannot read panics: `run` makes it a refusal.
    let _ = careerbrew_teacher::now_local(&files.root);
    Some(())
}

/// A value `json.dumps` writes the way `dump` does: text, whole numbers,
/// booleans, null, and lists and text-keyed maps of them.
fn dumpable(value: &Canon) -> bool {
    match value {
        // `render::py_string` escapes DEL, which `json.dumps` keeps.
        Canon::Str(text) => !text.contains('\u{7f}'),
        Canon::Null | Canon::Bool(_) | Canon::Int(_) => true,
        Canon::List(items) => items.iter().all(dumpable),
        Canon::Map(pairs) => {
            plain_map(value)
                && pairs.iter().all(|(key, item)| {
                    matches!(key, Canon::Str(key) if !key.contains('\u{7f}')) && dumpable(item)
                })
        }
        Canon::Float(_) | Canon::Date(_) | Canon::DateTime(_) => false,
    }
}

/// `json.dumps(value, indent=2)` (ASCII only) for a `dumpable` value.
fn dump(value: &Canon, level: usize, out: &mut String) {
    let pad = |level: usize| "  ".repeat(level);
    match value {
        Canon::Null => out.push_str("null"),
        Canon::Bool(true) => out.push_str("true"),
        Canon::Bool(false) => out.push_str("false"),
        Canon::Int(value) => out.push_str(&value.to_string()),
        Canon::Str(text) => crate::render::py_string(text, out),
        Canon::List(items) if items.is_empty() => out.push_str("[]"),
        Canon::List(items) => {
            out.push_str("[\n");
            for (at, item) in items.iter().enumerate() {
                out.push_str(&pad(level + 1));
                dump(item, level + 1, out);
                out.push_str(if at + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push(']');
        }
        Canon::Map(pairs) if pairs.is_empty() => out.push_str("{}"),
        Canon::Map(pairs) => {
            out.push_str("{\n");
            for (at, (key, item)) in pairs.iter().enumerate() {
                out.push_str(&pad(level + 1));
                if let Canon::Str(key) = key {
                    crate::render::py_string(key, out);
                }
                out.push_str(": ");
                dump(item, level + 1, out);
                out.push_str(if at + 1 < pairs.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(level));
            out.push('}');
        }
        Canon::Float(_) | Canon::Date(_) | Canon::DateTime(_) => {}
    }
}

/// `tutor.rubrics()` from the checkout's `knowledge/rubrics/` (plugins are
/// a redaction input, refused earlier): id to rubric, the last file wins.
fn rubrics(repo: &Path) -> Option<Vec<(String, Canon)>> {
    let dir = repo.join("knowledge/rubrics");
    let mut out: Vec<(String, Canon)> = Vec::new();
    if !dir.is_dir() {
        return Some(out);
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().into_string().ok()?;
        if !(name.ends_with(".yaml") || name.ends_with(".yml")) {
            continue;
        }
        // A hidden name's suffix differs in `pathlib`; such a file goes to
        // Python.
        if name.starts_with('.') {
            return None;
        }
        if entry.path().is_file() {
            names.push(name);
        }
    }
    names.sort();
    for name in names {
        let data = repo_yaml(&dir.join(&name))?;
        let data = if falsy(&data) {
            Canon::Map(Vec::new())
        } else if plain_map(&data) {
            data
        } else {
            return None;
        };
        let stem = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem);
        let id = match get(&data, "id") {
            None => stem.to_owned(),
            Some(Canon::Str(id)) => id.clone(),
            Some(_) => return None,
        };
        match out.iter_mut().find(|(known, _)| *known == id) {
            Some((_, old)) => *old = data,
            None => out.push((id, data)),
        }
    }
    Some(out)
}

/// `careerbrew drill next` while no item has a card yet (no
/// `tutor/cards.json` and no `tutor/attempts.jsonl`): every mastery is 0,
/// so the next item is the new focus item with the smallest id.
fn drill_next(args: &Parsed, files: &Files) -> Option<Outcome> {
    clock_ok(files)?;
    let repo = repo()?;
    if repo.join("knowledge/knowledge").exists() {
        return None;
    }
    for name in ["tutor/cards.json", "tutor/attempts.jsonl"] {
        if files.root.join(name).symlink_metadata().is_ok() {
            return None;
        }
    }
    let plan = files.json("jobs/strategy.json")?;
    let mut focus: Vec<String> = CORE_TOPICS.iter().map(|t| (*t).to_owned()).collect();
    if !falsy(&plan) {
        if !plain_map(&plan) {
            return None;
        }
        match get(&plan, "skill_gaps") {
            None => {}
            Some(Canon::List(gaps)) => {
                for gap in gaps {
                    match gap {
                        Canon::List(pair) if pair.len() == 2 => match &pair[0] {
                            Canon::Str(skill) => focus.push(skill.clone()),
                            _ => return None,
                        },
                        _ => return None,
                    }
                }
            }
            Some(_) => return None,
        }
    }
    // `tutor.items()`: base items, then generated gap items; a later item
    // with a known id replaces the value in place.
    let mut items: Vec<(String, Canon)> = Vec::new();
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
            text(&item, "topic")?;
            let Canon::Map(mut pairs) = item else {
                return None;
            };
            set(&mut pairs, "origin", Canon::Str(origin.into()));
            let item = Canon::Map(pairs);
            match items.iter_mut().find(|(known, _)| *known == id) {
                Some((_, old)) => *old = item,
                None => items.push((id, item)),
            }
        }
    }
    let in_focus = |item: &Canon| text(item, "topic").is_some_and(|t| focus.iter().any(|f| f == t));
    let pool: Vec<&(String, Canon)> = if items.iter().any(|(_, item)| in_focus(item)) {
        items.iter().filter(|(_, item)| in_focus(item)).collect()
    } else {
        items.iter().collect()
    };
    let out = match pool.into_iter().min_by(|a, b| a.0.cmp(&b.0)) {
        None => "nothing due and no new items. Come back when reviews are due.\n".to_owned(),
        Some((id, item)) => drill_item(args, &repo, id, item)?,
    };
    // `paths.sub` makes the parents of the files Python reads; after every
    // check, so a refusal never follows it.
    for dir in ["jobs", "tutor"] {
        std::fs::create_dir_all(files.root.join(dir)).ok()?;
    }
    ok(out, 0)
}

/// The printed next item: `json.dumps` with `--json`, else `_print_item`
/// and the grading hint.
fn drill_item(args: &Parsed, repo: &Path, id: &str, item: &Canon) -> Option<String> {
    let Canon::Map(mut pairs) = item.clone() else {
        return None;
    };
    set(&mut pairs, "mode", Canon::Str("learn".into()));
    let item = Canon::Map(pairs);
    let mut out = String::new();
    if args.flag("json") {
        if !dumpable(&item) {
            return None;
        }
        dump(&item, 0, &mut out);
        out.push('\n');
        return Some(out);
    }
    let topic = text(&item, "topic")?;
    let round = text(&item, "round")?;
    let prompt = text(&item, "prompt")?;
    let teach = match get(&item, "teach") {
        None | Some(Canon::Null) => None,
        Some(Canon::Str(teach)) if teach.is_empty() => None,
        Some(Canon::Str(teach)) => Some(teach.as_str()),
        Some(_) => return None,
    };
    let rubric_id = text(&item, "rubric")?;
    let rubrics = rubrics(repo)?;
    let (_, rubric) = rubrics.iter().find(|(known, _)| known == rubric_id)?;
    let Some(Canon::List(criteria)) = get(rubric, "criteria") else {
        return None;
    };
    let mut parts = Vec::new();
    for criterion in criteria {
        if !plain_map(criterion) {
            return None;
        }
        parts.push(format!(
            "{}: {}",
            text(criterion, "id")?,
            text(criterion, "text")?
        ));
    }
    out.push_str(&format!(
        "[learn] {id} \u{b7} topic: {topic} \u{b7} round: {round}\n"
    ));
    if let Some(teach) = teach {
        out.push_str(&format!("Lesson: {teach}\n"));
    }
    out.push_str(&format!("Question: {prompt}\n"));
    out.push_str(&format!("Rubric (binary): {}\n", parts.join("; ")));
    out.push_str(&format!(
        "grade with: careerbrew drill answer {id} --met <criterion,ids>\n"
    ));
    Some(out)
}
