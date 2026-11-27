//! Native CLI handlers owned by `rust-18` (see `steps/mod.rs`): career
//! evidence notes, ladder listings and promotion gaps, through the
//! `careerbrew-career` component crate.
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
pub const COMMANDS: &[&str] = &["work.note", "work.gaps", "ladder.list", "ladder.levels"];

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
    let assessment = promo::assess(&ladder, &level, &at).ok()?;
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
