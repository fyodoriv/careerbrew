//! rust-27b slice 1: a native port of the repo scan behind
//! `scripts/leak-check.sh` (`careerbrew privacy scan [--staged]`,
//! `privacy.scan_repo` and `cli.cmd_privacy_scan`).
//!
//! Scope of this slice: the scan engine and its output. The terms come from
//! a supplied plaintext file (`--terms-file`), one term per line, read with
//! the rules of `privacy._read_terms`. Reading the owner's encrypted terms,
//! profile and plugin term files belongs to rust-04, rust-05 and rust-07, so
//! the wrapper still runs the Python scan. Staged visual snapshot PNGs are
//! refused as `scan-incomplete` here; their validated byte and pixel scan is
//! a later slice. Output prints term ids, never a term.

use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use unicode_normalization::UnicodeNormalization;

const INDEX_LIMIT: u64 = 32 * 1024 * 1024;
const STAGED_DIFF: &[&str] = &[
    "diff",
    "--cached",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
    "--ignore-submodules=none",
    "--raw",
    "--abbrev=64",
    "--diff-filter=ACMRT",
    "-z",
];

/// Parsed `leak-check` arguments.
pub struct Args {
    pub repo: PathBuf,
    pub terms_file: PathBuf,
    pub staged: bool,
}

/// `[--staged] [--repo <dir>] --terms-file <file>`; None is a usage error.
pub fn parse(rest: &[String], default_repo: &Path) -> Option<Args> {
    let (mut repo, mut terms, mut staged) = (None, None, false);
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--staged" if !staged => staged = true,
            "--repo" if repo.is_none() => repo = Some(PathBuf::from(it.next()?)),
            "--terms-file" if terms.is_none() => terms = Some(PathBuf::from(it.next()?)),
            _ => return None,
        }
    }
    Some(Args {
        repo: repo.unwrap_or_else(|| default_repo.to_path_buf()),
        terms_file: terms?,
        staged,
    })
}

/// Lines of a plaintext terms file, as `privacy._read_terms` keeps them,
/// sorted longest first like `sensitive_terms`.
pub fn read_terms(path: &Path) -> Result<Vec<String>, &'static str> {
    let text = std::fs::read_to_string(path).map_err(|_| "cannot read the terms file")?;
    let mut out: Vec<String> = py_splitlines(&text)
        .into_iter()
        .map(py_trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && l.chars().count() >= 3)
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    out.sort_by_key(|t| std::cmp::Reverse(t.chars().count()));
    Ok(out)
}

/// Python `str.isspace` for one char: Unicode White_Space plus U+001C..U+001F.
fn py_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

/// Python `str.strip()` with no argument.
fn py_trim(s: &str) -> &str {
    s.trim_matches(py_space)
}

/// Python `str.split()` with no argument: the count of words.
fn py_words(s: &str) -> usize {
    s.split(py_space).filter(|w| !w.is_empty()).count()
}

/// Python `str.splitlines()`: every line boundary Python knows, `\r\n` as one.
pub fn py_splitlines(text: &str) -> Vec<&str> {
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

pub fn term_id(term: &str) -> String {
    let digest = Sha256::digest(term.to_lowercase().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("t-{}", &hex[..8])
}

pub fn normalize(text: &str) -> String {
    text.nfkc()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}'))
        .collect()
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn leet(c: char) -> Option<&'static [char]> {
    Some(match fold(c) {
        'o' => &['o', '0'],
        'i' => &['i', '1', 'l', '!'],
        'l' => &['l', '1', 'i'],
        'e' => &['e', '3'],
        'a' => &['a', '4', '@'],
        's' => &['s', '5', '$'],
        't' => &['t', '7'],
        'b' => &['b', '8'],
        'g' => &['g', '9'],
        _ => return None,
    })
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '_' | '-' | '.' | '\u{1c}'..='\u{1f}')
}

/// One compiled `privacy.term_pattern`.
struct Pattern {
    atoms: Vec<char>,
    spaced: bool,
}

impl Pattern {
    fn new(term: &str) -> Self {
        let core = normalize(term);
        if core.chars().count() >= 5 {
            Pattern {
                atoms: core.chars().filter(|c| !c.is_whitespace()).collect(),
                spaced: true,
            }
        } else {
            Pattern {
                atoms: core.chars().collect(),
                spaced: false,
            }
        }
    }

    fn atom(&self, want: char, got: char) -> bool {
        if self.spaced {
            if let Some(class) = leet(want) {
                return class.iter().any(|&c| fold(c) == fold(got));
            }
        }
        fold(want) == fold(got)
    }

    /// The end of a greedy match of atoms[i..] from text[p], with backtracking.
    fn from(&self, text: &[char], i: usize, p: usize) -> Option<usize> {
        if p >= text.len() || !self.atom(self.atoms[i], text[p]) {
            return None;
        }
        let p = p + 1;
        if i + 1 == self.atoms.len() {
            return Some(p);
        }
        let mut most = 0;
        if self.spaced {
            while p + most < text.len() && is_sep(text[p + most]) {
                most += 1;
            }
        }
        (0..=most)
            .rev()
            .find_map(|skip| self.from(text, i + 1, p + skip))
    }

    /// Every leftmost, non-overlapping match as (start, end) char offsets.
    fn find_all(&self, text: &[char]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        if self.atoms.is_empty() {
            return out;
        }
        let mut s = 0;
        while s < text.len() {
            match self.from(text, 0, s) {
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

fn match_kind(got: &str, text: &[char], start: usize, end: usize, term: &str) -> String {
    let core = py_trim(term);
    if got.contains('\n') {
        return format!(
            "letters spread over {} lines, likely a false match",
            got.matches('\n').count() + 1
        );
    }
    let words = py_words(got);
    if words > py_words(core) {
        return format!("letters spread across {words} words, likely a false match");
    }
    if (start > 0 && text[start - 1].is_alphanumeric())
        || (end < text.len() && text[end].is_alphanumeric())
    {
        return "inside a longer word, likely a false match".into();
    }
    if got.to_lowercase() == core.to_lowercase() {
        "whole word".into()
    } else {
        "look-alike spelling".into()
    }
}

/// One span: (line, how, matched text).
pub type Span = (usize, String, String);

/// `privacy.ScanReport`; matched text is a dedupe key only, never printed.
#[derive(Default)]
pub struct Report {
    pub files_scanned: usize,
    pub findings: Vec<(String, String)>,
    /// (path, term id) -> (line, how, matched text); the text is only a
    /// dedupe key like Python's span tuple, and is never printed.
    pub spans: HashMap<(String, String), Vec<Span>>,
}

impl Report {
    fn add_terms(&mut self, path: &str, text: &str, terms: &[String], source: &str) {
        let norm: Vec<char> = normalize(text).chars().collect();
        let mut hits = Vec::new();
        for t in terms {
            let tid = term_id(t);
            for (s, e) in Pattern::new(t).find_all(&norm) {
                let line = norm[..s].iter().filter(|&&c| c == '\n').count() + 1;
                let got: String = norm[s..e].iter().collect();
                let how = match_kind(&got, &norm, s, e, t);
                hits.push((line, tid.clone(), how, got));
            }
        }
        hits.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        for (line, tid, how, got) in hits {
            let key = (path.to_string(), tid.clone());
            if !self.spans.contains_key(&key) {
                self.findings.push(key.clone());
            }
            let span = (line, format!("{source}: {how}"), got);
            let list = self.spans.entry(key).or_default();
            if !list.contains(&span) {
                list.push(span);
            }
        }
    }

    fn private_path(&mut self, rel: &str) -> bool {
        if rel.starts_with("private/") || rel.contains("/private/") {
            let f = (rel.to_string(), "path-under-private".to_string());
            if !self.findings.contains(&f) {
                self.findings.push(f);
            }
            return true;
        }
        false
    }

    /// The lines `cmd_privacy_scan` prints without `--show`.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (path, why) in &self.findings {
            match self.spans.get(&(path.clone(), why.clone())) {
                Some(spans) if !spans.is_empty() => {
                    for (line, how, _) in spans {
                        out.push(format!("LEAK  {path}:{line}: {why} ({how})"));
                    }
                }
                _ => out.push(format!("LEAK  {path}: {why}")),
            }
        }
        out.push(format!(
            "privacy scan: {} file(s), {} finding(s)",
            self.files_scanned,
            self.findings.len()
        ));
        if !self.spans.is_empty() {
            out.push("see each matched text yourself: add --show (prints the term; never paste it into chat or an issue)".into());
        }
        out
    }
}

type Fail = &'static str;

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, Fail> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|_| "could not inspect the Git index")?;
    if !out.status.success() {
        return Err("could not inspect the Git index");
    }
    Ok(out.stdout)
}

type IndexState = (String, Option<(u64, u64, u64, i64, i64, i64, i64)>);

fn index_state(repo: &Path) -> Result<IndexState, Fail> {
    let raw = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?;
    let path = raw
        .strip_suffix(b"\n")
        .filter(|p| !p.contains(&0))
        .and_then(|p| String::from_utf8(p.to_vec()).ok())
        .filter(|p| p.starts_with('/'))
        .ok_or("could not identify the Git index")?;
    let stat = match std::fs::metadata(&path) {
        Ok(m) => Some((
            m.dev(),
            m.ino(),
            m.size(),
            m.mtime(),
            m.mtime_nsec(),
            m.ctime(),
            m.ctime_nsec(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("could not identify the Git index"),
    };
    Ok((path, stat))
}

fn octal(field: &[u8], n: usize) -> bool {
    field.len() == n && field.iter().all(|b| (b'0'..=b'7').contains(b))
}

/// A Git path for display: Python's `os.fsdecode` keeps the bytes, so the
/// scan keeps them too; only the printed form is lossy.
fn display(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

/// Python's non-strict `Path.resolve()`: canonicalize the longest existing
/// ancestor and append the missing rest.
fn resolve_lenient(path: &Path) -> Option<PathBuf> {
    let mut tail = Vec::new();
    let mut cur = path;
    loop {
        if let Ok(real) = cur.canonicalize() {
            let mut out = real;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        tail.push(cur.file_name()?.to_os_string());
        cur = cur.parent()?;
    }
}

fn staged_entries(raw: &[u8]) -> Result<Vec<(Vec<u8>, String, String)>, Fail> {
    const BAD: Fail = "could not parse the Git index";
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<&[u8]> = raw.split(|&b| b == 0).collect();
    if !parts.last().unwrap().is_empty() || parts.len() % 2 != 1 {
        return Err(BAD);
    }
    let mut out = Vec::new();
    for pair in parts[..parts.len() - 1].chunks(2) {
        let (meta, name) = (pair[0], pair[1]);
        let f: Vec<&[u8]> = meta
            .split(|b| b.is_ascii_whitespace())
            .filter(|x| !x.is_empty())
            .collect();
        let oid_ok = |o: &[u8]| {
            (o.len() == 40 || o.len() == 64)
                && o.iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        };
        if f.len() != 5
            || f[0].first() != Some(&b':')
            || !octal(&f[0][1..], 6)
            || !octal(f[1], 6)
            || !oid_ok(f[3])
            || !matches!(f[4], b"A" | b"C" | b"M" | b"T")
            || name.is_empty()
        {
            return Err(BAD);
        }
        if name.starts_with(b"/")
            || name
                .split(|&b| b == b'/')
                .any(|p| p.is_empty() || p == b"." || p == b"..")
        {
            return Err(BAD);
        }
        let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        out.push((name.to_vec(), s(f[1]), s(f[3])));
    }
    Ok(out)
}

fn staged_blob(repo: &Path, mode: &str, oid: &str) -> Result<Vec<u8>, Fail> {
    if !matches!(mode, "100644" | "100755" | "120000")
        || git(repo, &["cat-file", "-t", oid])?.trim_ascii() != b"blob"
    {
        return Err("staged entry is not a readable blob");
    }
    let size: u64 = std::str::from_utf8(git(repo, &["cat-file", "-s", oid])?.trim_ascii())
        .ok()
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|s| s.parse().ok())
        .filter(|&n| n <= INDEX_LIMIT)
        .ok_or("staged blob exceeds the scan limit or has an invalid size")?;
    let data = git(repo, &["cat-file", "blob", oid])?;
    if data.len() as u64 != size {
        return Err("staged blob changed or could not be read");
    }
    Ok(data)
}

/// `privacy.scan_repo` over supplied terms.
pub fn scan_repo(repo: &Path, staged_only: bool, terms: &[String]) -> Report {
    let mut report = Report::default();
    let mut scanned = BTreeSet::new();
    let result = scan_into(repo, staged_only, terms, &mut report, &mut scanned);
    if result.is_err() {
        report
            .findings
            .push(("Git index or working tree".into(), "scan-incomplete".into()));
    }
    report.files_scanned = scanned.len();
    report
}

fn scan_into(
    repo: &Path,
    staged_only: bool,
    terms: &[String],
    report: &mut Report,
    scanned: &mut BTreeSet<Vec<u8>>,
) -> Result<(), Fail> {
    let state = index_state(repo)?;
    let staged_raw = git(repo, STAGED_DIFF)?;
    if !git(repo, &["ls-files", "--unmerged", "-z"])?.is_empty() {
        return Err("unmerged staged entries cannot be scanned");
    }
    for (name, mode, oid) in staged_entries(&staged_raw)? {
        let rel = display(&name);
        if report.private_path(&rel) {
            continue;
        }
        let data = staged_blob(repo, &mode, &oid)?;
        if rel.starts_with("web/visual/__screenshots__/") && rel.ends_with(".png") {
            return Err("staged visual snapshots are not ported yet");
        }
        let text = String::from_utf8(data).map_err(|_| "staged blob is not UTF-8")?;
        scanned.insert(name);
        report.add_terms(&rel, &text, terms, "staged");
    }
    if !staged_only {
        let raw = git(repo, &["ls-files", "-z"])?;
        if !raw.is_empty() && !raw.ends_with(&[0]) {
            return Err("could not parse tracked files");
        }
        let mut names: Vec<&[u8]> = raw.split(|&b| b == 0).filter(|n| !n.is_empty()).collect();
        names.sort_by_key(|n| (display(n), n.to_vec()));
        let root = repo.canonicalize().map_err(|_| "repository path")?;
        for name in names {
            let rel = display(name);
            if report.private_path(&rel) {
                continue;
            }
            let path = repo.join(OsStr::from_bytes(name));
            let parent = path
                .parent()
                .and_then(resolve_lenient)
                .ok_or("tracked working path leaves the repository")?;
            if !parent.starts_with(&root) {
                return Err("tracked working path leaves the repository");
            }
            let meta = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let text = if meta.file_type().is_symlink() {
                std::fs::read_link(&path)
                    .map_err(|_| "cannot read a link")?
                    .to_string_lossy()
                    .into_owned()
            } else if meta.is_file() {
                match std::fs::read(&path) {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(t) => t,
                        Err(_) => continue,
                    },
                    Err(_) => return Err("cannot read a tracked file"),
                }
            } else {
                continue;
            };
            scanned.insert(name.to_vec());
            report.add_terms(&rel, &text, terms, "working");
        }
    }
    if git(repo, STAGED_DIFF)? != staged_raw
        || !git(repo, &["ls-files", "--unmerged", "-z"])?.is_empty()
        || index_state(repo)? != state
    {
        return Err("the Git index changed during the scan");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(term: &str, text: &str) -> usize {
        let chars: Vec<char> = normalize(text).chars().collect();
        Pattern::new(term).find_all(&chars).len()
    }

    #[test]
    fn spaced_leet_and_case_variants_match() {
        assert_eq!(hits("zqxcanary", "a Z q-X_c4n.ary b"), 1);
        assert_eq!(hits("zqxcanary", "zqx canary"), 1);
        assert_eq!(hits("zqxcanary", "zqxcanery"), 0);
        assert_eq!(hits("zqxcanary", "ｚｑｘcanary and zq\u{200b}xcanary"), 2);
    }

    #[test]
    fn short_terms_match_only_literally() {
        assert_eq!(hits("qzx", "QZX"), 1);
        assert_eq!(hits("qzx", "q-z-x"), 0);
    }

    #[test]
    fn splitlines_matches_python() {
        let text = "a\rb\r\nc\u{0b}d\u{0c}e\u{1c}f\u{1d}g\u{1e}h\u{85}i\u{2028}j\u{2029}k\n";
        assert_eq!(
            py_splitlines(text),
            ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]
        );
        assert_eq!(py_splitlines("x\n\ny"), ["x", "", "y"]);
    }

    #[test]
    fn word_count_and_strip_use_python_whitespace() {
        assert_eq!(py_words("ab\u{1f}cd"), 2);
        assert_eq!(py_trim("\u{1c}abc\u{1f}"), "abc");
    }

    #[test]
    fn dedupe_keeps_spans_whose_matched_text_differs() {
        let mut r = Report::default();
        let terms = vec!["zqxcanary".to_string()];
        r.add_terms("a", "zqxcanary zqxcanary zqxc4nary", &terms, "working");
        let spans = r.spans.values().next().unwrap();
        // Equal (line, how, text) spans collapse; a differing text stays.
        assert_eq!(spans.len(), 2);
        let mut r2 = Report::default();
        r2.add_terms("a", "zq-xcanary zqx-canary", &terms, "working");
        assert_eq!(r2.spans.values().next().unwrap().len(), 2);
        assert_eq!(
            r2.lines().iter().filter(|l| l.starts_with("LEAK")).count(),
            2
        );
    }

    #[test]
    fn resolve_lenient_appends_the_missing_tail() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().canonicalize().unwrap();
        assert_eq!(
            resolve_lenient(&d.path().join("gone/deeper")).unwrap(),
            real.join("gone/deeper")
        );
    }

    #[test]
    fn term_id_matches_the_python_formula() {
        // sha256("abc") starts with ba7816bf.
        assert_eq!(term_id("ABC"), "t-ba7816bf");
    }
}
