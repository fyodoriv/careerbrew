//! Native CLI handlers owned by `rust-18` (see `steps/mod.rs`): career
//! evidence notes, ladder listings, ladder import, promotion gaps and the
//! day's work plan, through the `careerbrew-career` component crate. The
//! imported ladder is written with `crate::pyyaml::safe_dump`.
//!
//! Every handler refuses (exit 64, Python runs it) before any write when:
//! - Python's `privacy.redact` would read an input (`Ctx::redact_inputs_present`),
//! - the clock or its zone cannot be read the way `calendar.now()` reads it,
//! - a stored value has a shape Python could read differently or crash on,
//! - or a domain call fails (Python prints its own `error:` line).
//!
//! The helpers marked `pub(super)` are shared with `rust19`.
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;
use careerbrew_career::{evidence, ladder_grade, promo, py, when};
use careerbrew_vault::canon::Canon;
use std::io::Write;
use std::path::Path;

/// `serde_json::Map` and `serde_json::Value`, the types the career crate
/// returns, named through a public alias (this crate's manifest does not
/// list `serde_json`, and step modules leave the manifest alone).
pub(super) type Map = careerbrew_voice::judge::Label;
pub(super) type Value = <Map as std::ops::Index<&'static str>>::Output;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust18.rs`.
pub const COMMANDS: &[&str] = &[
    "work.note",
    "work.gaps",
    "ladder.list",
    "ladder.levels",
    "ladder.import",
    "work.plan",
];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(name: &str, args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    if ctx.redact_inputs_present() || !same_repo() {
        return None;
    }
    match name {
        "work.note" => work_note(args, ctx),
        "work.gaps" => work_gaps(args, ctx),
        "ladder.list" => ladder_list(ctx),
        "ladder.levels" => ladder_levels(args, ctx),
        "ladder.import" => ladder_import(args, ctx),
        "work.plan" => work_plan(args, ctx),
        _ => None,
    }
}

pub(super) fn ok(stdout: String, code: i32) -> Option<Outcome> {
    Some(Outcome {
        stdout,
        stderr: String::new(),
        code,
        emit_event: true,
    })
}

/// The base ladders and knowledge files are read from the checkout this
/// binary was built from; the console script names its own checkout in
/// `CAREERBREW_NATIVE_REPO`. Another checkout goes to Python.
pub(super) fn same_repo() -> bool {
    let Some(repo) = std::env::var_os("CAREERBREW_NATIVE_REPO") else {
        return true;
    };
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match (Path::new(&repo).canonicalize(), built.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `store.read_yaml(path, {}) or {}` as a mapping, or None when Python would
/// crash on `.get` (a truthy value that is not a mapping) or the read fails.
pub(super) fn read_map_yaml(path: &Path) -> Option<Map> {
    let value = careerbrew_career::read_yaml(path, Map::new().into()).ok()?;
    as_map(&value)
}

/// `value or {}` as a mapping (None for a truthy non-mapping).
pub(super) fn as_map(value: &Value) -> Option<Map> {
    if let Some(map) = value.as_object() {
        Some(map.clone())
    } else if !careerbrew_career::truthy(Some(value)) {
        Some(Map::new())
    } else {
        None
    }
}

/// The zone name `calendar.local_tz()` uses: the `timezone` setting, else
/// `TZ`, else the `/etc/localtime` link. None when Python would raise or fall
/// back to UTC with a warning, or when the name is not one the Rust zone
/// table holds.
fn zone_name(ctx: &Ctx) -> Option<String> {
    let settings = read_map_yaml(&ctx.root.join("settings.yaml"))?;
    let setting = settings.get("timezone");
    let name = if careerbrew_career::truthy(setting) {
        setting?.as_str()?.to_owned()
    } else {
        let env = std::env::var("TZ").unwrap_or_default();
        let env = env.trim_start_matches(':').to_owned();
        if env.is_empty() {
            let link = std::fs::canonicalize("/etc/localtime").ok()?;
            let link = link.to_str()?.to_owned();
            link.split_once("/zoneinfo/")?.1.to_owned()
        } else {
            env
        }
    };
    if name.is_empty() || when::zone(&name).is_err() {
        return None;
    }
    Some(name)
}

/// `calendar.now()`, truncated to whole seconds (every caller here prints
/// `isoformat(timespec="seconds")` or a date).
pub(super) fn py_now(ctx: &Ctx) -> Option<when::At> {
    let pinned = std::env::var("CAREERBREW_NOW")
        .ok()
        .filter(|value| !value.is_empty());
    if let Some(pinned) = pinned {
        let (naive, offset) = when::parse(&pinned).ok()?;
        return Some(match offset {
            Some(offset) => when::fixed(naive, offset),
            None => when::attach(naive, when::zone(&zone_name(ctx)?).ok()?),
        });
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let (naive, offset) = when::parse(&crate::events::stamp(secs)).ok()?;
    let utc = when::fixed(naive, offset?);
    Some(when::into(&utc, when::zone(&zone_name(ctx)?).ok()?))
}

/// `calendar.now_utc()`.
pub(super) fn py_now_utc(ctx: &Ctx) -> Option<when::At> {
    Some(when::into(&py_now(ctx)?, when::zone("UTC").ok()?))
}

/// Appends one `events.emit(event, **fields)` row (keys sorted, as
/// `json.dumps(sort_keys=True)` writes them). Every value must already be a
/// lowercase id that `events.clean` keeps.
pub(super) fn emit(ctx: &Ctx, event: &str, fields: &[(&str, &str)]) {
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = crate::events::stamp(secs);
    let mut pairs: Vec<(&str, &str)> = vec![
        ("event", event),
        ("ts", &stamp),
        ("v", crate::events::version()),
    ];
    pairs.extend_from_slice(fields);
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let mut line = String::from("{");
    for (i, (key, value)) in pairs.iter().enumerate() {
        if i > 0 {
            line.push_str(", ");
        }
        crate::render::py_string(key, &mut line);
        line.push_str(": ");
        crate::render::py_string(value, &mut line);
    }
    line.push_str("}\n");
    let logs = ctx.root.join("logs");
    if std::fs::create_dir_all(&logs).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("events.jsonl"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// `a.flag or default`: an empty value falls back, as Python's `or` does.
fn given<'a>(args: &'a Parsed, name: &str) -> Option<&'a str> {
    args.text(name).filter(|text| !text.is_empty())
}

// ---------- work note ----------

fn work_note(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let title = args.text("title")?;
    // `a.tags.split(",") if a.tags else []`: empty parts stay, as in Python.
    let tags: Vec<String> = match given(args, "tags") {
        Some(text) => text.split(',').map(str::to_owned).collect(),
        None => Vec::new(),
    };
    let impact = args.text("impact").unwrap_or("");
    let at = when::iso_seconds(&py_now(ctx)?);
    let item = evidence::note(title, &tags, impact, Some(&at), &[]).ok()?;
    emit(ctx, "work.note", &[("status", "ok")]);
    let id = item.get("id")?.as_str()?;
    let shown: Vec<&str> = item
        .get("tags")?
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<_>>()?;
    ok(format!("noted {id} tags={}\n", shown.join(",")), 0)
}

// ---------- ladders ----------

fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// Every ladder as `promo.ladders()` returns it, when each id and level is
/// text (Python sorts and joins them; another type could crash there).
fn ladders() -> Option<std::collections::BTreeMap<String, Value>> {
    let lads = promo::ladders().ok()?;
    for lad in lads.values() {
        lad.get("id")?.as_str()?;
        strings(lad.get("levels"))?;
        lad.get("origin")?.as_str()?;
    }
    Some(lads)
}

fn ladder_list(_ctx: &Ctx) -> Option<Outcome> {
    let mut out = String::new();
    for (id, lad) in ladders()? {
        let levels = strings(lad.get("levels"))?.join(",");
        let origin = lad.get("origin")?.as_str()?;
        out.push_str(&format!("{id:<20} levels={levels} origin={origin}\n"));
    }
    ok(out, 0)
}

/// `str(value)` of a setting, as `ladder_grade.target()` reads it, for the
/// text values it can hold here.
fn target(ctx: &Ctx) -> Option<(String, String)> {
    let settings = read_map_yaml(&ctx.root.join("settings.yaml"))?;
    for key in ["ladder", "target_level"] {
        if settings.get(key).is_some_and(|value| !value.is_string()) {
            return None;
        }
    }
    let (lid, lvl) = ladder_grade::target().ok()?;
    Some((lid, lvl))
}

/// The value text a dimension field may hold: text, or None for a null.
fn text_or_null(value: &Value) -> Option<Option<&str>> {
    if value.is_null() {
        Some(None)
    } else {
        value.as_str().map(Some)
    }
}

fn ladder_levels(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let (lid, lvl) = target(ctx)?;
    let lid = given(args, "ladder").map_or(lid, str::to_owned);
    let lvl = given(args, "level").map_or(lvl, str::to_owned);
    let lads = ladders()?;
    let Some(lad) = lads.get(&lid) else {
        let have: Vec<String> = lads.keys().cloned().collect();
        let msg = format!("unknown ladder {lid}; have {}", py::list_repr(&have));
        return ok(format!("error: {msg} (breaks US-88)\n"), 2);
    };
    let names = strings(lad.get("levels"))?;
    if !names.contains(&lvl) {
        let msg = format!("level {lvl} not in {}", py::list_repr(&names));
        return ok(format!("error: {msg} (breaks US-88)\n"), 2);
    }
    // The shapes `levels()` reads, checked so that a value Python would
    // list, crash on or print another way goes to Python.
    for c in lad.get("competencies")?.as_array()? {
        if let Some(ex) = c
            .get("examples")
            .filter(|v| careerbrew_career::truthy(Some(v)))
        {
            if let Some(level) = ex.get(&lvl).filter(|v| careerbrew_career::truthy(Some(v))) {
                for key in ["looks_like", "doesnt_look_like"] {
                    if let Some(list) = level
                        .as_object()?
                        .get(key)
                        .filter(|v| careerbrew_career::truthy(Some(v)))
                    {
                        strings(Some(list))?;
                    }
                }
            }
        }
        if let Some(how) = c.get("how_to_close") {
            how.as_str()?;
        }
    }
    let dims = ladder_grade::levels(&lid, &lvl).ok()?;
    for d in &dims {
        d.get("id")?.as_str()?;
        d.get("name")?.as_str()?;
        text_or_null(d.get("description")?)?;
    }
    if args.flag("json") {
        return ok(levels_json(&lid, &lvl, &dims)? + "\n", 0);
    }
    let mut out = String::new();
    for d in &dims {
        let id = d.get("id")?.as_str()?;
        let name = d.get("name")?.as_str()?;
        out.push_str(&format!("{id:<14} {name}"));
        if let Some(desc) = text_or_null(d.get("description")?)?.filter(|t| !t.is_empty()) {
            out.push_str(&format!(" — {desc}"));
        }
        out.push('\n');
    }
    ok(out, 0)
}

/// `json.dumps({"ladder", "level", "dimensions"}, indent=2)` in insertion
/// order, with `ensure_ascii` escapes.
fn levels_json(lid: &str, lvl: &str, dims: &[Value]) -> Option<String> {
    let s = |text: &str| {
        let mut out = String::new();
        crate::render::py_string(text, &mut out);
        out
    };
    let list = |items: &[String], indent: &str| -> String {
        if items.is_empty() {
            return "[]".into();
        }
        let inner = format!("{indent}  ");
        let body: Vec<String> = items.iter().map(|i| format!("{inner}{}", s(i))).collect();
        format!("[\n{}\n{indent}]", body.join(",\n"))
    };
    let mut out = format!("{{\n  \"ladder\": {},\n  \"level\": {},\n", s(lid), s(lvl));
    if dims.is_empty() {
        out.push_str("  \"dimensions\": []\n}");
        return Some(out);
    }
    out.push_str("  \"dimensions\": [\n");
    let mut rows = Vec::new();
    for d in dims {
        let field = |key: &str| -> Option<String> {
            Some(match text_or_null(d.get(key)?)? {
                Some(text) => s(text),
                None => "null".into(),
            })
        };
        let looks = strings(d.get("looks_like"))?;
        let not_looks = strings(d.get("doesnt_look_like"))?;
        let parts = [
            format!("      \"id\": {}", field("id")?),
            format!("      \"name\": {}", field("name")?),
            format!("      \"description\": {}", field("description")?),
            format!("      \"looks_like\": {}", list(&looks, "      ")),
            format!("      \"doesnt_look_like\": {}", list(&not_looks, "      ")),
            format!("      \"how_to_close\": {}", field("how_to_close")?),
        ];
        rows.push(format!("    {{\n{}\n    }}", parts.join(",\n")));
    }
    out.push_str(&rows.join(",\n"));
    out.push_str("\n  ]\n}");
    Some(out)
}

// ---------- work gaps ----------

/// True when Python's `fromisoformat(at.replace("Z", "+00:00"))` reads the
/// stamp as Rust does: an aware time. A naive one would raise in Python.
fn aware_stamp(text: &str) -> bool {
    matches!(when::parse(&text.replace('Z', "+00:00")), Ok((_, Some(_))))
}

fn work_gaps(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let assessment = assessment(args, ctx)?;
    let mut out = String::new();
    for (i, g) in promo::gaps(&assessment).iter().enumerate() {
        out.push_str(&format!(
            "{}. {:<14} coverage {}  priority {}  -> {}\n",
            i + 1,
            g.id,
            py::percent(g.coverage),
            py::float_repr(g.priority),
            g.how_to_close
        ));
    }
    ok(out, 0)
}

/// `cli._assessment(a)`, when every value it reads has a shape Rust reads
/// as Python does.
fn assessment(args: &Parsed, ctx: &Ctx) -> Option<promo::Assessment> {
    // Long-term goal words come from `goals.load()`; that reader stays Python.
    if ctx.root.join("goals.yaml").exists() {
        return None;
    }
    let settings = read_map_yaml(&ctx.root.join("settings.yaml"))?;
    let setting = |key: &str, default: &str| -> Option<String> {
        match settings.get(key) {
            None => Some(default.to_owned()),
            Some(value) => value.as_str().map(str::to_owned),
        }
    };
    let ladder = match given(args, "ladder") {
        Some(text) => text.to_owned(),
        None => setting("ladder", "generic-swe")?,
    };
    let level = match given(args, "level") {
        Some(text) => text.to_owned(),
        None => setting("target_level", "senior")?,
    };
    let lads = ladders()?;
    // An unknown ladder or level raises LadderError: Python prints it and
    // logs `cli.error`, which this binary does not write.
    let lad = lads.get(&ladder)?;
    if !strings(lad.get("levels"))?.contains(&level) {
        return None;
    }
    if lad
        .get("window_months")
        .is_some_and(|value| !(value.is_i64() || value.is_u64()))
    {
        return None;
    }
    for comp in lad.get("competencies")?.as_array()? {
        comp.get("id")?.as_str()?;
        comp.get("name")?.as_str()?;
        if !comp.get("weight")?.is_number() {
            return None;
        }
        if comp.get("effort").is_some_and(|v| !v.is_number()) {
            return None;
        }
        if !comp.get("expect")?.get(&level)?.is_number() {
            return None;
        }
        if comp.get("how_to_close").is_some_and(|v| !v.is_string()) {
            return None;
        }
    }
    for item in evidence::load().ok()? {
        let item = item.as_object()?;
        if let Some(at) = item.get("at") {
            if !aware_stamp(at.as_str()?) {
                return None;
            }
        }
        if let Some(title) = item.get("title") {
            title.as_str()?;
        }
        if let Some(tags) = item.get("tags") {
            strings(Some(tags))?;
        }
    }
    let at = py_now(ctx)?;
    promo::assess(&ladder, &level, &at).ok()
}

// ---------- ladder import ----------

/// True when `text` (a loaded ladder id) names one plain file in `ladders/`.
fn plain_id(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// The text of a `Canon` string.
fn canon_str(value: &Canon) -> Option<&str> {
    match value {
        Canon::Str(text) => Some(text),
        _ => None,
    }
}

/// The value under the text key `key` of a mapping (the first one: a
/// mapping with a repeated key never reaches the dump, which refuses it).
fn canon_get<'a>(pairs: &'a [(Canon, Canon)], key: &str) -> Option<&'a Canon> {
    pairs
        .iter()
        .find(|(k, _)| canon_str(k) == Some(key))
        .map(|(_, v)| v)
}

/// `promo.validate_ladder(data)` passes, for the shapes Rust checks the way
/// Python does: a mapping with a text id, a list of text levels, and a list
/// of competency mappings, each with every key and an expectation per level.
/// Any other shape (one Python could read another way, or rejects with a
/// `LadderError`) returns None.
fn valid_ladder(data: &Canon) -> Option<String> {
    let Canon::Map(top) = data else { return None };
    let id = canon_str(canon_get(top, "id")?)?;
    if !plain_id(id) {
        return None;
    }
    let Canon::List(levels) = canon_get(top, "levels")? else {
        return None;
    };
    let Canon::List(comps) = canon_get(top, "competencies")? else {
        return None;
    };
    if levels.is_empty() || comps.is_empty() {
        return None;
    }
    let levels: Vec<&str> = levels.iter().map(canon_str).collect::<Option<_>>()?;
    for comp in comps {
        let Canon::Map(c) = comp else { return None };
        for key in ["id", "name", "weight", "signals", "expect"] {
            canon_get(c, key)?;
        }
        let Canon::Map(expect) = canon_get(c, "expect")? else {
            return None;
        };
        if levels.iter().any(|lvl| canon_get(expect, lvl).is_none()) {
            return None;
        }
        for key in ["descriptions", "examples"] {
            if let Some(value) = canon_get(c, key) {
                if !matches!(value, Canon::Map(_)) {
                    return None;
                }
            }
        }
    }
    Some(id.to_owned())
}

/// True when PyYAML could read `text` in a way this module does not model:
/// anchors and aliases (shared objects dump with `&id001`), tags, merge
/// keys, directives, complex keys, a BOM or a carriage return.
fn odd_yaml(text: &str) -> bool {
    text.contains(['&', '*', '!', '\r', '\u{feff}'])
        || text.contains("<<")
        || text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with('%') || line.starts_with('?')
        })
}

fn ladder_import(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    let file = args.text("file")?;
    if file.is_empty() {
        return None;
    }
    let context = careerbrew_vault::transfer::Context::from_env().ok()?;
    let store = context.store();
    // A sealed write differs on every run.
    if store.encryption_enabled() {
        return None;
    }
    let src = Path::new(file);
    if !std::fs::symlink_metadata(src).ok()?.is_file() {
        return None;
    }
    let raw = std::fs::read(src).ok()?;
    if raw.starts_with(careerbrew_vault::MAGIC) || raw.starts_with(b"gAAAAA") {
        return None;
    }
    let text = String::from_utf8(raw).ok()?;
    if odd_yaml(&text) {
        return None;
    }
    let data = careerbrew_vault::json_yaml::yaml_read(&text).ok()?;
    let id = valid_ladder(&data)?;
    let dump = crate::pyyaml::safe_dump(&data)?;
    let dir = ctx.root.join("ladders");
    if std::fs::symlink_metadata(&dir).is_ok_and(|meta| !meta.is_dir()) {
        return None;
    }
    let path = dir.join(format!("{id}.yaml"));
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| !meta.is_file()) {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    store.write_text(&path, &dump).ok()?;
    ok(format!("imported ladder {id}\n"), 0)
}

// ---------- work plan ----------

/// `f"{value:g}"`: six significant digits, trailing zeros dropped, and the
/// exponent form outside 1e-4 to 1e6.
fn py_g(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(if value.is_sign_negative() { "-0" } else { "0" }.into());
    }
    let sci = format!("{value:.5e}");
    let (mantissa, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let trim = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if (-4..6).contains(&exp) {
        let precision = (5 - exp) as usize;
        Some(trim(&format!("{value:.precision$}")))
    } else {
        let sign = if exp < 0 { '-' } else { '+' };
        Some(format!("{}e{sign}{:02}", trim(mantissa), exp.abs()))
    }
}

/// `float(value)` for a stored task field, or `default` where Python's
/// `_num` falls back. None for a value whose `float()` this does not model
/// (text, a huge int).
fn py_num(value: Option<&Value>, default: f64) -> Option<f64> {
    let Some(value) = value else {
        return Some(default);
    };
    if let Some(b) = value.as_bool() {
        return Some(if b { 1.0 } else { 0.0 });
    }
    if value.is_null() || value.is_array() || value.is_object() {
        return Some(default);
    }
    if value.is_string() {
        return None;
    }
    if (value.is_i64() || value.is_u64()) && value.as_f64()?.abs() > 1e15 {
        return None;
    }
    let number = value.as_f64()?;
    Some(if number.is_finite() { number } else { default })
}

/// `priorities.estimate(t)`.
fn py_estimate(task: &Map) -> Option<f64> {
    let hours = py_num(task.get("estimate_h"), 1.0)?;
    Some(if hours.is_finite() && hours > 0.0 && hours <= 200.0 {
        hours
    } else {
        1.0
    })
}

/// Days since 1970-01-01 for a strict `YYYY-MM-DD`, or Some(None) for a date
/// of that shape that does not exist (`date.fromisoformat` raises, which
/// `rank` reads as no due date). None for another shape.
fn civil_days(text: &str) -> Option<Option<i64>> {
    let b = text.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || ![0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
    {
        return None;
    }
    let year: i64 = text[0..4].parse().ok()?;
    let month: i64 = text[5..7].parse().ok()?;
    let day: i64 = text[8..10].parse().ok()?;
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Some(None),
    };
    if year < 1 || day < 1 || day > last {
        return Some(None);
    }
    // Howard Hinnant's days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(Some(era * 146_097 + doe - 719_468))
}

/// One ranked task: its id, score, promotion bonus and estimate.
struct Ranked {
    id: String,
    score: f64,
    bonus: f64,
    estimate: f64,
}

/// Python's `max(values, default=0.0)` for finite floats.
fn py_max(values: impl Iterator<Item = f64>) -> f64 {
    values
        .fold(None, |acc: Option<f64>, p| {
            Some(acc.map_or(p, |a| if p > a { p } else { a }))
        })
        .unwrap_or(0.0)
}

/// `priorities.rank(gap_priorities)` for tasks whose fields Rust reads as
/// Python does, with no `project_priority` setting (so no project bonus).
fn rank(ctx: &Ctx, gaps: &[(String, f64)]) -> Option<Vec<Ranked>> {
    let settings = read_map_yaml(&ctx.root.join("settings.yaml"))?;
    if careerbrew_career::truthy(settings.get("project_priority")) {
        return None;
    }
    let lid = target(ctx)?.0;
    let today = civil_days(&when::strftime(&py_now(ctx)?, "%Y-%m-%d"))??;
    let top = py_max(gaps.iter().map(|(_, p)| *p));
    let top = if top == 0.0 { 1.0 } else { top };
    let file = read_map_yaml(&ctx.root.join("work").join("tasks.yaml"))?;
    let tasks = match file.get("tasks") {
        None => Vec::new(),
        Some(value) => value.as_array()?.clone(),
    };
    let mut out = Vec::new();
    for task in &tasks {
        let task = task.as_object()?;
        // `_migrate` rewrites an old `link`; that path stays Python.
        if task.contains_key("link") {
            return None;
        }
        let open = match task.get("status") {
            None => true,
            Some(value) => value.as_str() == Some("open"),
        };
        if !open {
            continue;
        }
        let id = task.get("id")?.as_str()?.to_owned();
        let title = match task.get("title") {
            None => String::new(),
            Some(value) => value.as_str()?.to_owned(),
        };
        // `project_priority.of_task` reads the links even with no projects.
        if let Some(links) = task.get("links") {
            if careerbrew_career::truthy(Some(links))
                && !links
                    .as_array()?
                    .iter()
                    .all(|link| link.is_string() || link.is_object())
            {
                return None;
            }
        }
        let mut urgency = py_num(task.get("urgency"), 3.0)?;
        if let Some(due) = task.get("due") {
            if careerbrew_career::truthy(Some(due)) {
                let days = match due.as_str() {
                    Some(text) => civil_days(text)?,
                    // `str(True)` never parses; a number may (`20261120`).
                    None if due.is_boolean() => None,
                    None => return None,
                };
                if days.is_some_and(|d| d - today <= 2) {
                    urgency += 3.0;
                }
            }
        }
        let dims: Vec<String> = match task.get("competencies") {
            Some(value) if careerbrew_career::truthy(Some(value)) => value
                .as_array()?
                .iter()
                .map(|c| c.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
            _ => ladder_grade::dims_of(&title, Some(&lid))
                .ok()?
                .iter()
                .map(|c| c.as_str().map(str::to_owned))
                .collect::<Option<_>>()?,
        };
        let best = py_max(dims.iter().map(|c| {
            gaps.iter()
                .rev()
                .find(|(id, _)| id == c)
                .map_or(0.0, |(_, p)| *p)
        }));
        let bonus = 3.0 * best / top;
        let impact = py_num(task.get("impact"), 3.0)?;
        let estimate = py_estimate(task)?;
        let size = if estimate > 0.25 { estimate } else { 0.25 };
        let score = (impact + urgency + bonus + 0.0) / size.sqrt();
        if !score.is_finite() || !bonus.is_finite() {
            return None;
        }
        out.push(Ranked {
            id,
            score: py::round(score, 2),
            bonus: py::round(bonus, 2),
            estimate,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    Some(out)
}

/// `repr(value)` for a value Rust prints as Python does (no exponent).
fn plain_repr(value: f64) -> Option<String> {
    (value == 0.0 || (value.abs() >= 1e-4 && value.abs() < 1e15)).then(|| py::float_repr(value))
}

fn work_plan(args: &Parsed, ctx: &Ctx) -> Option<Outcome> {
    // A terminal shows each title; that output stays Python.
    if ctx.env.stdout_is_terminal {
        return None;
    }
    let crate::args::Value::Number(hours) = args.get("hours") else {
        return None;
    };
    let capacity: f64 = hours.parse().ok()?;
    if !capacity.is_finite() {
        return None;
    }
    let assessment = assessment(args, ctx)?;
    let gaps: Vec<(String, f64)> = promo::gaps(&assessment)
        .iter()
        .map(|g| (g.id.clone(), g.priority))
        .collect();
    if gaps.iter().any(|(_, p)| !p.is_finite()) {
        return None;
    }
    let ranked = rank(ctx, &gaps)?;
    let mut chosen = Vec::new();
    let mut used = 0.0;
    for task in &ranked {
        if used + task.estimate <= capacity {
            chosen.push(task);
            used += task.estimate;
        }
    }
    let mut out = format!("plan: {}h of {}h\n", py_g(used)?, py_g(capacity)?);
    for task in &chosen {
        out.push_str(&format!(
            "- {} score {} promo+{} est {}h  (title hidden; run in your terminal)\n",
            task.id,
            plain_repr(task.score)?,
            plain_repr(task.bonus)?,
            py_g(task.estimate)?
        ));
    }
    emit_counts(
        ctx,
        "work.plan",
        &[("status", "ok")],
        &[("tasks", chosen.len())],
    );
    ok(out, 0)
}

/// `emit` with whole-number fields as well as text ones.
fn emit_counts(ctx: &Ctx, event: &str, texts: &[(&str, &str)], counts: &[(&str, usize)]) {
    if !ctx.env.caps.events_on {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = crate::events::stamp(secs);
    let quote = |text: &str| {
        let mut out = String::new();
        crate::render::py_string(text, &mut out);
        out
    };
    let mut pairs: Vec<(&str, String)> = vec![
        ("event", quote(event)),
        ("ts", quote(&stamp)),
        ("v", quote(crate::events::version())),
    ];
    for (key, value) in texts {
        pairs.push((key, quote(value)));
    }
    for (key, value) in counts {
        pairs.push((key, value.to_string()));
    }
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    let body: Vec<String> = pairs
        .iter()
        .map(|(key, value)| format!("{}: {value}", quote(key)))
        .collect();
    let line = format!("{{{}}}\n", body.join(", "));
    let logs = ctx.root.join("logs");
    if std::fs::create_dir_all(&logs).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("events.jsonl"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}
