//! Native CLI handlers owned by `rust-19` (see `steps/mod.rs`): the job
//! choices, the application log, the remote preset and the first story of
//! the prep bank.
//!
//! `jobs` and `prep` are personal-only: on a corporate machine Python raises
//! `RoleError` and logs `cli.error`, so those runs refuse. Every refusal comes
//! before any write. YAML is written only where the PyYAML text is fixed
//! (the preset) or built from plain words this module checks (the first
//! story); any other YAML write stays Python, as the vault emitter indents
//! lists differently from `yaml.safe_dump`.
use super::rust18::{emit, ok, py_now_utc, read_map_yaml};
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
