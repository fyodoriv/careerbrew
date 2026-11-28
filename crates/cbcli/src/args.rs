//! A conservative argparse reader for routed commands, built from each
//! command's argument spec in the generated registry.
//!
//! It accepts only forms whose meaning is certain: exact `--flag` names
//! (no abbreviations, no `--flag=value`), plain decimal numbers, values
//! that do not start with `-`, and listed choices. Anything else returns
//! `None`, so the command refuses (exit 64) and Python parses it, prints
//! its own error and exits as before. A refusal is never wrong, only slower.
use crate::registry::{Json, Registry};
use std::collections::BTreeMap;

/// One parsed value. A flag that was not given holds its registry default
/// (`Missing` when there is none), as argparse's namespace does.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Missing,
    Flag(bool),
    Text(String),
    Int(i64),
    /// The number as typed; argparse's `float()` would read it the same.
    Number(String),
    List(Vec<String>),
}

/// The parsed arguments of one command, by argparse destination name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    pub values: BTreeMap<String, Value>,
}

impl Parsed {
    pub fn get(&self, name: &str) -> &Value {
        self.values.get(name).unwrap_or(&Value::Missing)
    }

    /// True for a given `--flag`.
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), Value::Flag(true))
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        match self.get(name) {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn list(&self, name: &str) -> &[String] {
        match self.get(name) {
            Value::List(items) => items,
            _ => &[],
        }
    }
}

struct Spec<'a> {
    name: &'a str,
    flag: Option<&'a str>,
    kind: &'a str,
    choices: Option<Vec<&'a str>>,
    required: bool,
    many: bool,
    default: &'a Json,
}

fn specs<'a>(row: &'a Json) -> Option<Vec<Spec<'a>>> {
    let mut out = Vec::new();
    for arg in row.get("args")?.as_array()? {
        let choices = match arg.get("choices") {
            Some(Json::Array(items)) => {
                Some(items.iter().map(Json::as_str).collect::<Option<Vec<_>>>()?)
            }
            _ => None,
        };
        out.push(Spec {
            name: arg.get("name")?.as_str()?,
            flag: arg.get("flag").and_then(Json::as_str),
            kind: arg.get("kind")?.as_str()?,
            choices,
            required: matches!(arg.get("required"), Some(Json::Bool(true))),
            many: matches!(arg.get("many"), Some(Json::Bool(true))),
            default: arg.get("default").unwrap_or(&Json::Null),
        });
    }
    Some(out)
}

fn is_int(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn is_number(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let mut parts = body.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// Reads one value of `spec`'s kind, or None for a form argparse could read
/// differently (or reject).
fn value_of(spec: &Spec, text: &str) -> Option<Value> {
    if text.starts_with('-') || text.is_empty() && spec.kind != "text" {
        return None;
    }
    if let Some(choices) = &spec.choices {
        if !choices.contains(&text) {
            return None;
        }
    }
    match spec.kind {
        "int" if is_int(text) => text.parse().ok().map(Value::Int),
        "number" if is_number(text) => Some(Value::Number(text.to_owned())),
        "text" | "choice" => Some(Value::Text(text.to_owned())),
        _ => None,
    }
}

/// The registry default, in the form a given value would take.
fn default_of(spec: &Spec) -> Option<Value> {
    Some(match (spec.default, spec.kind) {
        (Json::Null, "flag") => Value::Flag(false),
        (Json::Null, _) if spec.many => Value::List(Vec::new()),
        (Json::Null, _) => Value::Missing,
        (Json::Bool(value), _) => Value::Flag(*value),
        (Json::Number(text), "int") => Value::Int(text.parse().ok()?),
        (Json::Number(text), _) => Value::Number(text.clone()),
        (Json::Str(text), _) if spec.many => {
            // A text default on an append flag: argparse appends to it,
            // which the registry cannot show; refuse such a command.
            if text.is_empty() {
                Value::List(Vec::new())
            } else {
                return None;
            }
        }
        (Json::Str(text), _) => Value::Text(text.clone()),
        (Json::Array(items), _) if spec.many && items.is_empty() => Value::List(Vec::new()),
        _ => return None,
    })
}

/// Parses `rest` (the words after the command's own words) for the
/// registry command `name`.
pub fn parse(reg: &Registry, name: &str, rest: &[String]) -> Option<Parsed> {
    let row = reg
        .rows
        .iter()
        .find(|row| row.get("name").and_then(Json::as_str) == Some(name))?;
    let specs = specs(row)?;
    let mut values = BTreeMap::new();
    for spec in &specs {
        values.insert(spec.name.to_owned(), default_of(spec)?);
    }
    let positionals: Vec<&Spec> = specs.iter().filter(|s| s.flag.is_none()).collect();
    // Only the last positional may take many words. An optional positional
    // before the last one is read only when every positional word comes
    // before the first flag: argparse then fills them left to right in one
    // chunk. Any other shape depends on argparse's regex matching.
    let inner = positionals.iter().rev().skip(1);
    if inner.clone().any(|s| s.many) {
        return None;
    }
    let inner_optional = inner.clone().any(|s| !s.required);
    let mut seen_flag = false;
    let mut words: Vec<&str> = Vec::new();
    let mut given: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if arg.starts_with('-') {
            if arg == "-" || arg == "--" || arg.contains('=') {
                return None;
            }
            let spec = specs.iter().find(|s| s.flag == Some(arg))?;
            seen_flag = true;
            given.push(spec.name);
            if spec.kind == "flag" {
                values.insert(spec.name.to_owned(), Value::Flag(true));
                i += 1;
                continue;
            }
            let value = value_of(spec, rest.get(i + 1)?)?;
            if spec.many {
                let Value::Text(text) = value else {
                    return None;
                };
                match values.get_mut(spec.name) {
                    Some(Value::List(items)) => items.push(text),
                    _ => return None,
                }
            } else {
                values.insert(spec.name.to_owned(), value);
            }
            i += 2;
        } else {
            if inner_optional && seen_flag {
                return None;
            }
            words.push(arg);
            i += 1;
        }
    }
    let mut words = words.into_iter();
    for spec in &positionals {
        if spec.many {
            let rest: Vec<String> = words.by_ref().map(str::to_owned).collect();
            if spec.required && rest.is_empty() {
                return None;
            }
            for word in &rest {
                value_of(spec, word)?;
            }
            values.insert(spec.name.to_owned(), Value::List(rest));
        } else if let Some(word) = words.next() {
            values.insert(spec.name.to_owned(), value_of(spec, word)?);
        } else if spec.required {
            return None;
        }
    }
    if words.next().is_some() {
        return None;
    }
    for spec in specs.iter().filter(|s| s.flag.is_some() && s.required) {
        if !given.contains(&spec.name) {
            return None;
        }
    }
    Some(Parsed { values })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> Registry {
        Registry::load().unwrap()
    }

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn reads_positionals_flags_ints_and_repeats() {
        let got = parse(
            &reg(),
            "work.priority.set",
            &words("acme --rank 2 --match */acme --match PROJ-*"),
        )
        .unwrap();
        assert_eq!(got.text("project"), Some("acme"));
        assert_eq!(got.int("rank"), Some(2));
        assert_eq!(got.list("match"), ["*/acme", "PROJ-*"]);
    }

    #[test]
    fn fills_defaults_and_unset_flags() {
        let got = parse(&reg(), "day.check", &[]).unwrap();
        assert!(!got.flag("json"));
        let got = parse(&reg(), "day.check", &words("--json")).unwrap();
        assert!(got.flag("json"));
    }

    #[test]
    fn refuses_every_uncertain_form() {
        let r = reg();
        for argv in [
            "acme --rank two",
            "acme --rank=2",
            "acme --ran 2",
            "acme --rank -2x",
            "acme extra",
            "--rank 2",
            "acme -- --rank",
        ] {
            assert_eq!(parse(&r, "work.priority.set", &words(argv)), None, "{argv}");
        }
        assert_eq!(parse(&r, "no.such.command", &[]), None);
    }

    #[test]
    fn inner_optional_positionals_fill_left_to_right_before_any_flag() {
        let r = reg();
        let got = parse(&r, "day.handoff", &words("add prs synthetic")).unwrap();
        assert_eq!(got.text("action"), Some("add"));
        assert_eq!(got.text("section"), Some("prs"));
        assert_eq!(got.text("text"), Some("synthetic"));
        let got = parse(&r, "day.handoff", &words("done h-1")).unwrap();
        assert_eq!(got.text("section"), Some("h-1"));
        assert_eq!(got.text("text"), None);
        assert!(parse(&r, "day.handoff", &words("show --today")).is_some());
        let got = parse(&r, "day.handoff", &words("add prs t --link u")).unwrap();
        assert_eq!(got.text("link"), Some("u"));
        // A positional word after a flag: argparse may split the chunk.
        assert_eq!(parse(&r, "day.handoff", &words("add --link u prs t")), None);
        assert_eq!(parse(&r, "day.handoff", &words("add prs --link u t")), None);
    }

    #[test]
    fn numbers_are_plain_decimals_only() {
        assert!(is_int("-12") && is_int("0") && !is_int("+1") && !is_int("1_0") && !is_int(""));
        assert!(
            is_number("1.5")
                && is_number("-2")
                && !is_number("1.")
                && !is_number(".5")
                && !is_number("1e3")
        );
    }
}
