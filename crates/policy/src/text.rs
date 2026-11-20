//! Python `str` rules on Python's own Unicode tables (`tables.rs`).
//!
//! Rust's `char` methods use Rust's Unicode version and Rust's definitions
//! (`is_whitespace`, `is_alphabetic`, `to_lowercase`). The policy decisions
//! must answer like CPython, so every predicate here reads the generated
//! tables instead.

use crate::tables;
use unicode_normalization::UnicodeNormalization;

fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let n = c as u32;
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < n {
                std::cmp::Ordering::Less
            } else if lo > n {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Assigned chars that `str.isprintable` refuses under Unicode 15.0.0:
/// categories Cc, Cf, Zs (but the space), Zl, Zp and Co. Computed with
/// `unicodedata` 15.0.0 and checked against `str.isprintable` for every
/// code point (0 differences).
const NOT_PRINTABLE: &[(u32, u32)] = &[
    (0x0000, 0x001f),
    (0x007f, 0x00a0),
    (0x00ad, 0x00ad),
    (0x0600, 0x0605),
    (0x061c, 0x061c),
    (0x06dd, 0x06dd),
    (0x070f, 0x070f),
    (0x0890, 0x0891),
    (0x08e2, 0x08e2),
    (0x1680, 0x1680),
    (0x180e, 0x180e),
    (0x2000, 0x200f),
    (0x2028, 0x202f),
    (0x205f, 0x2064),
    (0x2066, 0x206f),
    (0x3000, 0x3000),
    (0xe000, 0xf8ff),
    (0xfeff, 0xfeff),
    (0xfff9, 0xfffb),
    (0x110bd, 0x110bd),
    (0x110cd, 0x110cd),
    (0x13430, 0x1343f),
    (0x1bca0, 0x1bca3),
    (0x1d173, 0x1d17a),
    (0xe0001, 0xe0001),
    (0xe0020, 0xe007f),
    (0xf0000, 0xffffd),
    (0x100000, 0x10fffd),
];

/// Python `str.isprintable` for one char.
pub fn is_printable(c: char) -> bool {
    in_ranges(tables::ASSIGNED, c) && !in_ranges(NOT_PRINTABLE, c)
}

/// `str.isspace` for one char (also `re` `\s`).
pub fn is_space(c: char) -> bool {
    in_ranges(tables::SPACE, c)
}

/// `str.isalnum` for one char.
pub fn is_alnum(c: char) -> bool {
    in_ranges(tables::ALNUM, c)
}

/// `str.isalpha` for one char.
pub fn is_alpha(c: char) -> bool {
    in_ranges(tables::ALPHA, c)
}

/// `str.isdigit` for one char.
pub fn is_digit(c: char) -> bool {
    in_ranges(tables::DIGIT, c)
}

/// `re` `\d` in a str pattern for one char.
pub fn is_decimal(c: char) -> bool {
    in_ranges(tables::DECIMAL, c)
}

fn lower_row(c: char) -> Option<&'static (u32, u32, &'static str)> {
    let n = c as u32;
    tables::LOWER
        .binary_search_by_key(&n, |row| row.0)
        .ok()
        .map(|i| &tables::LOWER[i])
}

/// `re`'s simple lowercase of one char (`_sre.unicode_tolower`).
pub fn simple_lower(c: char) -> char {
    lower_row(c)
        .and_then(|row| char::from_u32(row.1))
        .unwrap_or(c)
}

/// `str.lower()`. Each char maps by Python's table; a capital sigma takes
/// its final or medial form from Rust's own Final_Sigma rule, which reads
/// the same Unicode properties.
pub fn lower(s: &str) -> String {
    let rust: Vec<char> = if s.contains('\u{3a3}') {
        s.to_lowercase().chars().collect()
    } else {
        Vec::new()
    };
    let mut cursor = 0;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '\u{3a3}' {
            out.push(rust.get(cursor).copied().unwrap_or('\u{3c3}'));
        } else {
            match lower_row(c) {
                Some(row) => out.push_str(row.2),
                None => out.push(c),
            }
        }
        cursor += c.to_lowercase().count();
    }
    out
}

/// `str.strip()` with no argument.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str.strip(chars)`.
pub fn strip_chars<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c| chars.contains(c))
}

/// `str.split()` with no argument.
pub fn split(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|w| !w.is_empty()).collect()
}

/// `len(s)`: code points.
pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// `str.splitlines()`: every line boundary Python knows, `\r\n` as one.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let brk = matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !brk {
            continue;
        }
        out.push(&text[start..i]);
        let mut end = i + c.len_utf8();
        if c == '\r' {
            if let Some(&(j, '\n')) = it.peek() {
                it.next();
                end = j + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn nfkc_into(out: &mut String, segment: &mut String) {
    out.extend(segment.nfkc());
    segment.clear();
}

/// `privacy.normalize`: NFKC, then no zero-width characters.
///
/// The NFKC tables of `unicode-normalization` are newer than Python's.
/// Unicode's normalization stability policy keeps every char assigned in
/// Python's version normalizing the same way. A char unassigned there is a
/// starter that Python leaves as it is and that composes with nothing, so
/// the text is cut at each such char and only the pieces between are
/// normalized.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut segment = String::new();
    for c in text.chars() {
        if in_ranges(tables::ASSIGNED, c) {
            segment.push(c);
        } else {
            nfkc_into(&mut out, &mut segment);
            out.push(c);
        }
    }
    nfkc_into(&mut out, &mut segment);
    out.retain(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'));
    out
}
