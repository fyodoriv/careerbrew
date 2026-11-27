//! Python text, number and regular-expression spellings the ported modules use.

use careerbrew_policy::pyre::Regex;
use careerbrew_policy::text;
use careerbrew_policy::Rules;

/// `value[:limit]` in code points.
pub fn take(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

pub fn len(value: &str) -> usize {
    value.chars().count()
}

pub fn lower(value: &str) -> String {
    text::lower(value)
}

pub fn upper(value: &str) -> String {
    value.to_uppercase()
}

pub fn strip(value: &str) -> &str {
    text::strip(value)
}

pub fn strip_chars<'a>(value: &'a str, chars: &str) -> &'a str {
    text::strip_chars(value, chars)
}

/// `repr(str)`.
pub fn repr(value: &str) -> String {
    careerbrew_policy::approvals::py_repr(value)
}

/// `repr(list_of_str)`: `['a', 'b']`.
pub fn list_repr(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|v| repr(v))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `repr(float)` for the finite values these modules compute.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    let text = format!("{value:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            let (sign, digits) = match exponent.strip_prefix('-') {
                Some(digits) => ('-', digits),
                None => ('+', exponent),
            };
            let mantissa = mantissa.strip_suffix(".0").unwrap_or(mantissa);
            format!("{mantissa}e{sign}{digits:0>2}")
        }
        None => text,
    }
}

/// `round(value, digits)`.
pub fn round(value: f64, digits: i32) -> f64 {
    careerbrew_projects::python_round(value, digits)
}

/// `f"{value:.0%}"`.
pub fn percent(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

/// `f"{value:g}"` for the small counts a ladder expects.
pub fn general(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() || text == "-" {
        "0".into()
    } else {
        text.into()
    }
}

/// `re.escape` for a literal inside a pattern: every character but an ASCII
/// letter, digit or underscore is escaped (the matcher reads `\x` as `x`).
pub fn escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

/// One Python `re` pattern over the backtracking matcher.
pub struct Re(Regex);

impl Re {
    pub fn new(src: &str, icase: bool) -> Re {
        Re(Regex::new(src, icase, &Rules::python())
            .unwrap_or_else(|e| panic!("pattern {src}: {e}")))
    }

    pub fn search(&self, value: &str) -> bool {
        self.0.is_match(value)
    }

    /// `re.sub(pattern, repl, value)` with a literal replacement.
    pub fn sub(&self, value: &str, repl: &str) -> String {
        self.0.sub(value, repl)
    }

    /// Every match: its text and each group's text (empty when unset).
    pub fn find_iter(&self, value: &str) -> Vec<(String, Vec<Option<String>>)> {
        let chars: Vec<char> = value.chars().collect();
        self.0
            .matches(&chars)
            .into_iter()
            .map(|(start, end, caps)| {
                let groups = (1..caps.len())
                    .map(|i| caps[i].map(|(s, e)| chars[s..e].iter().collect::<String>()))
                    .collect();
                (chars[start..end].iter().collect(), groups)
            })
            .collect()
    }

    /// The first match's groups, or None.
    pub fn captures(&self, value: &str) -> Option<Vec<Option<String>>> {
        self.find_iter(value)
            .into_iter()
            .next()
            .map(|(_, groups)| groups)
    }

    /// The first match's text, or None.
    pub fn find(&self, value: &str) -> Option<String> {
        self.find_iter(value)
            .into_iter()
            .next()
            .map(|(text, _)| text)
    }
}
