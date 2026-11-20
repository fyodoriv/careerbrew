//! Canary outputs (rust-04e): `privacy.assert_clean`, `privacy.redact` and
//! `privacy._names_private_path` over supplied text and terms.
//!
//! `assert_clean` refuses with term ids only, never term text. `redact`
//! returns the text unchanged when no term matches, else the normalized
//! text with each term's matches replaced in term order, like `re.sub`.

use crate::allow::PyError;
use crate::canary::Rules;
use crate::text;

/// The `privacy.PrivacyError` of `assert_clean`: a count, the place and ids.
pub fn assert_clean(
    rules: &Rules,
    text_in: &str,
    place: &str,
    terms: &[String],
) -> Result<(), PyError> {
    let leaks = rules.find_leaks(text_in, terms);
    if leaks.is_empty() {
        return Ok(());
    }
    Err(PyError::new(
        "PrivacyError",
        format!(
            "blocked: {} sensitive term(s) would leave the data root via {}: {}",
            leaks.len(),
            place,
            leaks.join(", ")
        ),
    ))
}

/// The replacement `privacy.redact` writes for each match.
pub const REDACTED: &str = "[REDACTED]";

impl Rules {
    /// `privacy.redact` over supplied terms.
    pub fn redact(&self, text_in: &str, terms: &[String]) -> String {
        if self.find_leaks(text_in, terms).is_empty() {
            return text_in.to_owned();
        }
        let mut chars: Vec<char> = text::normalize(text_in).chars().collect();
        for t in terms {
            let spans = self.match_spans(t, &chars);
            if spans.is_empty() {
                continue;
            }
            let mut out = Vec::with_capacity(chars.len());
            let mut at = 0;
            for (s, e) in spans {
                out.extend_from_slice(&chars[at..s]);
                out.extend(REDACTED.chars());
                at = e;
            }
            out.extend_from_slice(&chars[at..]);
            chars = out;
        }
        chars.into_iter().collect()
    }
}

/// `privacy.redact` with CPython's rules.
pub fn redact(text_in: &str, terms: &[String]) -> String {
    Rules::python().redact(text_in, terms)
}

/// `re` `\w` in a str pattern: `isalnum` or `_`.
fn is_word(c: char) -> bool {
    c == '_' || text::is_alnum(c)
}

/// `[\w.-]`, a char of a path name after `private/`.
fn is_name(c: char) -> bool {
    is_word(c) || c == '.' || c == '-'
}

/// `privacy._PATH_START`: a char that can open a path.
fn opens_path(c: char) -> bool {
    text::is_space(c) || "([{<\"'`=:,".contains(c)
}

/// `privacy._SYSTEM_DIRS`: the macOS system roots under `/private`.
const SYSTEM_DIRS: &[&str] = &["tmp", "var", "etc"];

/// `privacy._names_private_path`: a path through a `private/` folder. The
/// bare word `private/` and macOS system paths (`/private/tmp`) do not count.
pub fn names_private_path(text_in: &str) -> bool {
    let t: Vec<char> = text_in.chars().collect();
    let head: Vec<char> = "private/".chars().collect();
    let mut i = 0;
    while i + head.len() <= t.len() {
        let starts = t[i..].starts_with(&head) && (i == 0 || !is_name(t[i - 1]));
        let name_end = if starts {
            let from = i + head.len();
            from + t[from..].iter().take_while(|&&c| is_name(c)).count()
        } else {
            0
        };
        if !starts || name_end == i + head.len() {
            i += 1;
            continue;
        }
        let name: String = t[i + head.len()..name_end].iter().collect();
        let path_start = i == 1 || (i >= 2 && opens_path(t[i - 2]));
        let system_root =
            i >= 1 && t[i - 1] == '/' && path_start && SYSTEM_DIRS.contains(&name.as_str());
        if !system_root {
            return true;
        }
        i = name_end;
    }
    false
}
