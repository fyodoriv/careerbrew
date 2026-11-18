//! `careerbrew actions` output: the text listing and the `--json` dump,
//! byte-equal to `cli.cmd_actions` in Python.
use crate::registry::Json;

/// Python's `json.dumps` string form with `ensure_ascii=True`: `\"`, `\\`,
/// the five short escapes, and `\uXXXX` (lowercase hex, surrogate pairs
/// above U+FFFF) for every other character outside space to `~`.
pub fn py_string(text: &str, out: &mut String) {
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
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Python's `json.dumps(value, indent=1)`: `", "`-free item separators
/// (`,` then a newline), `": "` after keys, insertion order, and `[]`/`{}`
/// for empty containers.
pub fn py_dumps(value: &Json) -> String {
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

fn newline(level: usize, out: &mut String) {
    out.push('\n');
    for _ in 0..level {
        out.push(' ');
    }
}

fn write(value: &Json, level: usize, out: &mut String) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Number(text) => out.push_str(text),
        Json::Str(text) => py_string(text, out),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Object(pairs) if pairs.is_empty() => out.push_str("{}"),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(level + 1, out);
                write(item, level + 1, out);
            }
            newline(level, out);
            out.push(']');
        }
        Json::Object(pairs) => {
            out.push('{');
            for (i, (key, item)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(level + 1, out);
                py_string(key, out);
                out.push_str(": ");
                write(item, level + 1, out);
            }
            newline(level, out);
            out.push('}');
        }
    }
}

/// The rows `actions [name]` shows: all of them, or the one named and its
/// `name.` children. An empty name filters nothing, as in Python.
pub fn select<'a>(rows: &'a [Json], name: Option<&str>) -> Vec<&'a Json> {
    rows.iter()
        .filter(|row| match name {
            Some(name) if !name.is_empty() => {
                let own = row.get("name").and_then(Json::as_str).unwrap_or("");
                own == name
                    || own
                        .strip_prefix(name)
                        .is_some_and(|rest| rest.starts_with('.'))
            }
            _ => true,
        })
        .collect()
}

/// `actions --json`: the selected rows and a newline.
pub fn json_listing(rows: &[&Json]) -> String {
    let array = Json::Array(rows.iter().map(|row| (*row).clone()).collect());
    let mut out = py_dumps(&array);
    out.push('\n');
    out
}

fn text(row: &Json, key: &str) -> String {
    row.get(key).and_then(Json::as_str).unwrap_or("").to_owned()
}

/// `actions`: one line per row, then the count line.
pub fn text_listing(rows: &[&Json]) -> String {
    let mut out = String::new();
    for row in rows {
        let terminal = row.get("terminal_only").unwrap_or(&Json::Null);
        let confirm = row.get("confirm").unwrap_or(&Json::Null);
        let mut note = if terminal.truthy() {
            format!("  [terminal only: {}]", text(row, "terminal_only"))
        } else if confirm.truthy() {
            format!("  [confirm: {}]", text(row, "confirm"))
        } else {
            String::new()
        };
        if row.get("agent_note").is_some_and(Json::truthy) {
            note.push_str(&format!("  [{}]", text(row, "agent_note")));
        }
        let args: Vec<String> = row
            .get("args")
            .and_then(Json::as_array)
            .unwrap_or(&[])
            .iter()
            .filter(|arg| {
                arg.get("required").is_some_and(Json::truthy)
                    || !arg.get("flag").is_some_and(Json::truthy)
            })
            .map(|arg| {
                let name = text(arg, "name");
                if arg.get("flag").is_some_and(Json::truthy) {
                    format!("--{}", name.replace('_', "-"))
                } else {
                    format!("<{name}>")
                }
            })
            .collect();
        let head = format!("{} {}", text(row, "command"), args.join(" "));
        out.push_str(head.trim_end_matches(py_space));
        out.push_str(&format!("  - {}{}\n", text(row, "help"), note));
    }
    out.push_str(&format!(
        "{} action(s); the web app runs them in Actions; details: careerbrew actions --json\n",
        rows.len()
    ));
    out
}

/// Python's `str.rstrip()` whitespace set.
fn py_space(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{1c}'..='\u{1f}')
}
