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
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
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

/// `re._casefix._EXTRA_CASES` (CPython 3.12): characters that Python's
/// `re.IGNORECASE` treats as equal beyond their simple lowercase.
const EXTRA_CASES: &[&[u32]] = &[
    &[0x69, 0x131],
    &[0x73, 0x17f],
    &[0xb5, 0x3bc],
    &[0x345, 0x3b9, 0x1fbe],
    &[0x390, 0x1fd3],
    &[0x3b0, 0x1fe3],
    &[0x3b2, 0x3d0],
    &[0x3b5, 0x3f5],
    &[0x3b8, 0x3d1],
    &[0x3ba, 0x3f0],
    &[0x3c0, 0x3d6],
    &[0x3c1, 0x3f1],
    &[0x3c2, 0x3c3],
    &[0x3c6, 0x3d5],
    &[0x432, 0x1c80],
    &[0x434, 0x1c81],
    &[0x43e, 0x1c82],
    &[0x441, 0x1c83],
    &[0x442, 0x1c84, 0x1c85],
    &[0x44a, 0x1c86],
    &[0x463, 0x1c87],
    &[0x1c88, 0xa64b],
    &[0x1e61, 0x1e9b],
    &[0xfb05, 0xfb06],
];

/// The `re.IGNORECASE` class of a char: its simple lowercase, mapped to the
/// smallest member of its `EXTRA_CASES` group.
fn fold(c: char) -> char {
    let lo = c.to_lowercase().next().unwrap_or(c);
    EXTRA_CASES
        .iter()
        .find(|group| group.contains(&(lo as u32)))
        .and_then(|group| char::from_u32(group[0]))
        .unwrap_or(lo)
}

/// Python `str.isalnum` for one char (Unicode 15.0 data of CPython 3.12).
fn py_isalnum(c: char) -> bool {
    let n = c as u32;
    PY_ALNUM
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

/// `path.read_text` universal newlines: `\r\n` and a lone `\r` become `\n`.
fn universal_newlines(text: String) -> String {
    if !text.contains('\r') {
        return text;
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
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
    if (start > 0 && py_isalnum(text[start - 1])) || (end < text.len() && py_isalnum(text[end])) {
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

/// `privacy._git`'s `timeout=30`.
const GIT_TIMEOUT: Duration = Duration::from_secs(30);

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, Fail> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_OPTIONAL_LOCKS", "0");
    run_bounded(cmd, GIT_TIMEOUT)
}

/// Run a command, keep its stdout, and kill it after `limit` like
/// `subprocess.run(timeout=...)`. Any failure is the same content-free error.
fn run_bounded(mut cmd: Command, limit: Duration) -> Result<Vec<u8>, Fail> {
    const FAIL: Fail = "could not inspect the Git index";
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| FAIL)?;
    let mut out = child.stdout.take().ok_or(FAIL)?;
    let mut err = child.stderr.take().ok_or(FAIL)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        out.read_to_end(&mut buf).map(|_| buf)
    });
    let drain = std::thread::spawn(move || std::io::copy(&mut err, &mut std::io::sink()));
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FAIL);
            }
        }
    };
    let stdout = reader.join().map_err(|_| FAIL)?.map_err(|_| FAIL)?;
    let _ = drain.join();
    if !status.success() {
        return Err(FAIL);
    }
    Ok(stdout)
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
                        Ok(t) => universal_newlines(t),
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

/// Inclusive code point ranges where CPython 3.12 `str.isalnum` is true
/// (generated from `chr(c).isalnum()`, Unicode 15.0).
#[rustfmt::skip]
const PY_ALNUM: &[(u32, u32)] = &[
    (0x30, 0x39),
    (0x41, 0x5a),
    (0x61, 0x7a),
    (0xaa, 0xaa),
    (0xb2, 0xb3),
    (0xb5, 0xb5),
    (0xb9, 0xba),
    (0xbc, 0xbe),
    (0xc0, 0xd6),
    (0xd8, 0xf6),
    (0xf8, 0x2c1),
    (0x2c6, 0x2d1),
    (0x2e0, 0x2e4),
    (0x2ec, 0x2ec),
    (0x2ee, 0x2ee),
    (0x370, 0x374),
    (0x376, 0x377),
    (0x37a, 0x37d),
    (0x37f, 0x37f),
    (0x386, 0x386),
    (0x388, 0x38a),
    (0x38c, 0x38c),
    (0x38e, 0x3a1),
    (0x3a3, 0x3f5),
    (0x3f7, 0x481),
    (0x48a, 0x52f),
    (0x531, 0x556),
    (0x559, 0x559),
    (0x560, 0x588),
    (0x5d0, 0x5ea),
    (0x5ef, 0x5f2),
    (0x620, 0x64a),
    (0x660, 0x669),
    (0x66e, 0x66f),
    (0x671, 0x6d3),
    (0x6d5, 0x6d5),
    (0x6e5, 0x6e6),
    (0x6ee, 0x6fc),
    (0x6ff, 0x6ff),
    (0x710, 0x710),
    (0x712, 0x72f),
    (0x74d, 0x7a5),
    (0x7b1, 0x7b1),
    (0x7c0, 0x7ea),
    (0x7f4, 0x7f5),
    (0x7fa, 0x7fa),
    (0x800, 0x815),
    (0x81a, 0x81a),
    (0x824, 0x824),
    (0x828, 0x828),
    (0x840, 0x858),
    (0x860, 0x86a),
    (0x870, 0x887),
    (0x889, 0x88e),
    (0x8a0, 0x8c9),
    (0x904, 0x939),
    (0x93d, 0x93d),
    (0x950, 0x950),
    (0x958, 0x961),
    (0x966, 0x96f),
    (0x971, 0x980),
    (0x985, 0x98c),
    (0x98f, 0x990),
    (0x993, 0x9a8),
    (0x9aa, 0x9b0),
    (0x9b2, 0x9b2),
    (0x9b6, 0x9b9),
    (0x9bd, 0x9bd),
    (0x9ce, 0x9ce),
    (0x9dc, 0x9dd),
    (0x9df, 0x9e1),
    (0x9e6, 0x9f1),
    (0x9f4, 0x9f9),
    (0x9fc, 0x9fc),
    (0xa05, 0xa0a),
    (0xa0f, 0xa10),
    (0xa13, 0xa28),
    (0xa2a, 0xa30),
    (0xa32, 0xa33),
    (0xa35, 0xa36),
    (0xa38, 0xa39),
    (0xa59, 0xa5c),
    (0xa5e, 0xa5e),
    (0xa66, 0xa6f),
    (0xa72, 0xa74),
    (0xa85, 0xa8d),
    (0xa8f, 0xa91),
    (0xa93, 0xaa8),
    (0xaaa, 0xab0),
    (0xab2, 0xab3),
    (0xab5, 0xab9),
    (0xabd, 0xabd),
    (0xad0, 0xad0),
    (0xae0, 0xae1),
    (0xae6, 0xaef),
    (0xaf9, 0xaf9),
    (0xb05, 0xb0c),
    (0xb0f, 0xb10),
    (0xb13, 0xb28),
    (0xb2a, 0xb30),
    (0xb32, 0xb33),
    (0xb35, 0xb39),
    (0xb3d, 0xb3d),
    (0xb5c, 0xb5d),
    (0xb5f, 0xb61),
    (0xb66, 0xb6f),
    (0xb71, 0xb77),
    (0xb83, 0xb83),
    (0xb85, 0xb8a),
    (0xb8e, 0xb90),
    (0xb92, 0xb95),
    (0xb99, 0xb9a),
    (0xb9c, 0xb9c),
    (0xb9e, 0xb9f),
    (0xba3, 0xba4),
    (0xba8, 0xbaa),
    (0xbae, 0xbb9),
    (0xbd0, 0xbd0),
    (0xbe6, 0xbf2),
    (0xc05, 0xc0c),
    (0xc0e, 0xc10),
    (0xc12, 0xc28),
    (0xc2a, 0xc39),
    (0xc3d, 0xc3d),
    (0xc58, 0xc5a),
    (0xc5d, 0xc5d),
    (0xc60, 0xc61),
    (0xc66, 0xc6f),
    (0xc78, 0xc7e),
    (0xc80, 0xc80),
    (0xc85, 0xc8c),
    (0xc8e, 0xc90),
    (0xc92, 0xca8),
    (0xcaa, 0xcb3),
    (0xcb5, 0xcb9),
    (0xcbd, 0xcbd),
    (0xcdd, 0xcde),
    (0xce0, 0xce1),
    (0xce6, 0xcef),
    (0xcf1, 0xcf2),
    (0xd04, 0xd0c),
    (0xd0e, 0xd10),
    (0xd12, 0xd3a),
    (0xd3d, 0xd3d),
    (0xd4e, 0xd4e),
    (0xd54, 0xd56),
    (0xd58, 0xd61),
    (0xd66, 0xd78),
    (0xd7a, 0xd7f),
    (0xd85, 0xd96),
    (0xd9a, 0xdb1),
    (0xdb3, 0xdbb),
    (0xdbd, 0xdbd),
    (0xdc0, 0xdc6),
    (0xde6, 0xdef),
    (0xe01, 0xe30),
    (0xe32, 0xe33),
    (0xe40, 0xe46),
    (0xe50, 0xe59),
    (0xe81, 0xe82),
    (0xe84, 0xe84),
    (0xe86, 0xe8a),
    (0xe8c, 0xea3),
    (0xea5, 0xea5),
    (0xea7, 0xeb0),
    (0xeb2, 0xeb3),
    (0xebd, 0xebd),
    (0xec0, 0xec4),
    (0xec6, 0xec6),
    (0xed0, 0xed9),
    (0xedc, 0xedf),
    (0xf00, 0xf00),
    (0xf20, 0xf33),
    (0xf40, 0xf47),
    (0xf49, 0xf6c),
    (0xf88, 0xf8c),
    (0x1000, 0x102a),
    (0x103f, 0x1049),
    (0x1050, 0x1055),
    (0x105a, 0x105d),
    (0x1061, 0x1061),
    (0x1065, 0x1066),
    (0x106e, 0x1070),
    (0x1075, 0x1081),
    (0x108e, 0x108e),
    (0x1090, 0x1099),
    (0x10a0, 0x10c5),
    (0x10c7, 0x10c7),
    (0x10cd, 0x10cd),
    (0x10d0, 0x10fa),
    (0x10fc, 0x1248),
    (0x124a, 0x124d),
    (0x1250, 0x1256),
    (0x1258, 0x1258),
    (0x125a, 0x125d),
    (0x1260, 0x1288),
    (0x128a, 0x128d),
    (0x1290, 0x12b0),
    (0x12b2, 0x12b5),
    (0x12b8, 0x12be),
    (0x12c0, 0x12c0),
    (0x12c2, 0x12c5),
    (0x12c8, 0x12d6),
    (0x12d8, 0x1310),
    (0x1312, 0x1315),
    (0x1318, 0x135a),
    (0x1369, 0x137c),
    (0x1380, 0x138f),
    (0x13a0, 0x13f5),
    (0x13f8, 0x13fd),
    (0x1401, 0x166c),
    (0x166f, 0x167f),
    (0x1681, 0x169a),
    (0x16a0, 0x16ea),
    (0x16ee, 0x16f8),
    (0x1700, 0x1711),
    (0x171f, 0x1731),
    (0x1740, 0x1751),
    (0x1760, 0x176c),
    (0x176e, 0x1770),
    (0x1780, 0x17b3),
    (0x17d7, 0x17d7),
    (0x17dc, 0x17dc),
    (0x17e0, 0x17e9),
    (0x17f0, 0x17f9),
    (0x1810, 0x1819),
    (0x1820, 0x1878),
    (0x1880, 0x1884),
    (0x1887, 0x18a8),
    (0x18aa, 0x18aa),
    (0x18b0, 0x18f5),
    (0x1900, 0x191e),
    (0x1946, 0x196d),
    (0x1970, 0x1974),
    (0x1980, 0x19ab),
    (0x19b0, 0x19c9),
    (0x19d0, 0x19da),
    (0x1a00, 0x1a16),
    (0x1a20, 0x1a54),
    (0x1a80, 0x1a89),
    (0x1a90, 0x1a99),
    (0x1aa7, 0x1aa7),
    (0x1b05, 0x1b33),
    (0x1b45, 0x1b4c),
    (0x1b50, 0x1b59),
    (0x1b83, 0x1ba0),
    (0x1bae, 0x1be5),
    (0x1c00, 0x1c23),
    (0x1c40, 0x1c49),
    (0x1c4d, 0x1c7d),
    (0x1c80, 0x1c88),
    (0x1c90, 0x1cba),
    (0x1cbd, 0x1cbf),
    (0x1ce9, 0x1cec),
    (0x1cee, 0x1cf3),
    (0x1cf5, 0x1cf6),
    (0x1cfa, 0x1cfa),
    (0x1d00, 0x1dbf),
    (0x1e00, 0x1f15),
    (0x1f18, 0x1f1d),
    (0x1f20, 0x1f45),
    (0x1f48, 0x1f4d),
    (0x1f50, 0x1f57),
    (0x1f59, 0x1f59),
    (0x1f5b, 0x1f5b),
    (0x1f5d, 0x1f5d),
    (0x1f5f, 0x1f7d),
    (0x1f80, 0x1fb4),
    (0x1fb6, 0x1fbc),
    (0x1fbe, 0x1fbe),
    (0x1fc2, 0x1fc4),
    (0x1fc6, 0x1fcc),
    (0x1fd0, 0x1fd3),
    (0x1fd6, 0x1fdb),
    (0x1fe0, 0x1fec),
    (0x1ff2, 0x1ff4),
    (0x1ff6, 0x1ffc),
    (0x2070, 0x2071),
    (0x2074, 0x2079),
    (0x207f, 0x2089),
    (0x2090, 0x209c),
    (0x2102, 0x2102),
    (0x2107, 0x2107),
    (0x210a, 0x2113),
    (0x2115, 0x2115),
    (0x2119, 0x211d),
    (0x2124, 0x2124),
    (0x2126, 0x2126),
    (0x2128, 0x2128),
    (0x212a, 0x212d),
    (0x212f, 0x2139),
    (0x213c, 0x213f),
    (0x2145, 0x2149),
    (0x214e, 0x214e),
    (0x2150, 0x2189),
    (0x2460, 0x249b),
    (0x24ea, 0x24ff),
    (0x2776, 0x2793),
    (0x2c00, 0x2ce4),
    (0x2ceb, 0x2cee),
    (0x2cf2, 0x2cf3),
    (0x2cfd, 0x2cfd),
    (0x2d00, 0x2d25),
    (0x2d27, 0x2d27),
    (0x2d2d, 0x2d2d),
    (0x2d30, 0x2d67),
    (0x2d6f, 0x2d6f),
    (0x2d80, 0x2d96),
    (0x2da0, 0x2da6),
    (0x2da8, 0x2dae),
    (0x2db0, 0x2db6),
    (0x2db8, 0x2dbe),
    (0x2dc0, 0x2dc6),
    (0x2dc8, 0x2dce),
    (0x2dd0, 0x2dd6),
    (0x2dd8, 0x2dde),
    (0x2e2f, 0x2e2f),
    (0x3005, 0x3007),
    (0x3021, 0x3029),
    (0x3031, 0x3035),
    (0x3038, 0x303c),
    (0x3041, 0x3096),
    (0x309d, 0x309f),
    (0x30a1, 0x30fa),
    (0x30fc, 0x30ff),
    (0x3105, 0x312f),
    (0x3131, 0x318e),
    (0x3192, 0x3195),
    (0x31a0, 0x31bf),
    (0x31f0, 0x31ff),
    (0x3220, 0x3229),
    (0x3248, 0x324f),
    (0x3251, 0x325f),
    (0x3280, 0x3289),
    (0x32b1, 0x32bf),
    (0x3400, 0x4dbf),
    (0x4e00, 0xa48c),
    (0xa4d0, 0xa4fd),
    (0xa500, 0xa60c),
    (0xa610, 0xa62b),
    (0xa640, 0xa66e),
    (0xa67f, 0xa69d),
    (0xa6a0, 0xa6ef),
    (0xa717, 0xa71f),
    (0xa722, 0xa788),
    (0xa78b, 0xa7ca),
    (0xa7d0, 0xa7d1),
    (0xa7d3, 0xa7d3),
    (0xa7d5, 0xa7d9),
    (0xa7f2, 0xa801),
    (0xa803, 0xa805),
    (0xa807, 0xa80a),
    (0xa80c, 0xa822),
    (0xa830, 0xa835),
    (0xa840, 0xa873),
    (0xa882, 0xa8b3),
    (0xa8d0, 0xa8d9),
    (0xa8f2, 0xa8f7),
    (0xa8fb, 0xa8fb),
    (0xa8fd, 0xa8fe),
    (0xa900, 0xa925),
    (0xa930, 0xa946),
    (0xa960, 0xa97c),
    (0xa984, 0xa9b2),
    (0xa9cf, 0xa9d9),
    (0xa9e0, 0xa9e4),
    (0xa9e6, 0xa9fe),
    (0xaa00, 0xaa28),
    (0xaa40, 0xaa42),
    (0xaa44, 0xaa4b),
    (0xaa50, 0xaa59),
    (0xaa60, 0xaa76),
    (0xaa7a, 0xaa7a),
    (0xaa7e, 0xaaaf),
    (0xaab1, 0xaab1),
    (0xaab5, 0xaab6),
    (0xaab9, 0xaabd),
    (0xaac0, 0xaac0),
    (0xaac2, 0xaac2),
    (0xaadb, 0xaadd),
    (0xaae0, 0xaaea),
    (0xaaf2, 0xaaf4),
    (0xab01, 0xab06),
    (0xab09, 0xab0e),
    (0xab11, 0xab16),
    (0xab20, 0xab26),
    (0xab28, 0xab2e),
    (0xab30, 0xab5a),
    (0xab5c, 0xab69),
    (0xab70, 0xabe2),
    (0xabf0, 0xabf9),
    (0xac00, 0xd7a3),
    (0xd7b0, 0xd7c6),
    (0xd7cb, 0xd7fb),
    (0xf900, 0xfa6d),
    (0xfa70, 0xfad9),
    (0xfb00, 0xfb06),
    (0xfb13, 0xfb17),
    (0xfb1d, 0xfb1d),
    (0xfb1f, 0xfb28),
    (0xfb2a, 0xfb36),
    (0xfb38, 0xfb3c),
    (0xfb3e, 0xfb3e),
    (0xfb40, 0xfb41),
    (0xfb43, 0xfb44),
    (0xfb46, 0xfbb1),
    (0xfbd3, 0xfd3d),
    (0xfd50, 0xfd8f),
    (0xfd92, 0xfdc7),
    (0xfdf0, 0xfdfb),
    (0xfe70, 0xfe74),
    (0xfe76, 0xfefc),
    (0xff10, 0xff19),
    (0xff21, 0xff3a),
    (0xff41, 0xff5a),
    (0xff66, 0xffbe),
    (0xffc2, 0xffc7),
    (0xffca, 0xffcf),
    (0xffd2, 0xffd7),
    (0xffda, 0xffdc),
    (0x10000, 0x1000b),
    (0x1000d, 0x10026),
    (0x10028, 0x1003a),
    (0x1003c, 0x1003d),
    (0x1003f, 0x1004d),
    (0x10050, 0x1005d),
    (0x10080, 0x100fa),
    (0x10107, 0x10133),
    (0x10140, 0x10178),
    (0x1018a, 0x1018b),
    (0x10280, 0x1029c),
    (0x102a0, 0x102d0),
    (0x102e1, 0x102fb),
    (0x10300, 0x10323),
    (0x1032d, 0x1034a),
    (0x10350, 0x10375),
    (0x10380, 0x1039d),
    (0x103a0, 0x103c3),
    (0x103c8, 0x103cf),
    (0x103d1, 0x103d5),
    (0x10400, 0x1049d),
    (0x104a0, 0x104a9),
    (0x104b0, 0x104d3),
    (0x104d8, 0x104fb),
    (0x10500, 0x10527),
    (0x10530, 0x10563),
    (0x10570, 0x1057a),
    (0x1057c, 0x1058a),
    (0x1058c, 0x10592),
    (0x10594, 0x10595),
    (0x10597, 0x105a1),
    (0x105a3, 0x105b1),
    (0x105b3, 0x105b9),
    (0x105bb, 0x105bc),
    (0x10600, 0x10736),
    (0x10740, 0x10755),
    (0x10760, 0x10767),
    (0x10780, 0x10785),
    (0x10787, 0x107b0),
    (0x107b2, 0x107ba),
    (0x10800, 0x10805),
    (0x10808, 0x10808),
    (0x1080a, 0x10835),
    (0x10837, 0x10838),
    (0x1083c, 0x1083c),
    (0x1083f, 0x10855),
    (0x10858, 0x10876),
    (0x10879, 0x1089e),
    (0x108a7, 0x108af),
    (0x108e0, 0x108f2),
    (0x108f4, 0x108f5),
    (0x108fb, 0x1091b),
    (0x10920, 0x10939),
    (0x10980, 0x109b7),
    (0x109bc, 0x109cf),
    (0x109d2, 0x10a00),
    (0x10a10, 0x10a13),
    (0x10a15, 0x10a17),
    (0x10a19, 0x10a35),
    (0x10a40, 0x10a48),
    (0x10a60, 0x10a7e),
    (0x10a80, 0x10a9f),
    (0x10ac0, 0x10ac7),
    (0x10ac9, 0x10ae4),
    (0x10aeb, 0x10aef),
    (0x10b00, 0x10b35),
    (0x10b40, 0x10b55),
    (0x10b58, 0x10b72),
    (0x10b78, 0x10b91),
    (0x10ba9, 0x10baf),
    (0x10c00, 0x10c48),
    (0x10c80, 0x10cb2),
    (0x10cc0, 0x10cf2),
    (0x10cfa, 0x10d23),
    (0x10d30, 0x10d39),
    (0x10e60, 0x10e7e),
    (0x10e80, 0x10ea9),
    (0x10eb0, 0x10eb1),
    (0x10f00, 0x10f27),
    (0x10f30, 0x10f45),
    (0x10f51, 0x10f54),
    (0x10f70, 0x10f81),
    (0x10fb0, 0x10fcb),
    (0x10fe0, 0x10ff6),
    (0x11003, 0x11037),
    (0x11052, 0x1106f),
    (0x11071, 0x11072),
    (0x11075, 0x11075),
    (0x11083, 0x110af),
    (0x110d0, 0x110e8),
    (0x110f0, 0x110f9),
    (0x11103, 0x11126),
    (0x11136, 0x1113f),
    (0x11144, 0x11144),
    (0x11147, 0x11147),
    (0x11150, 0x11172),
    (0x11176, 0x11176),
    (0x11183, 0x111b2),
    (0x111c1, 0x111c4),
    (0x111d0, 0x111da),
    (0x111dc, 0x111dc),
    (0x111e1, 0x111f4),
    (0x11200, 0x11211),
    (0x11213, 0x1122b),
    (0x1123f, 0x11240),
    (0x11280, 0x11286),
    (0x11288, 0x11288),
    (0x1128a, 0x1128d),
    (0x1128f, 0x1129d),
    (0x1129f, 0x112a8),
    (0x112b0, 0x112de),
    (0x112f0, 0x112f9),
    (0x11305, 0x1130c),
    (0x1130f, 0x11310),
    (0x11313, 0x11328),
    (0x1132a, 0x11330),
    (0x11332, 0x11333),
    (0x11335, 0x11339),
    (0x1133d, 0x1133d),
    (0x11350, 0x11350),
    (0x1135d, 0x11361),
    (0x11400, 0x11434),
    (0x11447, 0x1144a),
    (0x11450, 0x11459),
    (0x1145f, 0x11461),
    (0x11480, 0x114af),
    (0x114c4, 0x114c5),
    (0x114c7, 0x114c7),
    (0x114d0, 0x114d9),
    (0x11580, 0x115ae),
    (0x115d8, 0x115db),
    (0x11600, 0x1162f),
    (0x11644, 0x11644),
    (0x11650, 0x11659),
    (0x11680, 0x116aa),
    (0x116b8, 0x116b8),
    (0x116c0, 0x116c9),
    (0x11700, 0x1171a),
    (0x11730, 0x1173b),
    (0x11740, 0x11746),
    (0x11800, 0x1182b),
    (0x118a0, 0x118f2),
    (0x118ff, 0x11906),
    (0x11909, 0x11909),
    (0x1190c, 0x11913),
    (0x11915, 0x11916),
    (0x11918, 0x1192f),
    (0x1193f, 0x1193f),
    (0x11941, 0x11941),
    (0x11950, 0x11959),
    (0x119a0, 0x119a7),
    (0x119aa, 0x119d0),
    (0x119e1, 0x119e1),
    (0x119e3, 0x119e3),
    (0x11a00, 0x11a00),
    (0x11a0b, 0x11a32),
    (0x11a3a, 0x11a3a),
    (0x11a50, 0x11a50),
    (0x11a5c, 0x11a89),
    (0x11a9d, 0x11a9d),
    (0x11ab0, 0x11af8),
    (0x11c00, 0x11c08),
    (0x11c0a, 0x11c2e),
    (0x11c40, 0x11c40),
    (0x11c50, 0x11c6c),
    (0x11c72, 0x11c8f),
    (0x11d00, 0x11d06),
    (0x11d08, 0x11d09),
    (0x11d0b, 0x11d30),
    (0x11d46, 0x11d46),
    (0x11d50, 0x11d59),
    (0x11d60, 0x11d65),
    (0x11d67, 0x11d68),
    (0x11d6a, 0x11d89),
    (0x11d98, 0x11d98),
    (0x11da0, 0x11da9),
    (0x11ee0, 0x11ef2),
    (0x11f02, 0x11f02),
    (0x11f04, 0x11f10),
    (0x11f12, 0x11f33),
    (0x11f50, 0x11f59),
    (0x11fb0, 0x11fb0),
    (0x11fc0, 0x11fd4),
    (0x12000, 0x12399),
    (0x12400, 0x1246e),
    (0x12480, 0x12543),
    (0x12f90, 0x12ff0),
    (0x13000, 0x1342f),
    (0x13441, 0x13446),
    (0x14400, 0x14646),
    (0x16800, 0x16a38),
    (0x16a40, 0x16a5e),
    (0x16a60, 0x16a69),
    (0x16a70, 0x16abe),
    (0x16ac0, 0x16ac9),
    (0x16ad0, 0x16aed),
    (0x16b00, 0x16b2f),
    (0x16b40, 0x16b43),
    (0x16b50, 0x16b59),
    (0x16b5b, 0x16b61),
    (0x16b63, 0x16b77),
    (0x16b7d, 0x16b8f),
    (0x16e40, 0x16e96),
    (0x16f00, 0x16f4a),
    (0x16f50, 0x16f50),
    (0x16f93, 0x16f9f),
    (0x16fe0, 0x16fe1),
    (0x16fe3, 0x16fe3),
    (0x17000, 0x187f7),
    (0x18800, 0x18cd5),
    (0x18d00, 0x18d08),
    (0x1aff0, 0x1aff3),
    (0x1aff5, 0x1affb),
    (0x1affd, 0x1affe),
    (0x1b000, 0x1b122),
    (0x1b132, 0x1b132),
    (0x1b150, 0x1b152),
    (0x1b155, 0x1b155),
    (0x1b164, 0x1b167),
    (0x1b170, 0x1b2fb),
    (0x1bc00, 0x1bc6a),
    (0x1bc70, 0x1bc7c),
    (0x1bc80, 0x1bc88),
    (0x1bc90, 0x1bc99),
    (0x1d2c0, 0x1d2d3),
    (0x1d2e0, 0x1d2f3),
    (0x1d360, 0x1d378),
    (0x1d400, 0x1d454),
    (0x1d456, 0x1d49c),
    (0x1d49e, 0x1d49f),
    (0x1d4a2, 0x1d4a2),
    (0x1d4a5, 0x1d4a6),
    (0x1d4a9, 0x1d4ac),
    (0x1d4ae, 0x1d4b9),
    (0x1d4bb, 0x1d4bb),
    (0x1d4bd, 0x1d4c3),
    (0x1d4c5, 0x1d505),
    (0x1d507, 0x1d50a),
    (0x1d50d, 0x1d514),
    (0x1d516, 0x1d51c),
    (0x1d51e, 0x1d539),
    (0x1d53b, 0x1d53e),
    (0x1d540, 0x1d544),
    (0x1d546, 0x1d546),
    (0x1d54a, 0x1d550),
    (0x1d552, 0x1d6a5),
    (0x1d6a8, 0x1d6c0),
    (0x1d6c2, 0x1d6da),
    (0x1d6dc, 0x1d6fa),
    (0x1d6fc, 0x1d714),
    (0x1d716, 0x1d734),
    (0x1d736, 0x1d74e),
    (0x1d750, 0x1d76e),
    (0x1d770, 0x1d788),
    (0x1d78a, 0x1d7a8),
    (0x1d7aa, 0x1d7c2),
    (0x1d7c4, 0x1d7cb),
    (0x1d7ce, 0x1d7ff),
    (0x1df00, 0x1df1e),
    (0x1df25, 0x1df2a),
    (0x1e030, 0x1e06d),
    (0x1e100, 0x1e12c),
    (0x1e137, 0x1e13d),
    (0x1e140, 0x1e149),
    (0x1e14e, 0x1e14e),
    (0x1e290, 0x1e2ad),
    (0x1e2c0, 0x1e2eb),
    (0x1e2f0, 0x1e2f9),
    (0x1e4d0, 0x1e4eb),
    (0x1e4f0, 0x1e4f9),
    (0x1e7e0, 0x1e7e6),
    (0x1e7e8, 0x1e7eb),
    (0x1e7ed, 0x1e7ee),
    (0x1e7f0, 0x1e7fe),
    (0x1e800, 0x1e8c4),
    (0x1e8c7, 0x1e8cf),
    (0x1e900, 0x1e943),
    (0x1e94b, 0x1e94b),
    (0x1e950, 0x1e959),
    (0x1ec71, 0x1ecab),
    (0x1ecad, 0x1ecaf),
    (0x1ecb1, 0x1ecb4),
    (0x1ed01, 0x1ed2d),
    (0x1ed2f, 0x1ed3d),
    (0x1ee00, 0x1ee03),
    (0x1ee05, 0x1ee1f),
    (0x1ee21, 0x1ee22),
    (0x1ee24, 0x1ee24),
    (0x1ee27, 0x1ee27),
    (0x1ee29, 0x1ee32),
    (0x1ee34, 0x1ee37),
    (0x1ee39, 0x1ee39),
    (0x1ee3b, 0x1ee3b),
    (0x1ee42, 0x1ee42),
    (0x1ee47, 0x1ee47),
    (0x1ee49, 0x1ee49),
    (0x1ee4b, 0x1ee4b),
    (0x1ee4d, 0x1ee4f),
    (0x1ee51, 0x1ee52),
    (0x1ee54, 0x1ee54),
    (0x1ee57, 0x1ee57),
    (0x1ee59, 0x1ee59),
    (0x1ee5b, 0x1ee5b),
    (0x1ee5d, 0x1ee5d),
    (0x1ee5f, 0x1ee5f),
    (0x1ee61, 0x1ee62),
    (0x1ee64, 0x1ee64),
    (0x1ee67, 0x1ee6a),
    (0x1ee6c, 0x1ee72),
    (0x1ee74, 0x1ee77),
    (0x1ee79, 0x1ee7c),
    (0x1ee7e, 0x1ee7e),
    (0x1ee80, 0x1ee89),
    (0x1ee8b, 0x1ee9b),
    (0x1eea1, 0x1eea3),
    (0x1eea5, 0x1eea9),
    (0x1eeab, 0x1eebb),
    (0x1f100, 0x1f10c),
    (0x1fbf0, 0x1fbf9),
    (0x20000, 0x2a6df),
    (0x2a700, 0x2b739),
    (0x2b740, 0x2b81d),
    (0x2b820, 0x2cea1),
    (0x2ceb0, 0x2ebe0),
    (0x2f800, 0x2fa1d),
    (0x30000, 0x3134a),
    (0x31350, 0x323af),
];

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
    fn ignorecase_folds_like_python_re() {
        // Python re.IGNORECASE: i ~ dotless i, sigma ~ final sigma, s ~ long s.
        assert_eq!(hits("zqxjiv", "zqxj\u{131}v"), 1);
        assert_eq!(hits("zq\u{3c3}xjv", "zq\u{3c2}xjv"), 1);
        assert_eq!(hits("zqsxjv", "zq\u{17f}xjv"), 1);
        assert_eq!(hits("zqkxjv", "zq\u{212a}xjv"), 1);
        assert_eq!(hits("zq\u{3bc}xjv", "zq\u{b5}xjv"), 1);
        assert_eq!(hits("zqxjv", "zqyjv"), 0);
    }

    #[test]
    fn isalnum_follows_python_not_alphabetic() {
        // A combining mark is Alphabetic in Rust but not alnum in Python.
        assert!('\u{345}'.is_alphabetic());
        assert!(!py_isalnum('\u{345}'));
        assert!(!py_isalnum('\u{24b6}'));
        assert!(py_isalnum('\u{bd}'));
        assert!(py_isalnum('a') && py_isalnum('9') && !py_isalnum('_'));
        let text: Vec<char> = "x\u{345}zqxjvq".chars().collect();
        let kind = match_kind("zqxjv", &text, 2, 7, "zqxjv");
        assert_eq!(kind, "inside a longer word, likely a false match");
        let text: Vec<char> = "\u{345}zqxjv".chars().collect();
        assert_eq!(match_kind("zqxjv", &text, 1, 6, "zqxjv"), "whole word");
    }

    #[test]
    fn working_text_gets_universal_newlines() {
        assert_eq!(universal_newlines("a\r\nb\rc\n".into()), "a\nb\nc\n");
    }

    #[test]
    fn a_slow_command_times_out_and_fails() {
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let start = Instant::now();
        assert!(run_bounded(cmd, Duration::from_millis(100)).is_err());
        assert!(start.elapsed() < Duration::from_secs(4));
        let mut ok = Command::new("printf");
        ok.arg("hi");
        assert_eq!(run_bounded(ok, Duration::from_secs(5)).unwrap(), b"hi");
    }

    #[test]
    fn term_id_matches_the_python_formula() {
        // sha256("abc") starts with ba7816bf.
        assert_eq!(term_id("ABC"), "t-ba7816bf");
    }
}
