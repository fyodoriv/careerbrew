//! Native CLI handlers owned by `rust-19` (see `steps/mod.rs`): the job
//! choices, the application log, the remote preset, the first story of
//! the prep bank, and the interview knowledge readers (`interview`,
//! `interview-patterns`, `research stale`, `jobs interview-gaps`).
//!
//! The knowledge readers read the tracked `knowledge/` files of the checkout
//! this binary was built from (`rust18::same_repo`). An installed plugin
//! (its lock file is a redaction input) can add company files, so such a
//! run refuses. A value Python would print, sort or compare differently (a
//! non-ASCII name, a float outside the plain range, an odd shape) refuses.
//!
//! `jobs` and `prep` are personal-only: on a corporate machine Python raises
//! `RoleError` and logs `cli.error`, so those runs refuse. Every refusal comes
//! before any write. YAML is written only where the PyYAML text is fixed
//! (the preset) or built from plain words this module checks (the first
//! story); any other YAML write stays Python, as the vault emitter indents
//! lists differently from `yaml.safe_dump`.
use super::rust18::{emit, ok, py_now, py_now_utc, read_map_yaml, same_repo};
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_vault::canon::Canon;
use std::path::{Path, PathBuf};

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust19.rs`.
pub const COMMANDS: &[&str] = &[
    "jobs.choose",
    "jobs.unchoose",
    "jobs.log",
    "jobs.preset",
    "prep.story",
    "interview",
    "interview-patterns",
    "research.stale",
    "jobs.interview-gaps",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() || !personal(ctx) {
        return None;
    }
    let store_ctx = careerbrew_vault::transfer::Context::from_env().ok()?;
    let io = Io { ctx: &store_ctx };
    match name {
        "jobs.choose" => jobs_choose(args, ctx, &io),
        "jobs.unchoose" => jobs_unchoose(args, ctx, &io),
        "jobs.log" => jobs_log(args, ctx, &io),
        "jobs.preset" => jobs_preset(args, ctx, &io),
        "prep.story" => prep_story(args, ctx, &io),
        "interview" if same_repo() => interview(args, ctx, &io),
        "interview-patterns" if same_repo() => interview_patterns(args, &io),
        "research.stale" if same_repo() => research_stale(args, ctx, &io),
        "jobs.interview-gaps" if same_repo() => jobs_interview_gaps(&io),
        _ => None,
    }
}

/// `machine.guard(group)` passes: the role reads as personal. A truthy
/// `machine.yaml` that is not a mapping crashes `machine.info()` in Python.
fn personal(ctx: &Ctx) -> bool {
    read_map_yaml(&ctx.root.join("machine.yaml")).is_some()
        && careerbrew_setup::machine::role().is_ok_and(|role| role != "corporate")
}

struct Io<'a> {
    ctx: &'a careerbrew_vault::transfer::Context,
}

impl Io<'_> {
    /// `paths.sub(*parts)`: the parent folder is made even for a read.
    fn sub(&self, parts: &[&str]) -> Option<PathBuf> {
        let mut path = self.ctx.root.clone();
        for part in parts {
            path.push(part);
        }
        std::fs::create_dir_all(path.parent()?).ok()?;
        Some(path)
    }

    fn read_json(&self, path: &Path) -> Option<Canon> {
        self.ctx
            .store()
            .read_json(path, Canon::Map(Vec::new()), true)
            .ok()
    }

    fn write_json(&self, path: &Path, value: &Canon) -> Option<()> {
        self.ctx.store().write_json(path, value).ok()
    }

    fn write_text(&self, path: &Path, text: &str) -> Option<()> {
        self.ctx.store().write_text(path, text).ok()
    }
}

/// Python truthiness of a stored value.
fn truthy(value: &Canon) -> bool {
    match value {
        Canon::Null => false,
        Canon::Bool(b) => *b,
        Canon::Int(n) => n.to_string() != "0",
        Canon::Float(f) => *f != 0.0,
        Canon::Str(s) | Canon::Date(s) | Canon::DateTime(s) => !s.is_empty(),
        Canon::List(items) => !items.is_empty(),
        Canon::Map(items) => !items.is_empty(),
    }
}

/// `value or {}` as mapping pairs, or None for a truthy non-mapping.
fn pairs(value: Canon) -> Option<Vec<(Canon, Canon)>> {
    match value {
        Canon::Map(items) => Some(items),
        other if !truthy(&other) => Some(Vec::new()),
        _ => None,
    }
}

// ---------- jobs choose / unchoose ----------

const POSTING_FIELDS: &[&str] = &[
    "id",
    "company",
    "title",
    "location",
    "remote",
    "url",
    "description",
    "salary_min",
    "salary_max",
    "seniority",
    "skills",
    "origin",
    "tier_hint",
    "region",
    "source_name",
];
const POSTING_REQUIRED: &[&str] = &[
    "id",
    "company",
    "title",
    "location",
    "remote",
    "url",
    "description",
];

/// The posting ids `sources.load()` builds, when every stored posting is one
/// `Posting(**v)` accepts with a text id; else None.
fn posting_ids(io: &Io) -> Option<Vec<String>> {
    let path = io.sub(&["jobs", "postings.json"])?;
    let mut ids = Vec::new();
    for (_, posting) in pairs(io.read_json(&path)?)? {
        let Canon::Map(fields) = posting else {
            return None;
        };
        let mut keys = Vec::new();
        for (key, _) in &fields {
            let Canon::Str(key) = key else { return None };
            if !POSTING_FIELDS.contains(&key.as_str()) || keys.contains(key) {
                return None;
            }
            keys.push(key.clone());
        }
        if POSTING_REQUIRED
            .iter()
            .any(|k| !keys.iter().any(|have| have == k))
        {
            return None;
        }
        let id = fields.iter().find_map(|(k, v)| match (k, v) {
            (Canon::Str(k), Canon::Str(v)) if k == "id" => Some(v.clone()),
            _ => None,
        })?;
        ids.push(id);
    }
    Some(ids)
}

/// `strategy.chosen()` as ordered pairs with text keys.
fn chosen(io: &Io) -> Option<(PathBuf, Vec<(Canon, Canon)>)> {
    let path = io.sub(&["jobs", "chosen.json"])?;
    let items = pairs(io.read_json(&path)?)?;
    for (i, (key, _)) in items.iter().enumerate() {
        if !matches!(key, Canon::Str(_)) || items[..i].iter().any(|(k, _)| k == key) {
            return None;
        }
    }
    Some((path, items))
}

/// True when `str(KeyError(msg)).strip("'\"")` gives `msg` back: plain
/// printable ASCII with no quote or backslash, so `repr` adds only quotes.
fn plain_ascii(text: &str) -> bool {
    text.bytes()
        .all(|b| (b' '..=b'~').contains(&b) && b != b'\'' && b != b'"' && b != b'\\')
}

fn jobs_choose(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let posting = args.text("posting")?;
    let tier = args.text("tier")?;
    if !["dream", "target", "practice"].contains(&tier) {
        return None;
    }
    let ids = posting_ids(io)?;
    if !ids.iter().any(|id| id == posting) {
        if !plain_ascii(posting) {
            return None;
        }
        return ok(
            format!("unknown posting {posting} (see `careerbrew jobs list`)\n"),
            1,
        );
    }
    let (path, mut items) = chosen(io)?;
    let key = Canon::Str(posting.to_owned());
    let value = Canon::Str(tier.to_owned());
    match items.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => items.push((key, value)),
    }
    let count = items.len();
    io.write_json(&path, &Canon::Map(items))?;
    emit(ctx, "jobs.choose", &[("status", "ok"), ("tier", tier)]);
    ok(
        format!(
            "chosen: {posting} as {tier} ({count} job(s) chosen). Run `careerbrew jobs strategy` to re-plan.\n"
        ),
        0,
    )
}

fn jobs_unchoose(args: &Parsed, _ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let posting = args.text("posting")?;
    let (path, items) = chosen(io)?;
    let key = Canon::Str(posting.to_owned());
    let kept: Vec<(Canon, Canon)> = items.into_iter().filter(|(k, _)| *k != key).collect();
    let count = kept.len();
    io.write_json(&path, &Canon::Map(kept))?;
    ok(format!("removed {posting}; {count} job(s) chosen\n"), 0)
}

// ---------- jobs log ----------

const STAGES: &[&str] = &[
    "applied",
    "screen",
    "onsite",
    "offer",
    "rejected",
    "withdrawn",
];

fn jobs_log(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let company = args.text("company")?;
    let tier = args.text("tier")?;
    let stage = args.text("stage")?;
    if !STAGES.contains(&stage) || !["practice", "target", "dream"].contains(&tier) {
        return None;
    }
    let at = careerbrew_career::when::iso_seconds(&py_now_utc(ctx)?);
    let path = io.sub(&["jobs", "applications.jsonl"])?;
    let s = |text: &str| Canon::Str(text.to_owned());
    let row = Canon::Map(vec![
        (s("company"), s(company)),
        (s("tier"), s(tier)),
        (s("stage"), s(stage)),
        (s("at"), s(&at)),
        (s("origin"), s("user")),
    ]);
    let context = io.ctx;
    let store = context.store();
    store.append_jsonl(&path, &[row]).ok()?;
    emit(
        ctx,
        "jobs.application",
        &[("status", stage), ("tier", tier)],
    );
    ok(format!("logged {tier} {stage}\n"), 0)
}

// ---------- jobs preset ----------

/// `yaml.safe_dump(platforms.REMOTE_PRESET, sort_keys=False, allow_unicode=True)`.
const REMOTE_PRESET_YAML: &str = "sources:\n- kind: himalayas\n  q: software engineer\n- kind: remotive\n  category: software-development\n- kind: remoteok\n  tags:\n  - dev\n  - engineer\n  - backend\n  - frontend\n  - python\n  - typescript\n- kind: wwr\n  feed: programming\n- kind: hn\n  must_mention:\n  - remote\n- kind: adzuna\n  what: software engineer\n  app_id: keychain:careerbrew.adzuna.id\n  app_key: keychain:careerbrew.adzuna.key\n";

/// The same dump of `{} | platforms.REMOTE_CRITERIA`.
const REMOTE_CRITERIA_YAML: &str = "regions_ok:\n- global\n";

/// True when `pathlib.Path` prints `path` unchanged (copied from
/// `crate::routed`, which keeps it private).
fn plain_path(path: &str) -> bool {
    !(path.contains("//")
        || path.contains("/./")
        || path.starts_with("./")
        || path.ends_with("/.")
        || (path.len() > 1 && path.ends_with('/'))
        || path == ".")
}

fn jobs_preset(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let src_path = io.sub(&["jobs", "sources.yaml"])?;
    let crit_path = io.sub(&["jobs", "criteria.yaml"])?;
    let sources = read_map_yaml(&src_path)?;
    let current = sources.get("sources");
    let count = match current.and_then(|value| value.as_array()) {
        Some(items) => items.len(),
        None if !careerbrew_career::truthy(current) => 0,
        None => return None,
    };
    if count > 0 && !args.flag("force") {
        let shown = src_path.to_str()?;
        if !plain_path(ctx.root.to_str()?) {
            return None;
        }
        return ok(
            format!(
                "{shown} already lists {count} source(s); rerun with --force to replace them\n"
            ),
            1,
        );
    }
    // The criteria merge keeps the file's other keys; their YAML stays
    // Python. Only an empty or missing file is served (checked before any
    // write).
    let crit = read_map_yaml(&crit_path)?;
    if !crit.is_empty() {
        return None;
    }
    io.write_text(&src_path, REMOTE_PRESET_YAML)?;
    io.write_text(&crit_path, REMOTE_CRITERIA_YAML)?;
    let out = [
        "Remote preset written: Himalayas, Remotive, Remote OK, We Work Remotely, HN Who is hiring, Adzuna (set its country and where for roles near you)",
        "criteria: regions global (remote roles limited to regions you did not allow are filtered out; a remote role whose location is only a country name reads as global unless that country is one of your labels in regions_ok)",
        "Adzuna needs a free key: careerbrew secret set careerbrew.adzuna.id && careerbrew secret set careerbrew.adzuna.key",
    ];
    ok(out.join("\n") + "\n", 0)
}

// ---------- prep story ----------

/// True when PyYAML writes `text` as a plain scalar on one line, unquoted:
/// ASCII letters, digits and single spaces, starting with a letter and
/// ending with a letter or digit, short, and no YAML 1.1 bool or null word.
fn plain_word(text: &str) -> bool {
    let bytes = text.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    let words = ["yes", "no", "true", "false", "on", "off", "null", "y", "n"];
    text.len() <= 50
        && first.is_ascii_alphabetic()
        && last.is_ascii_alphanumeric()
        && !text.contains("  ")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b' ')
        && !words.contains(&text.to_ascii_lowercase().as_str())
}

/// A story field: `''` when empty, else the plain word.
fn scalar(text: &str) -> Option<String> {
    if text.is_empty() {
        Some("''".into())
    } else if plain_word(text) {
        Some(text.to_owned())
    } else {
        None
    }
}

fn prep_story(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let path = io.sub(&["prep", "stories.yaml"])?;
    // Only the first story is served: the YAML of stories already in the
    // bank is re-dumped by PyYAML, which stays Python.
    if path.exists() || path.is_symlink() {
        return None;
    }
    let title = scalar(args.text("title")?)?;
    if title == "''" {
        return None;
    }
    let competencies: Vec<String> = args
        .text("competencies")
        .unwrap_or("")
        .split(',')
        .filter(|c| !c.is_empty())
        .map(|c| plain_word(c).then(|| c.to_owned()))
        .collect::<Option<_>>()?;
    let field = |name: &str| scalar(args.text(name).unwrap_or(""));
    let (situation, action, result) = (field("situation")?, field("action")?, field("result")?);
    let added = careerbrew_career::when::strftime(&py_now_utc(ctx)?, "%Y-%m-%d");
    let comps = if competencies.is_empty() {
        " []\n".to_owned()
    } else {
        competencies
            .iter()
            .map(|c| format!("\n  - {c}"))
            .collect::<String>()
            + "\n"
    };
    let text = format!(
        "stories:\n- id: s01\n  title: {title}\n  competencies:{comps}  situation: {situation}\n  action: {action}\n  result: {result}\n  added: '{added}'\n"
    );
    io.write_text(&path, &text)?;
    ok("story s01 saved (1 in the bank)\n".into(), 0)
}

// ---------- interview knowledge (interview.py, research.py) ----------

type Pairs = Vec<(Canon, Canon)>;

/// `paths.REPO_ROOT / "knowledge"` of the checkout this binary was built from.
fn knowledge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knowledge")
}

/// `paths.extra_knowledge()`.
fn extra_knowledge() -> bool {
    std::env::var("CAREERBREW_EXTRA_KNOWLEDGE").map_or(true, |value| value != "0")
}

/// A dict lookup by text key (the maps hold each key once, as a dict does).
fn get<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Option<&'a Canon> {
    pairs.iter().find_map(|(k, v)| match k {
        Canon::Str(name) if name == key => Some(v),
        _ => None,
    })
}

/// `repr(float)` for a finite value Python prints without an exponent;
/// None for any other value.
fn float_repr(value: f64) -> Option<String> {
    if value == 0.0 {
        return Some(
            if value.is_sign_negative() {
                "-0.0"
            } else {
                "0.0"
            }
            .into(),
        );
    }
    if !(1e-4..1e16).contains(&value.abs()) {
        return None;
    }
    let text = format!("{value}");
    Some(if text.contains('.') {
        text
    } else {
        text + ".0"
    })
}

/// `str(value)` in an f-string, for the scalars whose text is certain.
fn py_text(value: &Canon) -> Option<String> {
    Some(match value {
        Canon::Null => "None".into(),
        Canon::Bool(true) => "True".into(),
        Canon::Bool(false) => "False".into(),
        Canon::Int(n) => n.to_string(),
        Canon::Float(f) => float_repr(*f)?,
        Canon::Str(s) | Canon::Date(s) => s.clone(),
        _ => return None,
    })
}

/// `str(d.get(key, default))`.
fn text_or(pairs: &[(Canon, Canon)], key: &str, default: &str) -> Option<String> {
    match get(pairs, key) {
        None => Some(default.to_owned()),
        Some(value) => py_text(value),
    }
}

/// A list of text items (`d.get(key, [])` iterated as text); a missing key is
/// empty, any other shape refuses.
fn texts(value: Option<&Canon>) -> Option<Vec<String>> {
    match value {
        None => Some(Vec::new()),
        Some(Canon::List(items)) => items
            .iter()
            .map(|item| match item {
                Canon::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// A list of mappings (`d.get(key, [])` iterated as dicts).
fn maps(value: Option<&Canon>) -> Option<Vec<&Pairs>> {
    match value {
        None => Some(Vec::new()),
        Some(Canon::List(items)) => items
            .iter()
            .map(|item| match item {
                Canon::Map(pairs) => Some(pairs),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// `(d.get(key) or {})` as a mapping.
fn map_or_empty(value: Option<&Canon>) -> Option<Pairs> {
    match value {
        None => Some(Vec::new()),
        Some(value) => pairs(value.clone()),
    }
}

/// Text Python casefolds the same way as ASCII lowercase.
fn ascii_fold(text: &str) -> Option<String> {
    text.is_ascii().then(|| text.to_ascii_lowercase())
}

/// `store.read_yaml(path, {}) or {}` as a mapping (a knowledge file).
fn yaml_map(io: &Io, path: &Path) -> Option<Pairs> {
    let value = io
        .ctx
        .store()
        .read_yaml(path, Canon::Map(Vec::new()), true)
        .ok()?;
    pairs(value)
}

/// `interview.sources()`: id to source mapping, later files overriding.
fn sources(io: &Io) -> Option<Vec<(String, Pairs)>> {
    let files: &[&str] = if extra_knowledge() {
        &["sources-extra.yaml", "sources.yaml"]
    } else {
        &["sources.yaml"]
    };
    let mut out: Vec<(String, Pairs)> = Vec::new();
    for file in files {
        let data = yaml_map(io, &knowledge().join(file))?;
        for (key, value) in map_or_empty(get(&data, "sources"))? {
            let (Canon::Str(key), Canon::Map(value)) = (key, value) else {
                return None;
            };
            match out.iter_mut().find(|(have, _)| *have == key) {
                Some(slot) => slot.1 = value,
                None => out.push((key, value)),
            }
        }
    }
    Some(out)
}

/// `interview._company_files()` without plugin files (a plugin makes the run
/// refuse): every `*.yaml` in the company folders, in `Path` order.
fn company_files() -> Option<Vec<PathBuf>> {
    let base = knowledge();
    // `plugins.resources("knowledge")` also lists this base folder.
    if base.join("knowledge").symlink_metadata().is_ok() {
        return None;
    }
    let dirs: &[&str] = if extra_knowledge() {
        &["companies-extra", "companies"]
    } else {
        &["companies"]
    };
    let mut found: Vec<(String, String)> = Vec::new();
    for dir in dirs {
        let path = base.join(dir);
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            _ => return None,
        }
        for entry in std::fs::read_dir(&path).ok()? {
            let name = entry.ok()?.file_name().into_string().ok()?;
            if !name.ends_with(".yaml") {
                continue;
            }
            // A dot name has an odd `Path.stem`; a folder or a broken link
            // named `*.yaml` makes Python fail.
            if name.starts_with('.') || !std::fs::metadata(path.join(&name)).ok()?.is_file() {
                return None;
            }
            found.push(((*dir).to_owned(), name));
        }
    }
    found.sort();
    Some(
        found
            .into_iter()
            .map(|(dir, name)| base.join(dir).join(name))
            .collect(),
    )
}

/// `interview.companies()`: casefolded id to company mapping.
fn companies(io: &Io) -> Option<Vec<(String, Pairs)>> {
    let mut out: Vec<(String, Pairs)> = Vec::new();
    for file in company_files()? {
        let data = yaml_map(io, &file)?;
        if !get(&data, "loop").is_some_and(truthy) {
            continue;
        }
        let id = match get(&data, "id") {
            None => {
                let name = file.file_name()?.to_str()?;
                name.strip_suffix(".yaml")?.to_owned()
            }
            Some(Canon::Str(id)) => id.clone(),
            Some(Canon::Int(id)) => id.to_string(),
            _ => return None,
        };
        let key = ascii_fold(&id)?;
        match out.iter_mut().find(|(have, _)| *have == key) {
            Some(slot) => slot.1 = data,
            None => out.push((key, data)),
        }
    }
    Some(out)
}

/// `interview.find_company(name)`: Some(None) when no company matches.
fn find_company<'a>(cs: &'a [(String, Pairs)], name: &str) -> Option<Option<&'a Pairs>> {
    let key = ascii_fold(name)?;
    let key = key.trim_matches(' ');
    if let Some((_, c)) = cs.iter().find(|(k, _)| k == key) {
        return Some(Some(c));
    }
    for (_, c) in cs {
        let folded = match get(c, "name") {
            None => String::new(),
            Some(Canon::Str(name)) => ascii_fold(name)?,
            _ => return None,
        };
        if folded == key {
            return Some(Some(c));
        }
    }
    Some(None)
}

/// The step ids of a company loop (`[s["step"] for s in c["loop"]]`).
fn loop_steps(c: &[(Canon, Canon)]) -> Option<Vec<(String, &Pairs)>> {
    maps(Some(get(c, "loop")?))?
        .into_iter()
        .map(|s| match get(s, "step") {
            Some(Canon::Str(step)) => Some((step.clone(), s)),
            _ => None,
        })
        .collect()
}

/// `round(k / n, 2)`, or None on a decimal tie at the third place (where
/// Python's half-even rounding of the exact value needs care).
fn round2(k: usize, n: usize) -> Option<f64> {
    let scaled = k * 200;
    if scaled.is_multiple_of(n) && !(scaled / n).is_multiple_of(2) {
        return None;
    }
    format!("{:.2}", k as f64 / n as f64).parse().ok()
}

struct Patterns {
    companies: usize,
    share: Vec<(String, f64)>,
    typical: Vec<String>,
    transitions: Vec<(String, String, i64)>,
    ai: Vec<(String, i64)>,
    shared: Canon,
}

fn bump<K: PartialEq>(counts: &mut Vec<(K, i64)>, key: K) {
    match counts.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 += 1,
        None => counts.push((key, 1)),
    }
}

/// `interview.patterns(0.4)`.
fn patterns(io: &Io) -> Option<Patterns> {
    let cs = companies(io)?;
    let n = cs.len().max(1);
    let mut freq: Vec<(String, i64)> = Vec::new();
    let mut positions: Vec<(String, Vec<f64>)> = Vec::new();
    let mut pairs_seen: Vec<((String, String), i64)> = Vec::new();
    let mut ai: Vec<(String, i64)> = Vec::new();
    for (_, c) in &cs {
        let steps: Vec<String> = loop_steps(c)?.into_iter().map(|(s, _)| s).collect();
        let denom = steps.len().saturating_sub(1).max(1) as f64;
        let mut unique: Vec<&String> = Vec::new();
        for step in &steps {
            if !unique.contains(&step) {
                unique.push(step);
            }
        }
        for (i, step) in unique.iter().enumerate() {
            bump(&mut freq, (*step).clone());
            let at = i as f64 / denom;
            match positions.iter_mut().find(|(k, _)| k == *step) {
                Some(slot) => slot.1.push(at),
                None => positions.push(((*step).clone(), vec![at])),
            }
        }
        for pair in steps.windows(2) {
            bump(&mut pairs_seen, (pair[0].clone(), pair[1].clone()));
        }
        let policy = match get(c, "ai_policy") {
            Some(value) if truthy(value) => match value {
                Canon::Map(policy) => text_or(policy, "allowed", "unknown")?,
                _ => return None,
            },
            _ => "unknown".to_owned(),
        };
        bump(&mut ai, policy);
    }
    let mut ranked = freq.clone();
    ranked.sort_by_key(|(_, count)| -count);
    let median = |step: &str| -> f64 {
        let mut values = positions
            .iter()
            .find(|(k, _)| k == step)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        values.sort_by(|a, b| a.total_cmp(b));
        values[values.len() / 2]
    };
    let mut typical: Vec<(String, f64)> = ranked
        .iter()
        .filter(|(_, count)| *count as f64 / n as f64 >= 0.4)
        .map(|(step, _)| (step.clone(), median(step)))
        .collect();
    typical.sort_by(|a, b| a.1.total_cmp(&b.1));
    let share = ranked
        .iter()
        .map(|(step, count)| Some((step.clone(), round2(*count as usize, n)?)))
        .collect::<Option<Vec<_>>>()?;
    pairs_seen.sort_by_key(|(_, count)| -count);
    let transitions = pairs_seen
        .into_iter()
        .take(8)
        .map(|((a, b), k)| (a, b, k))
        .collect();
    let strategy = yaml_map(io, &knowledge().join("strategy.yaml"))?;
    let shared = get(&strategy, "ai_shared_patterns")
        .cloned()
        .unwrap_or(Canon::List(Vec::new()));
    Some(Patterns {
        companies: cs.len(),
        share,
        typical: typical.into_iter().map(|(s, _)| s).collect(),
        transitions,
        ai,
        shared,
    })
}

const DEFAULT_LOOP: &[&str] = &[
    "recruiter-screen",
    "technical-phone-screen",
    "coding-onsite",
    "system-design",
    "behavioral",
    "offer-negotiation",
];

/// `interview.default_loop(seniority)` from computed patterns.
fn default_loop(pt: &Patterns, seniority: i64) -> Vec<String> {
    let order: Vec<String> = if pt.typical.is_empty() {
        DEFAULT_LOOP.iter().map(|s| (*s).to_owned()).collect()
    } else {
        pt.typical.clone()
    };
    let mut out: Vec<String> = order
        .into_iter()
        .filter(|s| s != "offer-negotiation" && (seniority >= 3 || s != "system-design"))
        .collect();
    if seniority >= 3 && !out.iter().any(|s| s == "system-design") {
        let at = out.len().saturating_sub(1);
        out.insert(at, "system-design".to_owned());
    }
    out.push("offer-negotiation".to_owned());
    out
}

/// `json.dumps(value, indent=2)` (insertion order, `ensure_ascii`).
enum J {
    Int(String),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
    List(Vec<J>),
    Map(Vec<(String, J)>),
}

/// A stored value as JSON, or None for one `json.dumps(default=list)` would
/// write differently or fail on (a date, a float, a non-text key).
fn j_of(value: &Canon) -> Option<J> {
    Some(match value {
        Canon::Null => J::Null,
        Canon::Bool(b) => J::Bool(*b),
        Canon::Int(n) => J::Int(n.to_string()),
        Canon::Str(s) => J::Str(s.clone()),
        Canon::List(items) => J::List(items.iter().map(j_of).collect::<Option<_>>()?),
        Canon::Map(pairs) => J::Map(
            pairs
                .iter()
                .map(|(k, v)| match k {
                    Canon::Str(k) => Some((k.clone(), j_of(v)?)),
                    _ => None,
                })
                .collect::<Option<_>>()?,
        ),
        _ => return None,
    })
}

fn j_write(value: &J, level: usize, out: &mut String) -> Option<()> {
    let pad = |level: usize| "  ".repeat(level);
    match value {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Int(n) => out.push_str(n),
        J::Float(f) => out.push_str(&float_repr(*f)?),
        J::Str(s) => crate::render::py_string(s, out),
        J::List(items) if items.is_empty() => out.push_str("[]"),
        J::Map(items) if items.is_empty() => out.push_str("{}"),
        J::List(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&pad(level + 1));
                j_write(item, level + 1, out)?;
            }
            out.push('\n');
            out.push_str(&pad(level));
            out.push(']');
        }
        J::Map(items) => {
            out.push_str("{\n");
            for (i, (key, item)) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&pad(level + 1));
                crate::render::py_string(key, out);
                out.push_str(": ");
                j_write(item, level + 1, out)?;
            }
            out.push('\n');
            out.push_str(&pad(level));
            out.push('}');
        }
    }
    Some(())
}

fn interview_patterns(args: &Parsed, io: &Io) -> Option<Outcome> {
    let pt = patterns(io)?;
    if args.flag("json") {
        let s = |text: &str| J::Str(text.to_owned());
        let value = J::Map(vec![
            ("companies".into(), J::Int(pt.companies.to_string())),
            (
                "step_share".into(),
                J::Map(
                    pt.share
                        .iter()
                        .map(|(k, v)| (k.clone(), J::Float(*v)))
                        .collect(),
                ),
            ),
            (
                "typical_order".into(),
                J::List(pt.typical.iter().map(|t| s(t)).collect()),
            ),
            (
                "transitions".into(),
                J::List(
                    pt.transitions
                        .iter()
                        .map(|(a, b, k)| J::List(vec![s(a), s(b), J::Int(k.to_string())]))
                        .collect(),
                ),
            ),
            (
                "ai_policy".into(),
                J::Map(
                    pt.ai
                        .iter()
                        .map(|(k, v)| (k.clone(), J::Int(v.to_string())))
                        .collect(),
                ),
            ),
            ("ai_shared_patterns".into(), j_of(&pt.shared)?),
        ]);
        let mut out = String::new();
        j_write(&value, 0, &mut out)?;
        out.push('\n');
        return ok(out, 0);
    }
    let shared = maps(Some(&pt.shared))?
        .into_iter()
        .map(|p| py_text(get(p, "text")?))
        .collect::<Option<Vec<_>>>()?;
    let mut out = format!("shared across {} sourced companies:\n", pt.companies);
    let share: Vec<String> = pt
        .share
        .iter()
        .map(|(s, v)| format!("{s} {:.0}%", v * 100.0))
        .collect();
    out += &format!("  how many companies use each step: {}\n", share.join(", "));
    out += &format!("  typical order: {}\n", pt.typical.join(" → "));
    let transitions: Vec<String> = pt
        .transitions
        .iter()
        .map(|(a, b, k)| format!("{a} → {b} ({k})"))
        .collect();
    out += &format!("  common transitions: {}\n", transitions.join("; "));
    let mut ai = pt.ai.clone();
    ai.sort();
    let ai: Vec<String> = ai.iter().map(|(k, v)| format!("{k}={v}")).collect();
    out += &format!("  AI tools allowed: {}\n", ai.join(", "));
    for text in shared {
        out += &format!("  AI pattern: {text}\n");
    }
    out += &format!(
        "  default loop for unknown companies (level 3): {}\n",
        default_loop(&pt, 3).join(" → ")
    );
    ok(out, 0)
}

// ---------- research stale ----------

/// `rows[:top]` with Python's slice bounds.
fn head<T>(rows: &[T], top: i64) -> &[T] {
    let len = rows.len() as i64;
    let end = if top >= 0 {
        top.min(len)
    } else {
        (len + top).max(0)
    };
    &rows[..end as usize]
}

/// The casefolded company names of `jobs/strategy.json` tiers.
fn pipeline(io: &Io) -> Option<Vec<String>> {
    let path = io.sub(&["jobs", "strategy.json"])?;
    let plan = pairs(io.read_json(&path)?)?;
    let mut out = Vec::new();
    for (_, rows) in map_or_empty(get(&plan, "tiers"))? {
        for row in maps(Some(&rows))? {
            let Some(Canon::Str(company)) = get(row, "company") else {
                return None;
            };
            out.push(ascii_fold(company)?);
        }
    }
    Some(out)
}

struct StaleRow {
    id: String,
    url: String,
    age: i64,
    companies: Vec<String>,
    in_pipeline: bool,
}

fn research_stale(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let days = args.int("days")?;
    let top = args.int("top")?;
    let today = py_now(ctx)?.date_naive();
    let srcs = sources(io)?;
    let mut used_by: Vec<(String, Vec<String>)> = Vec::new();
    for (cid, c) in companies(io)? {
        let mut ids = texts(get(&c, "sources"))?;
        ids.extend(texts(get(&map_or_empty(get(&c, "ai_policy"))?, "sources"))?);
        for s in maps(get(&c, "loop"))? {
            ids.extend(texts(get(s, "sources"))?);
        }
        let name = match get(&c, "name") {
            None => cid.clone(),
            Some(Canon::Str(name)) => name.clone(),
            _ => return None,
        };
        for id in ids {
            match used_by.iter_mut().find(|(have, _)| *have == id) {
                Some(slot) => slot.1.push(name.clone()),
                None => used_by.push((id, vec![name.clone()])),
            }
        }
    }
    let pipeline = pipeline(io)?;
    let mut rows: Vec<StaleRow> = Vec::new();
    for (sid, s) in &srcs {
        let checked = text_or(s, "checked", "1970-01-01")?;
        if checked.starts_with("0000") {
            return None;
        }
        let date = careerbrew_career::when::date(&checked).ok()?;
        let age = (today - date).num_days();
        if age > days {
            let url = py_text(get(s, "url")?)?;
            let mut names = used_by
                .iter()
                .find(|(have, _)| have == sid)
                .map(|(_, names)| names.clone())
                .unwrap_or_default();
            names.sort();
            names.dedup();
            let mut in_pipeline = false;
            for name in &names {
                in_pipeline |= pipeline.contains(&ascii_fold(name)?);
            }
            rows.push(StaleRow {
                id: sid.clone(),
                url,
                age,
                companies: names,
                in_pipeline,
            });
        }
    }
    rows.sort_by_key(|r| (!r.in_pipeline, -r.age));
    let mut out = format!("{} source(s) older than {days} days\n", rows.len());
    for r in head(&rows, top) {
        let flag = if r.in_pipeline { " [pipeline]" } else { "" };
        let names = r.companies.join(", ");
        let names = if names.is_empty() { "strategy" } else { &names };
        out += &format!("  {}d  {}{flag}  {}  ({names})\n", r.age, r.id, r.url);
    }
    if !rows.is_empty() {
        out.push_str(
            "re-check with the browser-research skill, then rerun scripts/import-research.py\n",
        );
    }
    ok(out, 0)
}

// ---------- jobs interview-gaps ----------

/// True for printable ASCII: Python's `strip` and `casefold` then act as
/// trimming spaces and ASCII lowercase.
fn printable(text: &str) -> bool {
    text.bytes().all(|b| (b' '..=b'~').contains(&b))
}

fn jobs_interview_gaps(io: &Io) -> Option<Outcome> {
    let path = io.sub(&["jobs", "strategy.json"])?;
    let plan = pairs(io.read_json(&path)?)?;
    let tiers = map_or_empty(get(&plan, "tiers"))?;
    let mut tiered: Vec<(&str, String, String)> = Vec::new();
    for tier in ["dream", "target", "practice"] {
        for row in maps(get(&tiers, tier))? {
            let Some(Canon::Str(company)) = get(row, "company") else {
                return None;
            };
            if !printable(company) {
                return None;
            }
            tiered.push((tier, company.clone(), text_or(row, "want", "0")?));
        }
    }
    // An empty plan falls back to the scored postings, which stay Python.
    if tiered.is_empty() {
        return None;
    }
    let cs = companies(io)?;
    let mut seen: Vec<String> = Vec::new();
    let mut missing: Vec<(&str, String, String)> = Vec::new();
    for (tier, company, want) in tiered {
        let key = company.to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if find_company(&cs, &company)?.is_none() {
            missing.push((tier, company, want));
        }
    }
    let mut out = format!(
        "{}/{} pipeline companies have a sourced interview loop\n",
        seen.len() - missing.len(),
        seen.len()
    );
    for (tier, company, want) in missing {
        out += &format!("  research next: {company} ({tier}, Want {want}) -> ask your agent: research how {company} interviews (company-research skill)\n");
    }
    ok(out, 0)
}

// ---------- interview ----------

struct Info<'a> {
    company: String,
    known: bool,
    steps: Vec<Pairs>,
    company_tips: Option<&'a Canon>,
    values: Option<&'a Canon>,
    remote_policy: Option<&'a Canon>,
    local_presence: Option<&'a Canon>,
    ai_policy: Option<&'a Canon>,
    ai_shared: Option<Canon>,
}

/// `{**all_steps.get(step, {}), "id": step, ...}`.
fn merged_step(all: &[(Canon, Canon)], step: &str, notes: Canon, srcs: Canon) -> Option<Pairs> {
    let mut out = match get(all, step) {
        None => Vec::new(),
        Some(Canon::Map(fields)) => fields.clone(),
        Some(_) => return None,
    };
    let s = |text: &str| Canon::Str(text.to_owned());
    careerbrew_vault::canon::insert(&mut out, s("id"), s(step));
    careerbrew_vault::canon::insert(&mut out, s("company_notes"), notes);
    careerbrew_vault::canon::insert(&mut out, s("company_sources"), srcs);
    Some(out)
}

/// `cite(ids)` of `interview.render`.
fn cite(
    ids: Option<&Canon>,
    with_sources: bool,
    src: &[(String, Pairs)],
    order: &mut Vec<String>,
) -> Option<String> {
    if !with_sources {
        return Some(String::new());
    }
    let mut nums = Vec::new();
    for id in texts(ids)? {
        if src.iter().any(|(k, _)| *k == id) {
            if !order.contains(&id) {
                order.push(id.clone());
            }
            let at = order.iter().position(|k| *k == id)?;
            nums.push((at + 1).to_string());
        }
    }
    Some(if nums.is_empty() {
        String::new()
    } else {
        format!(" [{}]", nums.join(","))
    })
}

fn truthy_map(value: Option<&Canon>) -> Option<Option<&Pairs>> {
    match value {
        Some(value) if truthy(value) => match value {
            Canon::Map(fields) => Some(Some(fields)),
            _ => None,
        },
        _ => Some(None),
    }
}

/// `interview.render(info, with_sources)`.
fn render(info: &Info, src: &[(String, Pairs)], with_sources: bool) -> Option<String> {
    let mut order: Vec<String> = Vec::new();
    let o = &mut order;
    let mut lines: Vec<String> = vec![format!("# Interview loop: {}", info.company), String::new()];
    if !info.known {
        lines.push("No sourced loop for this company yet: this is the shared default for the level, built from the steps most sourced companies use (see `careerbrew interview-patterns`). Run the company-research skill to add one.".into());
        lines.push(String::new());
    }
    if let Some(rp) = truthy_map(info.remote_policy)? {
        let text = text_or(rp, "text", "")?;
        let c = cite(get(rp, "sources"), with_sources, src, o)?;
        lines.push(format!("Remote policy: {text}{c}"));
    }
    if let Some(tp) = truthy_map(info.local_presence)? {
        let value = text_or(tp, "value", "None")?;
        let note = text_or(tp, "note", "")?;
        let c = cite(get(tp, "sources"), with_sources, src, o)?;
        lines.push(format!("Local presence: {value}. {note}{c}"));
    }
    if let Some(ap) = truthy_map(info.ai_policy)? {
        let allowed = text_or(ap, "allowed", "unknown")?;
        let summary = text_or(ap, "summary", "")?;
        let checked = text_or(ap, "checked", "?")?;
        let c = cite(get(ap, "sources"), with_sources, src, o)?;
        lines.push(format!(
            "AI tools in this process: {allowed}. {summary} (checked {checked}){c}"
        ));
    }
    if let Some(shared) = info.ai_shared.as_ref().filter(|v| truthy(v)) {
        lines.push(String::new());
        lines.push("AI tools: this company has no sourced AI rule yet. Ask the recruiter. What sourced companies share:".into());
        for p in maps(Some(shared))? {
            let text = py_text(get(p, "text")?)?;
            let c = cite(get(p, "sources"), with_sources, src, o)?;
            lines.push(format!("- {text}{c}"));
        }
    }
    if let Some(values) = info.values.filter(|v| truthy(v)) {
        lines.push(format!(
            "Published values or principles: {}",
            texts(Some(values))?.join("; ")
        ));
    }
    for (n, st) in info.steps.iter().enumerate() {
        let id = py_text(get(st, "id")?)?;
        let name = text_or(st, "name", &id)?;
        let minutes = text_or(st, "typical_minutes", "?")?;
        let description = match get(st, "description") {
            None => String::new(),
            Some(Canon::Str(text)) => text.clone(),
            Some(_) => return None,
        };
        let c = cite(get(st, "sources"), with_sources, src, o)?;
        lines.push(String::new());
        lines.push(format!("## {}. {name} (~{minutes} min)", n + 1));
        lines.push(description + &c);
        if get(st, "company_notes").is_some_and(truthy) {
            let notes = py_text(get(st, "company_notes")?)?;
            let c = cite(get(st, "company_sources"), with_sources, src, o)?;
            lines.push(format!("At {}: {notes}{c}", info.company));
        }
        if let Some(evaluated) = get(st, "evaluated").filter(|v| truthy(v)) {
            let parts: Vec<String> = texts(Some(evaluated))?
                .iter()
                .map(|e| format!("{}.", e.trim_end_matches('.')))
                .collect();
            lines.push(format!("They evaluate: {}", parts.join(" ")));
        }
        for t in maps(get(st, "tips"))? {
            lines.push(format!("- Tip: {}", py_text(get(t, "text")?)?));
        }
        for t in maps(get(st, "traps"))? {
            lines.push(format!("- Avoid: {}", py_text(get(t, "text")?)?));
        }
    }
    if let Some(tips) = info.company_tips.filter(|v| truthy(v)) {
        lines.push(String::new());
        lines.push("## Company tips".into());
        for t in maps(Some(tips))? {
            let text = py_text(get(t, "text")?)?;
            let c = cite(get(t, "sources"), with_sources, src, o)?;
            lines.push(format!("- {text}{c}"));
        }
    }
    if with_sources && !o.is_empty() {
        lines.push(String::new());
        lines.push("## Sources".into());
        for (k, id) in o.iter().enumerate() {
            let (_, s) = src.iter().find(|(have, _)| have == id)?;
            let title = py_text(get(s, "title")?)?;
            let url = py_text(get(s, "url")?)?;
            lines.push(format!("{}. {title}: {url}", k + 1));
        }
    }
    Some(lines.join("\n") + "\n")
}

fn interview(args: &Parsed, ctx: &Ctx, io: &Io) -> Option<Outcome> {
    let raw = args.text("company")?;
    if !printable(raw) {
        return None;
    }
    let company = raw.trim_matches(' ');
    // An empty name raises ValueError in Python.
    if company.is_empty() {
        return None;
    }
    let level = args.int("level")?;
    let show = args.flag("show");
    let with_sources = !args.flag("no_sources");
    let cs = companies(io)?;
    let steps_file = yaml_map(io, &knowledge().join("interview").join("steps.yaml"))?;
    let all = match get(&steps_file, "steps") {
        None => Vec::new(),
        Some(Canon::Map(all)) => all.clone(),
        Some(_) => return None,
    };
    let info = match find_company(&cs, company)? {
        Some(c) => {
            let mut steps = Vec::new();
            for (step, s) in loop_steps(c)? {
                let notes = get(s, "notes")
                    .cloned()
                    .unwrap_or(Canon::Str(String::new()));
                let srcs = get(s, "sources")
                    .cloned()
                    .unwrap_or(Canon::List(Vec::new()));
                steps.push(merged_step(&all, &step, notes, srcs)?);
            }
            Info {
                company: text_or(c, "name", company)?,
                known: true,
                steps,
                company_tips: get(c, "company_tips"),
                values: get(c, "values_or_principles"),
                remote_policy: get(c, "remote_policy"),
                local_presence: get(c, "local_presence"),
                ai_policy: get(c, "ai_policy"),
                ai_shared: None,
            }
        }
        None => {
            let pt = patterns(io)?;
            let steps = default_loop(&pt, level)
                .iter()
                .map(|id| merged_step(&all, id, Canon::Str(String::new()), Canon::List(Vec::new())))
                .collect::<Option<Vec<_>>>()?;
            Info {
                company: company.to_owned(),
                known: false,
                steps,
                company_tips: None,
                values: None,
                remote_policy: None,
                local_presence: None,
                ai_policy: None,
                ai_shared: Some(pt.shared),
            }
        }
    };
    let src = sources(io)?;
    let text = render(&info, &src, with_sources)?;
    let slug: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if !show && !plain_path(ctx.root.to_str()?) {
        return None;
    }
    let dir = ctx.root.join("out");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("interview-{slug}.md"));
    let shown = path.to_str()?.to_owned();
    std::fs::write(&path, &text).ok()?;
    let stdout = if show {
        text + "\n"
    } else {
        format!(
            "{}: {} step(s) ({}); full guide with tips and sources: {shown}\n",
            info.company,
            info.steps.len(),
            if info.known {
                "sourced company loop"
            } else {
                "shared default loop"
            }
        )
    };
    ok(stdout, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_are_only_the_unquoted_ones() {
        assert!(plain_word("Shipped thing") && plain_word("design") && plain_word("a1"));
        for text in [
            "", "yes", "Off", "1a", "a:b", "a  b", " a", "a ", "naïve", "a-b",
        ] {
            assert!(!plain_word(text), "{text:?}");
        }
    }

    #[test]
    fn the_preset_criteria_are_fixed() {
        assert!(REMOTE_PRESET_YAML.starts_with("sources:\n- kind: himalayas\n"));
        assert_eq!(REMOTE_CRITERIA_YAML, "regions_ok:\n- global\n");
    }
}
