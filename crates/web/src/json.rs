//! Response bodies in Python `json.dumps` form: insertion-ordered keys,
//! `", "` and `": "` separators and `ensure_ascii` escapes, so a native answer
//! is byte-identical to the Python handler's `_encode`.

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Obj(Vec<(String, J)>),
}

pub fn obj(items: Vec<(&str, J)>) -> J {
    J::Obj(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub fn s(text: impl Into<String>) -> J {
    J::Str(text.into())
}

pub fn error(message: impl Into<String>) -> J {
    obj(vec![("error", s(message))])
}

fn string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(out: &mut String, value: &J) {
    match value {
        J::Null => out.push_str("null"),
        J::Bool(true) => out.push_str("true"),
        J::Bool(false) => out.push_str("false"),
        J::Int(v) => out.push_str(&v.to_string()),
        J::Str(v) => string(out, v),
        J::Obj(items) => {
            out.push('{');
            for (i, (key, item)) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                string(out, key);
                out.push_str(": ");
                write(out, item);
            }
            out.push('}');
        }
    }
}

/// `json.dumps(value)` with Python's default arguments.
pub fn dumps(value: &J) -> String {
    let mut out = String::new();
    write(&mut out, value);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_default_dumps() {
        let value = obj(vec![
            ("signed_in", J::Bool(true)),
            ("days_left", J::Int(10)),
            ("cookie", J::Bool(false)),
            ("note", s("caf\u{e9} \"q\" \u{1f600}\n")),
            ("none", J::Null),
        ]);
        assert_eq!(
            dumps(&value),
            r#"{"signed_in": true, "days_left": 10, "cookie": false, "note": "caf\u00e9 \"q\" \ud83d\ude00\n", "none": null}"#
        );
        assert_eq!(dumps(&J::Obj(vec![])), "{}");
        assert_eq!(dumps(&s("\u{7f}")), "\"\\u007f\"");
    }
}
