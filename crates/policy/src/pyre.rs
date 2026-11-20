//! A small backtracking matcher for the fixed `re` patterns of `privacy.py`
//! and `work/access.py` (rust-04e).
//!
//! It follows CPython's `re` for the features those patterns use: greedy
//! quantifiers, groups, alternation, classes, `\w`, `\s`, `\d` (and their
//! negations), `\b`, `^`, `$`, fixed-width lookbehind, lookahead and
//! `re.IGNORECASE`. `\w`, `\s` and `\d` read Python's Unicode tables
//! (`text.rs`). `sub` and `findall` step over matches like `re.sub`
//! (an empty match right after a match is allowed, never two at one place).
//! Any other syntax is refused when the pattern is built.

use crate::canary::Rules;
use crate::text;

#[derive(Clone, Debug)]
enum Item {
    Ch(char),
    Range(char, char),
    Word(bool),
    Space(bool),
    Digit(bool),
}

#[derive(Clone, Debug)]
enum Node {
    Ch(char),
    Any,
    Class(Vec<Item>, bool),
    Start,
    End,
    Boundary(bool),
    Group(Option<usize>, Box<Node>),
    Look {
        behind: bool,
        neg: bool,
        width: usize,
        node: Box<Node>,
    },
    Cat(Vec<Node>),
    Alt(Vec<Node>),
    Rep(Box<Node>, usize, usize),
}

const MANY: usize = usize::MAX;

/// Each group's (start, end), group 0 unused.
pub type Caps = Vec<Option<(usize, usize)>>;

/// One compiled pattern.
pub struct Regex {
    root: Node,
    groups: usize,
    icase: bool,
    rules: Rules,
}

struct Parser {
    s: Vec<char>,
    i: usize,
    groups: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn alt(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.cat()?];
        while self.eat('|') {
            branches.push(self.cat()?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap_or(Node::Cat(Vec::new()))
        } else {
            Node::Alt(branches)
        })
    }

    fn cat(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            items.push(self.quant(atom)?);
        }
        Ok(Node::Cat(items))
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        let digits: String = self.s[start..self.i].iter().collect();
        digits.parse().ok()
    }

    /// `{m}`, `{m,}`, `{,n}` or `{m,n}`; None (and no input used) otherwise.
    fn braces(&mut self) -> Option<(usize, usize)> {
        let save = self.i;
        if !self.eat('{') {
            return None;
        }
        let lo = self.number();
        let out = if self.eat(',') {
            let hi = self.number();
            self.eat('}').then(|| (lo.unwrap_or(0), hi.unwrap_or(MANY)))
        } else {
            lo.filter(|_| self.eat('}')).map(|n| (n, n))
        };
        if out.is_none() {
            self.i = save;
        }
        out
    }

    fn quant(&mut self, atom: Node) -> Result<Node, String> {
        let (lo, hi) = match self.peek() {
            Some('*') => {
                self.i += 1;
                (0, MANY)
            }
            Some('+') => {
                self.i += 1;
                (1, MANY)
            }
            Some('?') => {
                self.i += 1;
                (0, 1)
            }
            Some('{') => match self.braces() {
                Some(range) => range,
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        if matches!(self.peek(), Some('?' | '+')) {
            return Err("lazy or possessive quantifier".into());
        }
        Ok(Node::Rep(Box::new(atom), lo, hi))
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end")?;
        self.i += 1;
        Ok(match c {
            '(' => self.group()?,
            '[' => self.class()?,
            '\\' => self.escape()?,
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '*' | '+' | '?' => return Err("nothing to repeat".into()),
            c => Node::Ch(c),
        })
    }

    fn group(&mut self) -> Result<Node, String> {
        if !self.eat('?') {
            self.groups += 1;
            let index = self.groups;
            let inner = self.alt()?;
            return self.close(Node::Group(Some(index), Box::new(inner)));
        }
        let (behind, neg) = match self.peek() {
            Some(':') => {
                self.i += 1;
                let inner = self.alt()?;
                return self.close(Node::Group(None, Box::new(inner)));
            }
            Some('=') => (false, false),
            Some('!') => (false, true),
            Some('<') => {
                self.i += 1;
                match self.peek() {
                    Some('=') => (true, false),
                    Some('!') => (true, true),
                    _ => return Err("unsupported group".into()),
                }
            }
            _ => return Err("unsupported group".into()),
        };
        self.i += 1;
        let inner = self.alt()?;
        let width = if behind {
            width(&inner).ok_or("look-behind requires fixed-width pattern")?
        } else {
            0
        };
        self.close(Node::Look {
            behind,
            neg,
            width,
            node: Box::new(inner),
        })
    }

    fn close(&mut self, node: Node) -> Result<Node, String> {
        if self.eat(')') {
            Ok(node)
        } else {
            Err("missing )".into())
        }
    }

    fn category(c: char) -> Option<Item> {
        Some(match c {
            'w' => Item::Word(true),
            'W' => Item::Word(false),
            's' => Item::Space(true),
            'S' => Item::Space(false),
            'd' => Item::Digit(true),
            'D' => Item::Digit(false),
            _ => return None,
        })
    }

    fn escaped(&mut self) -> Result<char, String> {
        let c = self.peek().ok_or("trailing backslash")?;
        self.i += 1;
        match c {
            'n' => Ok('\n'),
            't' => Ok('\t'),
            c if c.is_ascii_alphanumeric() => Err("unsupported escape".into()),
            c => Ok(c),
        }
    }

    fn escape(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('b') => {
                self.i += 1;
                Ok(Node::Boundary(true))
            }
            Some('B') => {
                self.i += 1;
                Ok(Node::Boundary(false))
            }
            Some(c) if Self::category(c).is_some() => {
                self.i += 1;
                Ok(Node::Class(Self::category(c).into_iter().collect(), false))
            }
            _ => Ok(Node::Ch(self.escaped()?)),
        }
    }

    fn class_char(&mut self) -> Result<Result<char, Item>, String> {
        let c = self.peek().ok_or("unterminated class")?;
        self.i += 1;
        if c != '\\' {
            return Ok(Ok(c));
        }
        match self.peek().and_then(Self::category) {
            Some(item) => {
                self.i += 1;
                Ok(Err(item))
            }
            None => Ok(Ok(self.escaped()?)),
        }
    }

    fn class(&mut self) -> Result<Node, String> {
        let neg = self.eat('^');
        let mut items = Vec::new();
        let mut first = true;
        loop {
            if !first && self.eat(']') {
                break;
            }
            first = false;
            match self.class_char()? {
                Err(item) => items.push(item),
                Ok(lo) => {
                    if self.peek() == Some('-') && self.s.get(self.i + 1).is_some_and(|&c| c != ']')
                    {
                        self.i += 1;
                        match self.class_char()? {
                            Ok(hi) if hi >= lo => items.push(Item::Range(lo, hi)),
                            _ => return Err("bad character range".into()),
                        }
                    } else {
                        items.push(Item::Ch(lo));
                    }
                }
            }
        }
        Ok(Node::Class(items, neg))
    }
}

/// The fixed width of a node, None when it varies.
fn width(node: &Node) -> Option<usize> {
    match node {
        Node::Ch(_) | Node::Any | Node::Class(..) => Some(1),
        Node::Start | Node::End | Node::Boundary(_) | Node::Look { .. } => Some(0),
        Node::Group(_, inner) => width(inner),
        Node::Cat(items) => items.iter().map(width).sum(),
        Node::Alt(branches) => {
            let first = width(branches.first()?)?;
            branches
                .iter()
                .all(|b| width(b) == Some(first))
                .then_some(first)
        }
        Node::Rep(inner, lo, hi) if lo == hi => width(inner).map(|w| w * lo),
        Node::Rep(..) => None,
    }
}

/// `re` `\w` in a str pattern.
fn is_word(c: char) -> bool {
    c == '_' || text::is_alnum(c)
}

/// `_sre`'s simple uppercase (one char, else unchanged).
fn simple_upper(c: char) -> char {
    let mut up = c.to_uppercase();
    match (up.next(), up.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

impl Regex {
    /// Compile `src`; `icase` is `re.IGNORECASE`.
    pub fn new(src: &str, icase: bool, rules: &Rules) -> Result<Regex, String> {
        let mut p = Parser {
            s: src.chars().collect(),
            i: 0,
            groups: 0,
        };
        let root = p.alt()?;
        if p.i != p.s.len() {
            return Err("unbalanced parenthesis".into());
        }
        Ok(Regex {
            root,
            groups: p.groups,
            icase,
            rules: rules.clone(),
        })
    }

    fn same(&self, want: char, got: char) -> bool {
        if self.icase {
            self.rules.fold(want) == self.rules.fold(got)
        } else {
            want == got
        }
    }

    fn in_class(&self, items: &[Item], neg: bool, ch: char) -> bool {
        let lo = if self.icase {
            text::simple_lower(ch)
        } else {
            ch
        };
        let hit = items.iter().any(|item| match *item {
            Item::Ch(c) => self.same(c, ch),
            Item::Range(a, b) => {
                if self.icase {
                    (a..=b).contains(&lo) || (a..=b).contains(&simple_upper(lo))
                } else {
                    (a..=b).contains(&ch)
                }
            }
            Item::Word(pos) => is_word(lo) == pos,
            Item::Space(pos) => text::is_space(lo) == pos,
            Item::Digit(pos) => text::is_decimal(lo) == pos,
        });
        hit != neg
    }

    fn boundary(t: &[char], p: usize) -> bool {
        let before = p > 0 && is_word(t[p - 1]);
        let after = p < t.len() && is_word(t[p]);
        before != after
    }

    fn node(
        &self,
        t: &[char],
        n: &Node,
        p: usize,
        caps: &Caps,
        k: &mut dyn FnMut(usize, &Caps) -> bool,
    ) -> bool {
        match n {
            Node::Ch(c) => p < t.len() && self.same(*c, t[p]) && k(p + 1, caps),
            Node::Any => p < t.len() && t[p] != '\n' && k(p + 1, caps),
            Node::Class(items, neg) => {
                p < t.len() && self.in_class(items, *neg, t[p]) && k(p + 1, caps)
            }
            Node::Start => p == 0 && k(p, caps),
            Node::End => (p == t.len() || (p + 1 == t.len() && t[p] == '\n')) && k(p, caps),
            Node::Boundary(want) => Self::boundary(t, p) == *want && k(p, caps),
            Node::Group(index, inner) => self.node(t, inner, p, caps, &mut |e, c| match index {
                Some(i) => {
                    let mut c2 = c.clone();
                    c2[*i] = Some((p, e));
                    k(e, &c2)
                }
                None => k(e, c),
            }),
            Node::Look {
                behind,
                neg,
                width,
                node,
            } => {
                let start = if *behind {
                    p.checked_sub(*width)
                } else {
                    Some(p)
                };
                let found = start
                    .is_some_and(|s| self.node(t, node, s, caps, &mut |e, _| !*behind || e == p));
                found != *neg && k(p, caps)
            }
            Node::Cat(items) => self.cat(t, items, p, caps, k),
            Node::Alt(branches) => {
                for b in branches {
                    if self.node(t, b, p, caps, k) {
                        return true;
                    }
                }
                false
            }
            Node::Rep(inner, lo, hi) => match **inner {
                Node::Ch(_) | Node::Any | Node::Class(..) => {
                    self.rep_one(t, inner, *lo, *hi, p, caps, k)
                }
                _ => self.rep(t, inner, *lo, *hi, 0, p, caps, k),
            },
        }
    }

    fn cat(
        &self,
        t: &[char],
        items: &[Node],
        p: usize,
        caps: &Caps,
        k: &mut dyn FnMut(usize, &Caps) -> bool,
    ) -> bool {
        match items.split_first() {
            None => k(p, caps),
            Some((head, rest)) => {
                self.node(t, head, p, caps, &mut |e, c| self.cat(t, rest, e, c, k))
            }
        }
    }

    /// Does a one-char node (`Ch`, `Any`, `Class`) take `c`?
    fn one(&self, n: &Node, c: char) -> bool {
        match n {
            Node::Ch(want) => self.same(*want, c),
            Node::Any => c != '\n',
            Node::Class(items, neg) => self.in_class(items, *neg, c),
            _ => false,
        }
    }

    /// A greedy repeat of a one-char node, without recursion: count the
    /// longest run (up to `hi`), then hand each length from the longest
    /// down to `lo` to the continuation, like sre's `REPEAT_ONE`. A
    /// one-char node never matches empty, so the empty-match rule of `rep`
    /// does not apply. Stack use stays flat for a run of any length.
    #[allow(clippy::too_many_arguments)]
    fn rep_one(
        &self,
        t: &[char],
        inner: &Node,
        lo: usize,
        hi: usize,
        p: usize,
        caps: &Caps,
        k: &mut dyn FnMut(usize, &Caps) -> bool,
    ) -> bool {
        let room = hi.min(t.len() - p);
        let mut n = 0;
        while n < room && self.one(inner, t[p + n]) {
            n += 1;
        }
        if n < lo {
            return false;
        }
        loop {
            if k(p + n, caps) {
                return true;
            }
            if n == lo {
                return false;
            }
            n -= 1;
        }
    }

    /// A greedy repeat of any other node. It recurses once per repeat, so
    /// a body wider than one char (a group, a sequence) still uses stack
    /// in proportion to its repeat count (docs/rust-migration.md, rust-04e
    /// "Matcher limits").
    #[allow(clippy::too_many_arguments)]
    fn rep(
        &self,
        t: &[char],
        inner: &Node,
        lo: usize,
        hi: usize,
        count: usize,
        p: usize,
        caps: &Caps,
        k: &mut dyn FnMut(usize, &Caps) -> bool,
    ) -> bool {
        if count < hi
            && self.node(t, inner, p, caps, &mut |e, c| {
                if e == p && count >= lo {
                    return false;
                }
                self.rep(t, inner, lo, hi, count + 1, e, c, k)
            })
        {
            return true;
        }
        count >= lo && k(p, caps)
    }

    /// The first match at or after `pos`; with `must_advance`, not an
    /// empty match at `pos` (sre's rule after an empty match).
    ///
    /// It tries each start in turn, like sre without its prefix scans, so
    /// a pattern that fails late at every start of a long run is quadratic
    /// in the run's length (as it is in CPython for these patterns). The
    /// empty group record is built once per search, and a pattern with no
    /// groups never copies it while matching.
    fn search(&self, t: &[char], pos: usize, must_advance: bool) -> Option<(usize, usize, Caps)> {
        let empty: Caps = vec![None; self.groups + 1];
        for s in pos..=t.len() {
            let mut got = None;
            let ok = self.node(t, &self.root, s, &empty, &mut |e, c| {
                if must_advance && s == pos && e == pos {
                    return false;
                }
                got = Some((
                    e,
                    if c.iter().any(Option::is_some) {
                        Some(c.clone())
                    } else {
                        None
                    },
                ));
                true
            });
            if ok {
                if let Some((e, c)) = got {
                    return Some((s, e, c.unwrap_or_else(|| empty.clone())));
                }
            }
        }
        None
    }

    /// Every match as `re.finditer` steps: (start, end, groups).
    pub fn matches(&self, t: &[char]) -> Vec<(usize, usize, Caps)> {
        let mut out = Vec::new();
        let (mut pos, mut must_advance) = (0, false);
        while pos <= t.len() {
            let Some((s, e, c)) = self.search(t, pos, must_advance) else {
                break;
            };
            must_advance = s == e;
            pos = e;
            out.push((s, e, c));
        }
        out
    }

    /// `bool(re.search(...))`.
    pub fn is_match(&self, s: &str) -> bool {
        let t: Vec<char> = s.chars().collect();
        self.search(&t, 0, false).is_some()
    }

    /// `re.sub` with a replacement built from the match's groups.
    pub fn sub_with(&self, s: &str, repl: &dyn Fn(&[char], &Caps) -> String) -> String {
        let t: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for (start, end, caps) in self.matches(&t) {
            out.extend(&t[last..start]);
            out.push_str(&repl(&t, &caps));
            last = end;
        }
        out.extend(&t[last..]);
        out
    }

    /// `re.sub` with a literal replacement (no backslash in it).
    pub fn sub(&self, s: &str, repl: &str) -> String {
        self.sub_with(s, &|_, _| repl.to_owned())
    }

    /// `re.findall` for a pattern with groups: each match's groups, `""`
    /// for a group that did not take part.
    pub fn findall_groups(&self, s: &str) -> Vec<Vec<String>> {
        let t: Vec<char> = s.chars().collect();
        self.matches(&t)
            .into_iter()
            .map(|(_, _, caps)| (1..=self.groups).map(|i| group(&t, &caps, i)).collect())
            .collect()
    }
}

/// One group's text, `""` when it did not take part.
pub fn group(t: &[char], caps: &Caps, i: usize) -> String {
    caps.get(i)
        .copied()
        .flatten()
        .map(|(s, e)| t[s..e].iter().collect())
        .unwrap_or_default()
}
