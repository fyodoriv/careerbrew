//! The text `yaml.safe_dump(data, sort_keys=False, allow_unicode=True)`
//! writes (PyYAML 6, the pure-Python `SafeDumper`: width 80, indent 2,
//! `\n` line breaks), the writer behind `store.write_yaml`.
//!
//! This is a port of PyYAML's representer, resolver and emitter for the
//! values `Canon` holds. It keeps the emitter's own state (column, indent
//! stack, the `whitespace`, `indention` and `open_ended` flags) and its
//! scalar writers line by line, so folding at 80 columns and the indent of
//! `- ` items follow the same steps.
//!
//! `safe_dump` returns None for any input it cannot prove identical, so a
//! caller refuses and Python runs:
//! - a map key that is a list, a map, or a float (a float key can equal an
//!   int key in Python), or two keys that are equal in Python (`true`, `1`);
//! - a datetime that is not in `isoformat()` shape, or whose zone has
//!   seconds (PyYAML would tag it `!!timestamp`);
//! - a non-string scalar whose text the resolver would not read back with
//!   its own tag, or that cannot be written plain (it would get a tag);
//! - nesting deeper than `MAX_DEPTH`.
//!
//! Limit: PyYAML writes an anchor (`&id001`) for a list, map or date that
//! appears twice in one Python object by identity. `Canon` has no identity,
//! so this writes what PyYAML writes for data without shared objects.
use careerbrew_vault::canon::Canon;

const WIDTH: usize = 80;
const INDENT: usize = 2;
const MAX_DEPTH: usize = 64;
const BREAKS: [char; 4] = ['\n', '\u{85}', '\u{2028}', '\u{2029}'];
const SPACE_OR_BREAK: [char; 8] = [
    '\0', ' ', '\t', '\r', '\n', '\u{85}', '\u{2028}', '\u{2029}',
];

/// `yaml.safe_dump(value, sort_keys=False, allow_unicode=True)`, or None.
pub fn safe_dump(value: &Canon) -> Option<String> {
    let node = node(value, 0)?;
    let mut e = Emitter::new();
    e.node(&node, Ctx::Root)?;
    // expect_document_end, then the stream end.
    e.write_indent();
    if e.open_ended {
        e.write_indicator("...", true, false, false);
        e.write_indent();
    }
    Some(e.out)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tag {
    Str,
    Bool,
    Float,
    Int,
    Merge,
    Null,
    Timestamp,
    Value,
}

impl Tag {
    /// The length of the shorthand (`!!str`) `check_simple_key` counts.
    fn shorthand_len(self) -> usize {
        2 + match self {
            Tag::Str | Tag::Int => 3,
            Tag::Bool | Tag::Null => 4,
            Tag::Float | Tag::Value | Tag::Merge => 5,
            Tag::Timestamp => 9,
        }
    }
}

enum Node {
    Scalar {
        text: Vec<char>,
        tag: Tag,
        /// The plain text resolves to this node's own tag.
        implicit: bool,
    },
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

fn scalar(text: String, tag: Tag) -> Option<Node> {
    let implicit = resolve(&text) == tag;
    // A non-string scalar always resolves to its own tag in PyYAML's
    // output; anything else would carry an explicit tag.
    if tag != Tag::Str && !implicit {
        return None;
    }
    Some(Node::Scalar {
        text: text.chars().collect(),
        tag,
        implicit,
    })
}

fn node(value: &Canon, depth: usize) -> Option<Node> {
    if depth > MAX_DEPTH {
        return None;
    }
    match value {
        Canon::Null => scalar("null".into(), Tag::Null),
        Canon::Bool(b) => scalar(if *b { "true" } else { "false" }.into(), Tag::Bool),
        Canon::Int(n) => scalar(n.to_string(), Tag::Int),
        Canon::Float(f) => scalar(float_text(*f)?, Tag::Float),
        Canon::Str(s) => scalar(s.clone(), Tag::Str),
        Canon::Date(d) => {
            let b = d.as_bytes();
            let shape = b.len() == 10
                && b.iter().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        *c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                });
            if !shape {
                return None;
            }
            scalar(d.clone(), Tag::Timestamp)
        }
        Canon::DateTime(d) => scalar(datetime_text(d)?, Tag::Timestamp),
        Canon::List(items) => Some(Node::Seq(
            items
                .iter()
                .map(|item| node(item, depth + 1))
                .collect::<Option<_>>()?,
        )),
        Canon::Map(pairs) => {
            let mut seen: Vec<Canon> = Vec::new();
            let mut out = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                // Python's dict equality: `True == 1`, `False == 0`.
                let id = match k {
                    Canon::Str(_) | Canon::Null | Canon::Int(_) | Canon::Date(_) => k.clone(),
                    Canon::DateTime(_) => k.clone(),
                    Canon::Bool(b) => Canon::Int(u8::from(*b).into()),
                    _ => return None,
                };
                if seen.contains(&id) {
                    return None;
                }
                seen.push(id);
                out.push((node(k, depth + 1)?, node(v, depth + 1)?));
            }
            Some(Node::Map(out))
        }
    }
}

/// `datetime.isoformat(' ')` from the `isoformat()` text `Canon` holds:
/// `YYYY-MM-DDTHH:MM:SS[.ffffff][+HH:MM]`.
fn datetime_text(iso: &str) -> Option<String> {
    let b = iso.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    let base = b.len() >= 19
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    if !base {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        // isoformat() writes six digits, and none for a zero microsecond.
        if !digits(i + 1..i + 7) || &b[i + 1..i + 7] == b"000000" {
            return None;
        }
        i += 7;
    }
    if i < b.len() {
        let zone = &b[i..];
        let ok = zone.len() == 6
            && (zone[0] == b'+' || zone[0] == b'-')
            && digits(i + 1..i + 3)
            && zone[3] == b':'
            && digits(i + 4..i + 6)
            && zone != b"-00:00";
        if !ok {
            return None;
        }
    }
    Some(format!("{} {}", &iso[..10], &iso[11..]))
}

/// `repr(f).lower()` as PyYAML's `represent_float` adjusts it, or None.
fn float_text(f: f64) -> Option<String> {
    if f.is_nan() {
        return Some(".nan".into());
    }
    if f.is_infinite() {
        return Some(if f > 0.0 { ".inf" } else { "-.inf" }.into());
    }
    // repr gives the shortest digits that round-trip and, of those, the
    // ones nearest the value. Rust's `{:e}` gives the shortest length but
    // not always the nearest digits (`1166315293561431.3` for `.2`), so
    // take its length and round correctly to that many digits.
    let shortest = format!("{f:e}");
    let (m, _) = shortest.split_once('e')?;
    let len = m.chars().filter(char::is_ascii_digit).count();
    let sci = format!("{f:.*e}", len.saturating_sub(1));
    if sci.parse::<f64>().ok()? != f {
        return None;
    }
    let (mantissa, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let decpt = exp + 1;
    let body = if decpt <= -4 || decpt > 16 {
        let mut m = digits[..1].to_owned();
        if n > 1 {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        let e_text = format!("e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
        if m.contains('.') {
            format!("{m}{e_text}")
        } else {
            // PyYAML: `1e+16` is not a YAML float, so it writes `1.0e+16`.
            format!("{m}.0{e_text}")
        }
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat((-decpt) as usize))
    } else if decpt >= n {
        format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
    } else {
        format!(
            "{}.{}",
            &digits[..decpt as usize],
            &digits[decpt as usize..]
        )
    };
    Some(format!("{sign}{body}"))
}

// ---------- the YAML 1.1 implicit resolver (SafeLoader) ----------

/// The tag PyYAML's resolver gives plain text: the resolvers registered
/// for its first character, in registration order (bool, float, int,
/// merge, null, timestamp, value).
fn resolve(text: &str) -> Tag {
    let first = text.as_bytes().first().copied();
    let order: &[Tag] = match first {
        None => &[Tag::Null],
        Some(b'y' | b'Y' | b't' | b'T' | b'f' | b'F' | b'o' | b'O') => &[Tag::Bool],
        Some(b'n' | b'N') => &[Tag::Bool, Tag::Null],
        Some(b'-' | b'+') => &[Tag::Float, Tag::Int],
        Some(b'0'..=b'9') => &[Tag::Float, Tag::Int, Tag::Timestamp],
        Some(b'.') => &[Tag::Float],
        Some(b'<') => &[Tag::Merge],
        Some(b'~') => &[Tag::Null],
        Some(b'=') => &[Tag::Value],
        _ => &[],
    };
    for tag in order {
        let full: fn(&[u8]) -> bool = match tag {
            Tag::Bool => is_bool,
            Tag::Float => is_float,
            Tag::Int => is_int,
            Tag::Merge => |b| b == b"<<",
            Tag::Null => |b| matches!(b, b"" | b"~" | b"null" | b"Null" | b"NULL"),
            Tag::Timestamp => is_timestamp,
            Tag::Value => |b| b == b"=",
            Tag::Str => unreachable!(),
        };
        // Python's `$` also matches before one final newline.
        let b = text.as_bytes();
        if full(b) || (b.last() == Some(&b'\n') && full(&b[..b.len() - 1])) {
            return *tag;
        }
    }
    Tag::Str
}

fn is_bool(b: &[u8]) -> bool {
    matches!(
        b,
        b"yes"
            | b"Yes"
            | b"YES"
            | b"no"
            | b"No"
            | b"NO"
            | b"true"
            | b"True"
            | b"TRUE"
            | b"false"
            | b"False"
            | b"FALSE"
            | b"on"
            | b"On"
            | b"ON"
            | b"off"
            | b"Off"
            | b"OFF"
    )
}

/// The end of a run of bytes in `class` from `i`.
fn run(b: &[u8], i: usize, class: impl Fn(u8) -> bool) -> usize {
    let mut j = i;
    while j < b.len() && class(b[j]) {
        j += 1;
    }
    j
}

fn digit_us(c: u8) -> bool {
    c.is_ascii_digit() || c == b'_'
}

fn sign(b: &[u8], i: usize) -> usize {
    if matches!(b.get(i), Some(b'-' | b'+')) {
        i + 1
    } else {
        i
    }
}

/// `[eE][-+][0-9]+` from `i`, or `i` when absent; None when it starts and fails.
fn exponent(b: &[u8], i: usize) -> Option<usize> {
    if !matches!(b.get(i), Some(b'e' | b'E')) {
        return Some(i);
    }
    if !matches!(b.get(i + 1), Some(b'-' | b'+')) {
        return None;
    }
    let j = run(b, i + 2, |c| c.is_ascii_digit());
    (j > i + 2).then_some(j)
}

/// One or more `:[0-5]?[0-9]` groups from `i`; the end, or None.
fn sexagesimal(b: &[u8], mut i: usize) -> Option<usize> {
    let mut groups = 0;
    while b.get(i) == Some(&b':') {
        let j = run(b, i + 1, |c| c.is_ascii_digit());
        let ok = j == i + 2 || (j == i + 3 && b[i + 1] <= b'5');
        if !ok {
            return None;
        }
        i = j;
        groups += 1;
    }
    (groups > 0).then_some(i)
}

fn is_float(b: &[u8]) -> bool {
    let n = b.len();
    // [-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?
    let a = || {
        let i = sign(b, 0);
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        let i = run(b, i + 1, digit_us);
        if b.get(i) != Some(&b'.') {
            return false;
        }
        exponent(b, run(b, i + 1, digit_us)) == Some(n)
    };
    // \.[0-9][0-9_]*(?:[eE][-+][0-9]+)?
    let dot = || {
        b.first() == Some(&b'.')
            && b.get(1).is_some_and(u8::is_ascii_digit)
            && exponent(b, run(b, 2, digit_us)) == Some(n)
    };
    // [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
    let base60 = || {
        let i = sign(b, 0);
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return false;
        }
        match sexagesimal(b, run(b, i + 1, digit_us)) {
            Some(j) => b.get(j) == Some(&b'.') && run(b, j + 1, digit_us) == n,
            None => false,
        }
    };
    let inf = || matches!(&b[sign(b, 0)..], b".inf" | b".Inf" | b".INF");
    let nan = || matches!(b, b".nan" | b".NaN" | b".NAN");
    a() || dot() || base60() || inf() || nan()
}

fn is_int(b: &[u8]) -> bool {
    let n = b.len();
    let i = sign(b, 0);
    let rest = &b[i..];
    let after = |prefix: &[u8], class: fn(u8) -> bool| {
        rest.starts_with(prefix) && {
            let j = run(b, i + prefix.len(), class);
            j > i + prefix.len() && j == n
        }
    };
    if after(b"0b", |c| matches!(c, b'0' | b'1' | b'_')) {
        return true;
    }
    if after(b"0", |c| matches!(c, b'0'..=b'7' | b'_')) {
        return true;
    }
    if rest == b"0" {
        return true;
    }
    if after(b"0x", |c| c.is_ascii_hexdigit() || c == b'_') {
        return true;
    }
    if matches!(rest.first(), Some(b'1'..=b'9')) {
        let j = run(b, i + 1, digit_us);
        if j == n {
            return true;
        }
        if sexagesimal(b, j) == Some(n) {
            return true;
        }
    }
    false
}

fn is_timestamp(b: &[u8]) -> bool {
    let n = b.len();
    let d = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    if !(d(0) && d(1) && d(2) && d(3) && b.get(4) == Some(&b'-')) {
        return false;
    }
    if n == 10 && d(5) && d(6) && b[7] == b'-' && d(8) && d(9) {
        return true;
    }
    // One or two digits from `i`, then the end of that run.
    let one_or_two = |i: usize| {
        let j = run(b, i, |c| c.is_ascii_digit());
        (j == i + 1 || j == i + 2).then_some(j)
    };
    let Some(i) = one_or_two(5) else { return false };
    if b.get(i) != Some(&b'-') {
        return false;
    }
    let Some(mut i) = one_or_two(i + 1) else {
        return false;
    };
    match b.get(i) {
        Some(b'T' | b't') => i += 1,
        Some(b' ' | b'\t') => i = run(b, i, |c| c == b' ' || c == b'\t'),
        _ => return false,
    }
    let Some(i) = one_or_two(i) else { return false };
    let two = |i: usize| b.get(i) == Some(&b':') && d(i + 1) && d(i + 2);
    if !(two(i) && two(i + 3)) {
        return false;
    }
    let mut i = i + 6;
    if b.get(i) == Some(&b'.') {
        i = run(b, i + 1, |c| c.is_ascii_digit());
    }
    if i == n {
        return true;
    }
    // (?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?$
    let i = run(b, i, |c| c == b' ' || c == b'\t');
    match b.get(i) {
        Some(b'Z') => i + 1 == n,
        Some(b'-' | b'+') => match one_or_two(i + 1) {
            Some(j) if j == n => true,
            Some(j) => two(j) && j + 3 == n,
            None => false,
        },
        _ => false,
    }
}

// ---------- the emitter ----------

struct Analysis {
    empty: bool,
    multiline: bool,
    allow_block_plain: bool,
    allow_single_quoted: bool,
}

/// `Emitter.analyze_scalar` with `allow_unicode=True`, for block context.
fn analyze(s: &[char]) -> Analysis {
    if s.is_empty() {
        return Analysis {
            empty: true,
            multiline: false,
            allow_block_plain: true,
            allow_single_quoted: true,
        };
    }
    let ws = |c: char| SPACE_OR_BREAK.contains(&c);
    let mut block_indicators = false;
    let mut line_breaks = false;
    let mut special = false;
    let (mut leading_space, mut leading_break) = (false, false);
    let (mut trailing_space, mut trailing_break) = (false, false);
    let (mut break_space, mut space_break) = (false, false);
    let starts = |p: &str| s.len() >= 3 && s[..3].iter().copied().eq(p.chars());
    if starts("---") || starts("...") {
        block_indicators = true;
    }
    let mut preceded_by_ws = true;
    let mut followed_by_ws = s.len() == 1 || ws(s[1]);
    let (mut previous_space, mut previous_break) = (false, false);
    let last = s.len() - 1;
    for (index, &ch) in s.iter().enumerate() {
        if index == 0 {
            if "#,[]{}&*!|>'\"%@`".contains(ch) {
                block_indicators = true;
            }
            if (ch == '?' || ch == ':') && followed_by_ws {
                block_indicators = true;
            }
            if ch == '-' && followed_by_ws {
                block_indicators = true;
            }
        } else {
            if ch == ':' && followed_by_ws {
                block_indicators = true;
            }
            if ch == '#' && preceded_by_ws {
                block_indicators = true;
            }
        }
        if BREAKS.contains(&ch) {
            line_breaks = true;
        }
        if !(ch == '\n' || (' '..='~').contains(&ch)) {
            let unicode = (ch == '\u{85}'
                || ('\u{A0}'..='\u{D7FF}').contains(&ch)
                || ('\u{E000}'..='\u{FFFD}').contains(&ch)
                || ('\u{10000}'..'\u{10FFFF}').contains(&ch))
                && ch != '\u{FEFF}';
            if !unicode {
                special = true;
            }
        }
        if ch == ' ' {
            leading_space |= index == 0;
            trailing_space |= index == last;
            break_space |= previous_break;
            previous_space = true;
            previous_break = false;
        } else if BREAKS.contains(&ch) {
            leading_break |= index == 0;
            trailing_break |= index == last;
            space_break |= previous_space;
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
        preceded_by_ws = ws(ch);
        followed_by_ws = index + 2 >= s.len() || ws(s[index + 2]);
    }
    let mut allow_block_plain = true;
    let mut allow_single_quoted = true;
    if leading_space || leading_break || trailing_space || trailing_break {
        allow_block_plain = false;
    }
    if break_space {
        allow_block_plain = false;
        allow_single_quoted = false;
    }
    if space_break || special {
        allow_block_plain = false;
        allow_single_quoted = false;
    }
    if line_breaks || block_indicators {
        allow_block_plain = false;
    }
    Analysis {
        empty: false,
        multiline: line_breaks,
        allow_block_plain,
        allow_single_quoted,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Root,
    Sequence,
    Mapping,
    SimpleKey,
}

struct Emitter {
    out: String,
    column: usize,
    whitespace: bool,
    indention: bool,
    open_ended: bool,
    indent: Option<usize>,
    indents: Vec<Option<usize>>,
}

impl Emitter {
    fn new() -> Emitter {
        Emitter {
            out: String::new(),
            column: 0,
            whitespace: true,
            indention: true,
            open_ended: false,
            indent: None,
            indents: Vec::new(),
        }
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        self.indents.push(self.indent);
        self.indent = match self.indent {
            None => Some(if flow { INDENT } else { 0 }),
            Some(i) if !indentless => Some(i + INDENT),
            same => same,
        };
    }

    fn pop_indent(&mut self) {
        self.indent = self.indents.pop().flatten();
    }

    fn text(&mut self, chars: &[char]) {
        self.column += chars.len();
        self.out.extend(chars);
    }

    fn write_indicator(&mut self, ind: &str, need_ws: bool, whitespace: bool, indention: bool) {
        if !(self.whitespace || !need_ws) {
            self.out.push(' ');
            self.column += 1;
        }
        self.out.push_str(ind);
        self.column += ind.chars().count();
        self.whitespace = whitespace;
        self.indention = self.indention && indention;
        self.open_ended = false;
    }

    fn write_indent(&mut self) {
        let indent = self.indent.unwrap_or(0);
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.write_line_break(None);
        }
        if self.column < indent {
            self.whitespace = true;
            self.out
                .extend(std::iter::repeat_n(' ', indent - self.column));
            self.column = indent;
        }
    }

    fn write_line_break(&mut self, data: Option<char>) {
        self.out.push(data.unwrap_or('\n'));
        self.whitespace = true;
        self.indention = true;
        self.column = 0;
    }

    fn node(&mut self, node: &Node, ctx: Ctx) -> Option<()> {
        match node {
            Node::Scalar {
                text,
                tag,
                implicit,
            } => {
                self.increase_indent(true, false);
                self.scalar(text, *tag, *implicit, ctx)?;
                self.pop_indent();
            }
            Node::Seq(items) if items.is_empty() => self.empty_flow("[", "]"),
            Node::Map(pairs) if pairs.is_empty() => self.empty_flow("{", "}"),
            Node::Seq(items) => {
                let indentless = ctx == Ctx::Mapping && !self.indention;
                self.increase_indent(false, indentless);
                for item in items {
                    self.write_indent();
                    self.write_indicator("-", true, false, true);
                    self.node(item, Ctx::Sequence)?;
                }
                self.pop_indent();
            }
            Node::Map(pairs) => {
                self.increase_indent(false, false);
                for (k, v) in pairs {
                    self.write_indent();
                    if simple_key(k) {
                        self.node(k, Ctx::SimpleKey)?;
                        self.write_indicator(":", false, false, false);
                    } else {
                        self.write_indicator("?", true, false, true);
                        self.node(k, Ctx::Mapping)?;
                        self.write_indent();
                        self.write_indicator(":", true, false, true);
                    }
                    self.node(v, Ctx::Mapping)?;
                }
                self.pop_indent();
            }
        }
        Some(())
    }

    fn empty_flow(&mut self, open: &str, close: &str) {
        self.write_indicator(open, true, true, false);
        self.increase_indent(true, false);
        self.pop_indent();
        self.write_indicator(close, false, false, false);
    }

    fn scalar(&mut self, text: &[char], tag: Tag, implicit: bool, ctx: Ctx) -> Option<()> {
        let a = analyze(text);
        let simple = ctx == Ctx::SimpleKey;
        let plain = implicit && !(simple && (a.empty || a.multiline)) && a.allow_block_plain;
        let split = !simple;
        if plain {
            self.write_plain(text, split, ctx == Ctx::Root);
            return Some(());
        }
        // A quoted non-string scalar would carry an explicit tag.
        if tag != Tag::Str {
            return None;
        }
        if a.allow_single_quoted && !(simple && a.multiline) {
            self.write_single_quoted(text, split);
        } else {
            self.write_double_quoted(text, split);
        }
        Some(())
    }

    fn write_plain(&mut self, text: &[char], split: bool, root: bool) {
        if root {
            self.open_ended = true;
        }
        if text.is_empty() {
            return;
        }
        if !self.whitespace {
            self.out.push(' ');
            self.column += 1;
        }
        self.whitespace = false;
        self.indention = false;
        let (mut spaces, mut breaks) = (false, false);
        let mut start = 0;
        for end in 0..=text.len() {
            let ch = text.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end && self.column > WIDTH && split {
                        self.write_indent();
                        self.whitespace = false;
                        self.indention = false;
                    } else {
                        self.text(&text[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if !ch.is_some_and(|c| BREAKS.contains(&c)) {
                    if text[start] == '\n' {
                        self.write_line_break(None);
                    }
                    for &br in &text[start..end] {
                        self.write_line_break((br != '\n').then_some(br));
                    }
                    self.write_indent();
                    self.whitespace = false;
                    self.indention = false;
                    start = end;
                }
            } else if ch.is_none_or(|c| c == ' ' || BREAKS.contains(&c)) {
                self.text(&text[start..end]);
                start = end;
            }
            if let Some(c) = ch {
                spaces = c == ' ';
                breaks = BREAKS.contains(&c);
            }
        }
    }

    fn write_single_quoted(&mut self, text: &[char], split: bool) {
        self.write_indicator("'", true, false, false);
        let (mut spaces, mut breaks) = (false, false);
        let mut start = 0;
        for end in 0..=text.len() {
            let ch = text.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end
                        && self.column > WIDTH
                        && split
                        && start != 0
                        && end != text.len()
                    {
                        self.write_indent();
                    } else {
                        self.text(&text[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if !ch.is_some_and(|c| BREAKS.contains(&c)) {
                    if text[start] == '\n' {
                        self.write_line_break(None);
                    }
                    for &br in &text[start..end] {
                        self.write_line_break((br != '\n').then_some(br));
                    }
                    self.write_indent();
                    start = end;
                }
            } else if ch.is_none_or(|c| c == ' ' || c == '\'' || BREAKS.contains(&c)) && start < end
            {
                self.text(&text[start..end]);
                start = end;
            }
            if ch == Some('\'') {
                self.out.push_str("''");
                self.column += 2;
                start = end + 1;
            }
            if let Some(c) = ch {
                spaces = c == ' ';
                breaks = BREAKS.contains(&c);
            }
        }
        self.write_indicator("'", false, false, false);
    }

    fn write_double_quoted(&mut self, text: &[char], split: bool) {
        self.write_indicator("\"", true, false, false);
        let mut start = 0;
        for end in 0..=text.len() {
            let ch = text.get(end).copied();
            let raw = |c: char| {
                (' '..='~').contains(&c)
                    || ('\u{A0}'..='\u{D7FF}').contains(&c)
                    || ('\u{E000}'..='\u{FFFD}').contains(&c)
            };
            let escape = match ch {
                None => true,
                Some(c) => "\"\\\u{85}\u{2028}\u{2029}\u{FEFF}".contains(c) || !raw(c),
            };
            if escape {
                if start < end {
                    self.text(&text[start..end]);
                    start = end;
                }
                if let Some(c) = ch {
                    let data = match c {
                        '\0' => "\\0".to_owned(),
                        '\u{07}' => "\\a".to_owned(),
                        '\u{08}' => "\\b".to_owned(),
                        '\t' => "\\t".to_owned(),
                        '\n' => "\\n".to_owned(),
                        '\u{0B}' => "\\v".to_owned(),
                        '\u{0C}' => "\\f".to_owned(),
                        '\r' => "\\r".to_owned(),
                        '\u{1B}' => "\\e".to_owned(),
                        '"' => "\\\"".to_owned(),
                        '\\' => "\\\\".to_owned(),
                        '\u{85}' => "\\N".to_owned(),
                        '\u{A0}' => "\\_".to_owned(),
                        '\u{2028}' => "\\L".to_owned(),
                        '\u{2029}' => "\\P".to_owned(),
                        c if (c as u32) <= 0xFF => format!("\\x{:02X}", c as u32),
                        c if (c as u32) <= 0xFFFF => format!("\\u{:04X}", c as u32),
                        c => format!("\\U{:08X}", c as u32),
                    };
                    self.column += data.chars().count();
                    self.out.push_str(&data);
                    start = end + 1;
                }
            }
            if 0 < end
                && end + 1 < text.len()
                && (ch == Some(' ') || start >= end)
                && self.column as isize + end as isize - start as isize > WIDTH as isize
                && split
            {
                if start < end {
                    self.text(&text[start..end]);
                    start = end;
                }
                self.out.push('\\');
                self.column += 1;
                self.write_indent();
                self.whitespace = false;
                self.indention = false;
                if text[start] == ' ' {
                    self.out.push('\\');
                    self.column += 1;
                }
            }
        }
        self.write_indicator("\"", false, false, false);
    }
}

/// `Emitter.check_simple_key` for a scalar key: its tag shorthand and text
/// under 128 characters, and the text neither empty nor multi-line.
fn simple_key(node: &Node) -> bool {
    match node {
        Node::Scalar { text, tag, .. } => {
            let a = analyze(text);
            tag.shorthand_len() + text.len() < 128 && !a.empty && !a.multiline
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Canon {
        Canon::Str(text.into())
    }

    fn map(pairs: Vec<(&str, Canon)>) -> Canon {
        Canon::Map(pairs.into_iter().map(|(k, v)| (s(k), v)).collect())
    }

    #[test]
    fn known_outputs() {
        let cases: Vec<(Canon, &str)> = vec![
            (s("foo"), "foo\n...\n"),
            (s(""), "''\n"),
            (s("yes"), "'yes'\n"),
            (Canon::Null, "null\n...\n"),
            (Canon::Map(vec![]), "{}\n"),
            (Canon::List(vec![]), "[]\n"),
            (
                map(vec![(
                    "a",
                    Canon::List(vec![s("x"), Canon::List(vec![s("y"), s("z")])]),
                )]),
                "a:\n- x\n- - y\n  - z\n",
            ),
            (
                map(vec![("t", s("12:30")), ("u", s("08:30"))]),
                "t: '12:30'\nu: 08:30\n",
            ),
            (Canon::Float(1e16), "1.0e+16\n...\n"),
            (Canon::Float(0.1), "0.1\n...\n"),
            (Canon::Float(1e-5), "1.0e-05\n...\n"),
            // Shortest length, nearest digits (Rust's `{:e}` says `.3`).
            (
                Canon::Float(1166315293561431.2),
                "1166315293561431.2\n...\n",
            ),
            (Canon::Date("2026-11-28".into()), "2026-11-28\n...\n"),
        ];
        for (value, want) in cases {
            assert_eq!(safe_dump(&value).as_deref(), Some(want), "{value:?}");
        }
    }

    #[test]
    fn refusals() {
        let float_key = Canon::Map(vec![(Canon::Float(1.0), Canon::Null)]);
        assert_eq!(safe_dump(&float_key), None);
        let dup = Canon::Map(vec![
            (Canon::Bool(true), Canon::Null),
            (Canon::Int(1.into()), Canon::Null),
        ]);
        assert_eq!(safe_dump(&dup), None);
        assert_eq!(
            safe_dump(&Canon::DateTime("2026-11-28 10:00:00".into())),
            None
        );
    }
}
