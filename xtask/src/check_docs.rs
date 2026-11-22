//! rust-27b slice 3: the native port of `scripts/check-docs.sh`, the docs
//! drift check. It reads files only and prints the same lines:
//! 1. every `docs/*.md` but `README.md` has a `(<name>)` link in
//!    `docs/README.md`;
//! 2. every `tests/e2e/us-*` path that `docs/user-stories.md` names exists;
//! 3. every regular file `tests/e2e/us-*` is named in `docs/user-stories.md`,
//!    and names its own story id (`us-01-x.sh` says `US-01`).
//!
//! The script's `grep -q` patterns are basic regular expressions; here `.`
//! matches any one character and every other character is literal. A file
//! name with another BRE special (`[`, `*`, `^`, `$`, `\`) is a known gap.
//! Names are sorted bytewise, like the script under `LC_ALL=C`.
use std::path::Path;

/// One output line per problem, then the pass line when there is none.
pub fn check(repo: &Path) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let read = |p: &str| std::fs::read(repo.join(p)).ok();
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
        if !readme
            .as_deref()
            .is_some_and(|t| bre_lite(t, pat.as_bytes()))
        {
            out.push(format!(
                "check-docs: docs/{base} has no row in docs/README.md"
            ));
        }
    }

    let stories = read("docs/user-stories.md");
    let mut named = story_paths(stories.as_deref().unwrap_or_default());
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
        if !stories
            .as_deref()
            .is_some_and(|s| bre_lite(s, t.as_bytes()))
        {
            out.push(format!(
                "check-docs: {t} is not listed in docs/user-stories.md"
            ));
        }
        let digits: String = base[3..].chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let id = format!("US-{digits}");
            let body = read(&t).unwrap_or_default();
            if !bre_lite(&body, id.as_bytes()) {
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

/// `grep -q <pat>` for a pattern whose only special character is `.`.
pub fn bre_lite(text: &[u8], pat: &[u8]) -> bool {
    text.split(|&c| c == b'\n').any(|line| {
        pat.len() <= line.len()
            && line
                .windows(pat.len())
                .any(|w| w.iter().zip(pat).all(|(&c, &p)| p == b'.' || c == p))
    })
}

#[cfg(test)]
mod tests {
    use super::{bre_lite, story_paths};

    #[test]
    fn story_paths_end_on_a_word_character() {
        let got = story_paths(b"see `tests/e2e/us-01-a.sh`. and tests/e2e/us-02-b.sh.\n");
        assert_eq!(got, ["tests/e2e/us-01-a.sh", "tests/e2e/us-02-b.sh"]);
        assert!(story_paths(b"tests/e2e/us-.").is_empty());
    }

    #[test]
    fn a_dot_matches_any_character() {
        assert!(bre_lite(b"x (aXmd) y", b"(a.md)"));
        assert!(!bre_lite(b"(a\n.md)", b"(a.md)"));
    }

    #[test]
    fn bre_anchors_apply_only_at_the_pattern_edges() {
        assert!(bre_lite(b"alpha", b"^alpha"));
        assert!(!bre_lite(b"xalpha", b"^alpha"));
        assert!(bre_lite(b"alpha", b"alpha$"));
        assert!(!bre_lite(b"alphax", b"alpha$"));
        assert!(bre_lite(b"x^y$z", b"x^y$z"));
    }

    #[test]
    fn bre_ranges_negation_and_escaped_specials_match() {
        assert!(bre_lite(b"(braket.md)", b"(br[ak]et.md)"));
        assert!(!bre_lite(b"(br[ak]et.md)", b"(br[ak]et.md)"));
        assert!(bre_lite(b"abc", b"[^z]*"));
        assert!(bre_lite(b"a*", r"a\*".as_bytes()));
        assert!(!bre_lite(b"aa", r"a\*".as_bytes()));
    }
}
