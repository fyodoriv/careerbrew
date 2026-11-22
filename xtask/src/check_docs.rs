//! rust-27b slice 3: the native port of `scripts/check-docs.sh`, the docs
//! drift check. It reads files only and prints the same lines:
//! 1. every `docs/*.md` but `README.md` has a `(<name>)` link in
//!    `docs/README.md`;
//! 2. every `tests/e2e/us-*` path that `docs/user-stories.md` names exists;
//! 3. every regular file `tests/e2e/us-*` is named in `docs/user-stories.md`,
//!    and names its own story id (`us-01-x.sh` says `US-01`).
//!
//! The script's `grep -q` patterns use the platform's POSIX basic regular
//! expression engine. Names are sorted bytewise, like the script under
//! `LC_ALL=C`.
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::Path;

/// One output line per problem, then the pass line when there is none.
pub fn check(repo: &Path) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let read = |p: &str| std::fs::read(repo.join(p));
    let readme = read("docs/README.md");

    let mut docs = list(&repo.join("docs"), |n| n.ends_with(".md"));
    if docs.is_empty() {
        // Without nullglob the script loops once over the literal pattern.
        docs.push("*.md".to_string());
    }
    for base in &docs {
        if base == "README.md" {
            continue;
        }
        let pat = format!("({base})");
        if !grep_file(&readme, "docs/README.md", pat.as_bytes()) {
            out.push(format!(
                "check-docs: docs/{base} has no row in docs/README.md"
            ));
        }
    }

    let stories = read("docs/user-stories.md");
    let mut named = match &stories {
        Ok(text) => story_paths(text),
        Err(error) => {
            grep_error("docs/user-stories.md", error);
            Vec::new()
        }
    };
    named.sort();
    named.dedup();
    for t in &named {
        if !repo.join(t).exists() {
            out.push(format!(
                "check-docs: docs/user-stories.md names missing test {t}"
            ));
        }
    }

    for base in list(&repo.join("tests/e2e"), |n| n.starts_with("us-")) {
        let t = format!("tests/e2e/{base}");
        if !repo.join(&t).is_file() {
            continue;
        }
        if !grep_file(&stories, "docs/user-stories.md", t.as_bytes()) {
            out.push(format!(
                "check-docs: {t} is not listed in docs/user-stories.md"
            ));
        }
        let digits: String = base[3..].chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let id = format!("US-{digits}");
            if !grep_file(&read(&t), &t, id.as_bytes()) {
                out.push(format!("check-docs: {t} does not name its story {id}"));
            }
        }
    }

    let ok = out.is_empty();
    if ok {
        out.push("check-docs: docs match the code".to_string());
    }
    (out, ok)
}

/// Match one grep call against a previously read file, retaining its diagnostic.
fn grep_file(file: &std::io::Result<Vec<u8>>, path: &str, pat: &[u8]) -> bool {
    match file {
        Ok(text) => bre_matches(text, pat),
        Err(error) => {
            grep_error(path, error);
            false
        }
    }
}

fn grep_error(path: &str, error: &std::io::Error) {
    if error.kind() == std::io::ErrorKind::NotFound {
        eprintln!("grep: {path}: No such file or directory");
    } else {
        eprintln!("grep: {path}: {error}");
    }
}

/// File names in `dir` that pass `keep`, sorted bytewise; hidden names
/// are left out, like a shell glob.
fn list(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.') && keep(n))
        .collect();
    names.sort();
    names
}

/// `grep -oE 'tests/e2e/us-[A-Za-z0-9_./-]*[A-Za-z0-9_]'`: leftmost-longest
/// matches, scanned left to right without overlap, line by line.
pub fn story_paths(text: &[u8]) -> Vec<String> {
    const HEAD: &[u8] = b"tests/e2e/us-";
    let tail = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let body = |c: u8| tail(c) || matches!(c, b'.' | b'/' | b'-');
    let mut found = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        let mut i = 0;
        while i + HEAD.len() <= line.len() {
            if &line[i..i + HEAD.len()] != HEAD {
                i += 1;
                continue;
            }
            let start = i + HEAD.len();
            let mut end = start;
            while end < line.len() && body(line[end]) {
                end += 1;
            }
            while end > start && !tail(line[end - 1]) {
                end -= 1;
            }
            if end == start {
                i += 1;
                continue;
            }
            found.push(String::from_utf8_lossy(&line[i..end]).into_owned());
            i = end;
        }
    }
    found
}

/// `grep -q <pat>` with a POSIX BRE, one line at a time.
pub fn bre_matches(text: &[u8], pat: &[u8]) -> bool {
    let regex = match Bre::new(pat) {
        Ok(regex) => regex,
        Err(error) => {
            eprintln!("grep: {error}");
            return false;
        }
    };
    text.split(|&c| c == b'\n').any(|line| regex.is_match(line))
}

/// An invocation-scoped compiled BRE; `Drop` releases its libc allocations.
struct Bre(libc::regex_t);

impl Bre {
    fn new(pat: &[u8]) -> Result<Self, String> {
        let pat = CString::new(pat).expect("filename patterns cannot contain NUL");
        let mut regex = MaybeUninit::zeroed();
        // SAFETY: `regex` points to writable storage and `pat` is NUL-terminated.
        let status = unsafe { libc::regcomp(regex.as_mut_ptr(), pat.as_ptr(), 0) };
        if status != 0 {
            return Err(regex_error(status, regex.as_ptr()));
        }
        // SAFETY: successful regcomp initialized this exact storage.
        Ok(Self(unsafe { regex.assume_init() }))
    }

    fn is_match(&self, line: &[u8]) -> bool {
        let Ok(len) = libc::regoff_t::try_from(line.len()) else {
            return false;
        };
        // REG_STARTEND bounds the read; the trailing NUL also satisfies regexec.
        let mut input = Vec::with_capacity(line.len() + 1);
        input.extend_from_slice(line);
        input.push(0);
        let mut range = libc::regmatch_t {
            rm_so: 0,
            rm_eo: len,
        };
        // SAFETY: the compiled regex and input outlive this call; `range` is
        // writable and REG_STARTEND makes its initial bounds the input range.
        unsafe {
            libc::regexec(
                &self.0,
                input.as_ptr().cast(),
                1,
                &mut range,
                libc::REG_STARTEND,
            ) == 0
        }
    }
}

/// Return the same platform diagnostic that grep obtains from `regerror`.
fn regex_error(status: libc::c_int, regex: *const libc::regex_t) -> String {
    // SAFETY: status came from regcomp with this regex storage. A zero buffer
    // size ignores the pointer; the second call writes into the sized buffer.
    unsafe {
        let size = libc::regerror(status, regex, std::ptr::null_mut(), 0);
        let mut buffer = vec![0_u8; size];
        libc::regerror(status, regex, buffer.as_mut_ptr().cast(), size);
        if buffer.last() == Some(&0) {
            buffer.pop();
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }
}

impl Drop for Bre {
    fn drop(&mut self) {
        // SAFETY: this owner holds one successfully compiled regex and drops once.
        unsafe { libc::regfree(&mut self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::{bre_matches, story_paths};

    #[test]
    fn story_paths_end_on_a_word_character() {
        let got = story_paths(b"see `tests/e2e/us-01-a.sh`. and tests/e2e/us-02-b.sh.\n");
        assert_eq!(got, ["tests/e2e/us-01-a.sh", "tests/e2e/us-02-b.sh"]);
        assert!(story_paths(b"tests/e2e/us-.").is_empty());
    }

    #[test]
    fn a_dot_matches_any_character() {
        assert!(bre_matches(b"x (aXmd) y", b"(a.md)"));
        assert!(!bre_matches(b"(a\n.md)", b"(a.md)"));
    }

    #[test]
    fn bre_anchors_apply_only_at_the_pattern_edges() {
        assert!(bre_matches(b"alpha", b"^alpha"));
        assert!(!bre_matches(b"xalpha", b"^alpha"));
        assert!(bre_matches(b"alpha", b"alpha$"));
        assert!(!bre_matches(b"alphax", b"alpha$"));
        assert!(bre_matches(b"x^y$z", b"x^y$z"));
    }

    #[test]
    fn bre_ranges_negation_and_escaped_specials_match() {
        assert!(bre_matches(b"(braet.md)", b"(br[ak]et.md)"));
        assert!(bre_matches(b"(brket.md)", b"(br[ak]et.md)"));
        assert!(!bre_matches(b"(braket.md)", b"(br[ak]et.md)"));
        assert!(!bre_matches(b"(br[ak]et.md)", b"(br[ak]et.md)"));
        assert!(bre_matches(b"abc", b"[^z]*"));
        assert!(bre_matches(b"a*", r"a\*".as_bytes()));
        assert!(!bre_matches(b"aa", r"a\*".as_bytes()));
    }
}
