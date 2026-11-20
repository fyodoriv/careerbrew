//! The canary engine: `privacy.term_id`, `normalize`, `term_pattern`,
//! `find_leaks`, `leak_spans` and `_match_kind`.
//!
//! `term_pattern` builds a Python regular expression. This port builds the
//! same pattern text (its hash is a vector) and matches it with a small
//! backtracking matcher that follows `re`: leftmost match, greedy separator
//! runs, and `re.IGNORECASE` as simple lowercase plus the `EXTRA_CASES`
//! groups. No output of this module holds a term; spans hold the matched
//! text only for the caller to measure.

use crate::tables;
use crate::text;
use sha2::{Digest, Sha256};

/// `re.IGNORECASE | re.UNICODE`, the flags of every `term_pattern`.
pub const PATTERN_FLAGS: u32 = 2 | 32;

/// `privacy._SEP`.
const SEP: &str = r"[\s_\-.]*";

/// `privacy._LEET`, keyed by the lowercase letter.
const LEET: &[(&str, &str, &[char])] = &[
    ("o", "[o0]", &['o', '0']),
    ("i", "[i1l!]", &['i', '1', 'l', '!']),
    ("l", "[l1i]", &['l', '1', 'i']),
    ("e", "[e3]", &['e', '3']),
    ("a", "[a4@]", &['a', '4', '@']),
    ("s", "[s5$]", &['s', '5', '$']),
    ("t", "[t7]", &['t', '7']),
    ("b", "[b8]", &['b', '8']),
    ("g", "[g9]", &['g', '9']),
];

/// Hex SHA-256 of the UTF-8 bytes.
pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `privacy.term_id`.
pub fn term_id(term: &str) -> String {
    format!("t-{}", &sha256_hex(&text::lower(term))[..8])
}

/// `re.escape` (Python 3.7 and later) of one string.
fn re_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "()[]{}?*+-|^$\\.&~# \t\n\r\u{0b}\u{0c}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn leet(c: char) -> Option<&'static (&'static str, &'static str, &'static [char])> {
    let lo = text::lower(c.encode_utf8(&mut [0; 4]));
    LEET.iter().find(|row| row.0 == lo)
}

/// The rules `re.IGNORECASE` matches with. `python()` is CPython's; a
/// planted control removes one `EXTRA_CASES` group.
#[derive(Clone, Debug)]
pub struct Rules {
    extra_cases: Vec<&'static [u32]>,
    /// `access.MENTION`'s lookbehind keeps its `<` (planted defect: false).
    pub(crate) mention_angle: bool,
    /// `approvals.validate_records` refuses an unknown status (planted defect: false).
    pub(crate) status_check: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Self::python()
    }
}

impl Rules {
    pub fn python() -> Self {
        Rules {
            extra_cases: tables::EXTRA_CASES.to_vec(),
            mention_angle: true,
            status_check: true,
        }
    }

    /// The rules with the `<` removed from the mention lookbehind (a planted defect).
    pub fn without_mention_angle() -> Self {
        Rules {
            mention_angle: false,
            ..Self::python()
        }
    }

    /// The rules whose receipt validator accepts an unknown status (a planted defect).
    pub fn accepting_unknown_status() -> Self {
        Rules {
            status_check: false,
            ..Self::python()
        }
    }

    /// The rules without one `EXTRA_CASES` group (a planted defect).
    pub fn without_extra_case(index: usize) -> Self {
        let mut rules = Self::python();
        if index < rules.extra_cases.len() {
            rules.extra_cases.remove(index);
        }
        rules
    }

    /// The `re.IGNORECASE` class of a char.
    pub(crate) fn fold(&self, c: char) -> char {
        let lo = text::simple_lower(c);
        self.extra_cases
            .iter()
            .find(|group| group.contains(&(lo as u32)))
            .and_then(|group| char::from_u32(group[0]))
            .unwrap_or(lo)
    }

    fn pattern(&self, term: &str) -> Pattern<'_> {
        Pattern::new(self, term)
    }

    /// `privacy.find_leaks` over supplied terms.
    pub fn find_leaks(&self, text_in: &str, terms: &[String]) -> Vec<String> {
        let norm: Vec<char> = text::normalize(text_in).chars().collect();
        terms
            .iter()
            .filter(|t| self.pattern(t).search(&norm))
            .map(|t| term_id(t))
            .collect()
    }

    /// Every `finditer` match of one term's pattern as char (start, end).
    pub(crate) fn match_spans(&self, term: &str, chars: &[char]) -> Vec<(usize, usize)> {
        self.pattern(term).find_all(chars)
    }

    /// `privacy.leak_spans` over supplied terms: (term id, line, how, matched text).
    pub fn leak_spans(&self, text_in: &str, terms: &[String]) -> Vec<Span> {
        let norm: Vec<char> = text::normalize(text_in).chars().collect();
        let mut out = Vec::new();
        for t in terms {
            let tid = term_id(t);
            for (s, e) in self.pattern(t).find_all(&norm) {
                let line = norm[..s].iter().filter(|&&c| c == '\n').count() + 1;
                let got: String = norm[s..e].iter().collect();
                let how = match_kind(&norm, s, e, &got, t);
                out.push(Span {
                    term_id: tid.clone(),
                    line,
                    how,
                    matched: got,
                });
            }
        }
        // `sorted(key=(line, id))` is stable.
        out.sort_by(|a, b| (a.line, &a.term_id).cmp(&(b.line, &b.term_id)));
        out
    }
}

/// One `leak_spans` row. `matched` is for the owner's eyes only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub term_id: String,
    pub line: usize,
    pub how: String,
    pub matched: String,
}

/// `privacy.find_leaks` with CPython's rules.
pub fn find_leaks(text_in: &str, terms: &[String]) -> Vec<String> {
    Rules::python().find_leaks(text_in, terms)
}

/// `privacy.leak_spans` with CPython's rules.
pub fn leak_spans(text_in: &str, terms: &[String]) -> Vec<Span> {
    Rules::python().leak_spans(text_in, terms)
}

/// The source text of `privacy.term_pattern(term).pattern`.
pub fn term_pattern_source(term: &str) -> String {
    let core = text::normalize(term);
    if text::len(&core) >= 5 {
        core.chars()
            .filter(|&c| !text::is_space(c))
            .map(|c| match leet(c) {
                Some(row) => row.1.to_owned(),
                None => re_escape(c.encode_utf8(&mut [0; 4])),
            })
            .collect::<Vec<_>>()
            .join(SEP)
    } else {
        re_escape(&core)
    }
}

enum Atom {
    Lit(char),
    Class(&'static [char]),
}

/// One compiled `term_pattern`.
struct Pattern<'r> {
    rules: &'r Rules,
    atoms: Vec<Atom>,
    spaced: bool,
}

impl<'r> Pattern<'r> {
    fn new(rules: &'r Rules, term: &str) -> Self {
        let core = text::normalize(term);
        let spaced = text::len(&core) >= 5;
        let atoms = if spaced {
            core.chars()
                .filter(|&c| !text::is_space(c))
                .map(|c| match leet(c) {
                    Some(row) => Atom::Class(row.2),
                    None => Atom::Lit(c),
                })
                .collect()
        } else {
            core.chars().map(Atom::Lit).collect()
        };
        Pattern {
            rules,
            atoms,
            spaced,
        }
    }

    fn atom(&self, atom: &Atom, got: char) -> bool {
        let got = self.rules.fold(got);
        match atom {
            Atom::Lit(want) => self.rules.fold(*want) == got,
            Atom::Class(members) => members.iter().any(|&m| self.rules.fold(m) == got),
        }
    }

    fn is_sep(c: char) -> bool {
        text::is_space(c) || matches!(c, '_' | '-' | '.')
    }

    /// The end of a greedy match of atoms[i..] from text[p], with backtracking.
    fn from(&self, text_in: &[char], i: usize, p: usize) -> Option<usize> {
        if p >= text_in.len() || !self.atom(&self.atoms[i], text_in[p]) {
            return None;
        }
        let p = p + 1;
        if i + 1 == self.atoms.len() {
            return Some(p);
        }
        let mut most = 0;
        if self.spaced {
            while p + most < text_in.len() && Self::is_sep(text_in[p + most]) {
                most += 1;
            }
        }
        (0..=most)
            .rev()
            .find_map(|skip| self.from(text_in, i + 1, p + skip))
    }

    fn search(&self, text_in: &[char]) -> bool {
        self.atoms.is_empty() || (0..text_in.len()).any(|s| self.from(text_in, 0, s).is_some())
    }

    /// `finditer`: every leftmost, non-overlapping match as (start, end).
    /// An empty pattern matches empty at every position, like `re`.
    fn find_all(&self, text_in: &[char]) -> Vec<(usize, usize)> {
        if self.atoms.is_empty() {
            return (0..=text_in.len()).map(|p| (p, p)).collect();
        }
        let mut out = Vec::new();
        let mut s = 0;
        while s < text_in.len() {
            match self.from(text_in, 0, s) {
                Some(e) => {
                    out.push((s, e));
                    s = e;
                }
                None => s += 1,
            }
        }
        out
    }
}

/// `privacy._match_kind`.
fn match_kind(norm: &[char], start: usize, end: usize, got: &str, term: &str) -> String {
    let core = text::strip(term);
    if got.contains('\n') {
        return format!(
            "letters spread over {} lines, likely a false match",
            got.matches('\n').count() + 1
        );
    }
    let words = text::split(got).len();
    if words > text::split(core).len() {
        return format!("letters spread across {words} words, likely a false match");
    }
    if (start > 0 && text::is_alnum(norm[start - 1]))
        || (end < norm.len() && text::is_alnum(norm[end]))
    {
        return "inside a longer word, likely a false match".into();
    }
    if text::lower(got) == text::lower(core) {
        "whole word".into()
    } else {
        "look-alike spelling".into()
    }
}
