//! rust-27b slice 5: the native port of `scripts/learnings-table.py` without
//! `--split`: print every learning file as one Markdown table, or `--check`
//! the gate's format rules. It reads files only and prints the same lines.
//!
//! Python details it keeps: `str.splitlines` line breaks, `str.strip`
//! whitespace (Rust whitespace plus U+001C to U+001F), the `$` of the file
//! name pattern that also matches before one final newline, the bare-pipe
//! escape, `line[:60]` in code points, and the sort (date descending, then
//! name). Known gaps, so the gate still runs the Python script: a file name
//! with non-ASCII decimal digits in its date is a name problem here and a date
//! problem in Python; a file that is not UTF-8, a non-UTF-8 name, or a folder
//! with a learning's name prints a short error here where Python prints a
//! traceback (stdout and the exit code still agree).
use std::path::Path;

const SECTIONS: [&str; 3] = ["Predicted", "Observed", "Lesson"];
const HEADER: &str = "| Date | Change | Predicted | Observed | Lesson |\n|---|---|---|---|---|";

/// What one run prints, and its exit code.
pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn strip(s: &str) -> &str {
    s.trim_matches(py_space)
}

/// Python's `str.splitlines()`.
fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
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
            if let Some(&(j, '\n')) = chars.peek() {
                chars.next();
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

/// `(?<!\\)\|` replaced by `\|`.
fn escape_pipes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = None;
    for c in s.chars() {
        if c == '|' && prev != Some('\\') {
            out.push('\\');
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// `len(BARE_PIPE.split(line))`.
fn bare_parts(s: &str) -> usize {
    let mut n = 1;
    let mut prev = None;
    for c in s.chars() {
        if c == '|' && prev != Some('\\') {
            n += 1;
        }
        prev = Some(c);
    }
    n
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn cell(lines: &[&str]) -> String {
    let parts: Vec<&str> = lines
        .iter()
        .map(|l| strip(l))
        .filter(|l| !l.is_empty())
        .collect();
    escape_pipes(&parts.join(" "))
}

/// The date of `<YYYY-MM-DD>-<slug>.md` (Python `$` allows one final `\n`).
fn name_date(name: &str) -> Option<&str> {
    let core = name.strip_suffix('\n').unwrap_or(name);
    let slug = core.strip_suffix(".md")?;
    let b = slug.as_bytes();
    if b.len() < 12 || b[4] != b'-' || b[7] != b'-' || b[10] != b'-' {
        return None;
    }
    if ![0..4, 5..7, 8..10]
        .into_iter()
        .all(|r| b[r].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    let words = &slug[11..];
    let ok = words.split('-').all(|w| {
        !w.is_empty()
            && w.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    ok.then(|| &slug[..10])
}

/// `datetime.date.fromisoformat` for an ASCII `YYYY-MM-DD`.
fn valid_date(date: &str) -> bool {
    let n = |r: std::ops::Range<usize>| date[r].parse::<u32>().unwrap();
    let (y, m, d) = (n(0..4), n(5..7), n(8..10));
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    y >= 1 && d >= 1 && d <= days
}

/// The row's cells and problems for one learning file.
fn parse(
    name: &str,
    read: impl FnOnce() -> Result<String, String>,
) -> Result<(Vec<String>, Vec<String>), String> {
    let Some(date) = name_date(name) else {
        return Ok((
            Vec::new(),
            vec![format!(
                "{name}: name it <YYYY-MM-DD>-<slug>.md (lower-case words joined by -)"
            )],
        ));
    };
    let mut problems = Vec::new();
    if !valid_date(date) {
        problems.push(format!("{name}: {date} is not a date"));
    }
    let text = read()?;
    let mut change = String::new();
    let mut parts: Vec<(&str, Vec<&str>)> = Vec::new();
    let mut current: Option<usize> = None;
    for (i, line) in splitlines(&text).into_iter().enumerate() {
        let n = i + 1;
        if line.starts_with("# ") && change.is_empty() && parts.is_empty() {
            change = strip(&line[2..]).to_string();
        } else if let Some(rest) = line.strip_prefix("## ") {
            let title = strip(rest);
            let known = SECTIONS.iter().find(|s| **s == title);
            match known {
                Some(s) if !parts.iter().any(|(t, _)| t == s) => {
                    parts.push((s, Vec::new()));
                    current = Some(parts.len() - 1);
                }
                _ => {
                    problems.push(format!(
                        "{name}:{n}: unexpected heading '{}' (use {}, once each)",
                        strip(line),
                        SECTIONS.join(", ")
                    ));
                    current = None;
                }
            }
        } else if let Some(k) = current {
            parts[k].1.push(line);
        } else if !strip(line).is_empty() {
            problems.push(format!(
                "{name}:{n}: text outside a section; start the file with '# <what changed>', then the sections"
            ));
        }
    }
    if change.is_empty() {
        problems.push(format!("{name}: no '# <what changed>' line at the top"));
    }
    let order: Vec<&str> = parts.iter().map(|(t, _)| *t).collect();
    let expected: Vec<&str> = SECTIONS
        .iter()
        .copied()
        .filter(|s| order.contains(s))
        .collect();
    if order != expected || order.len() != SECTIONS.len() {
        let want: Vec<String> = SECTIONS.iter().map(|s| format!("## {s}")).collect();
        problems.push(format!(
            "{name}: needs the sections {} in that order",
            want.join(", ")
        ));
    }
    for (title, body) in &parts {
        if cell(body).is_empty() {
            problems.push(format!("{name}: '## {title}' is empty"));
        }
    }
    let mut cells = vec![date.to_string(), escape_pipes(&change)];
    for s in SECTIONS {
        let body = parts
            .iter()
            .find(|(t, _)| *t == s)
            .map(|(_, b)| b.as_slice())
            .unwrap_or(&[]);
        cells.push(cell(body));
    }
    Ok((cells, problems))
}

type Entries = (Vec<Vec<String>>, Vec<String>);

fn entries(repo: &Path) -> Result<Entries, String> {
    let folder = repo.join("docs").join("learnings");
    let mut names = Vec::new();
    if folder.is_dir() {
        for entry in
            std::fs::read_dir(&folder).map_err(|e| format!("cannot list docs/learnings: {e}"))?
        {
            let entry = entry.map_err(|e| format!("cannot list docs/learnings: {e}"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "a file name in docs/learnings is not UTF-8".to_string())?;
            names.push(name);
        }
    }
    names.sort();
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for name in names {
        let path = folder.join(&name);
        let read = || {
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {name}: {e}"))?;
            String::from_utf8(bytes).map_err(|_| format!("{name} is not UTF-8"))
        };
        let (cells, bad) = parse(&name, read)?;
        let clean = bad.is_empty();
        problems.extend(bad);
        if clean {
            found.push((name, cells));
        }
    }
    // Python sorts by name, then (stable) by date, newest first.
    found.sort_by(|a, b| b.1[0].cmp(&a.1[0]).then_with(|| a.0.cmp(&b.0)));
    Ok((found.into_iter().map(|(_, c)| c).collect(), problems))
}

fn table(rows: &[Vec<String>]) -> String {
    let mut out = HEADER.to_string();
    for cells in rows {
        out.push_str("\n| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |");
    }
    out.push('\n');
    out
}

/// `^\|\s*(Date\s*\||-)` with Python's Unicode `\s`.
fn header_row(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('|') else {
        return false;
    };
    let rest = rest.trim_start_matches(py_space);
    rest.starts_with('-')
        || rest
            .strip_prefix("Date")
            .is_some_and(|r| r.trim_start_matches(py_space).starts_with('|'))
}

fn stray_rows(repo: &Path) -> Result<Vec<(usize, String)>, String> {
    let path = repo.join("docs").join("learnings.md");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read docs/learnings.md: {e}"))?;
    let text =
        String::from_utf8(bytes).map_err(|_| "docs/learnings.md is not UTF-8".to_string())?;
    Ok(splitlines(&text)
        .into_iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('|') && !header_row(l))
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect())
}

fn check(repo: &Path) -> Result<Run, String> {
    let (rows, mut problems) = entries(repo)?;
    for (n, line) in stray_rows(repo)? {
        problems.push(format!(
            "docs/learnings.md:{n}: a table row; each learning is its own file now. Run `uv run python scripts/learnings-table.py --split` ({})",
            head(&line, 60)
        ));
    }
    let text = table(&rows);
    for (i, line) in splitlines(&text).into_iter().skip(2).enumerate() {
        if bare_parts(line) != 7 {
            problems.push(format!(
                "learnings: row {} does not have 5 cells: {}",
                i + 1,
                head(line, 60)
            ));
        }
    }
    let mut stdout = String::new();
    for p in &problems {
        stdout.push_str(&format!("learnings: {p}\n"));
    }
    stdout.push_str(&format!(
        "learnings: {} learning file(s), {} problem(s)\n",
        rows.len(),
        problems.len()
    ));
    let code = u8::from(!problems.is_empty());
    Ok(Run {
        stdout,
        stderr: String::new(),
        code,
    })
}

fn print(repo: &Path) -> Result<Run, String> {
    let (rows, problems) = entries(repo)?;
    let stderr = problems
        .iter()
        .map(|p| format!("learnings: skipped {p}\n"))
        .collect();
    Ok(Run {
        stdout: table(&rows),
        stderr,
        code: 0,
    })
}

/// Run one mode over `repo`. A read error prints only a short line on
/// stderr and exits 1, like the Python traceback's stdout and exit code.
pub fn run(repo: &Path, check_mode: bool) -> Run {
    let result = if check_mode { check(repo) } else { print(repo) };
    result.unwrap_or_else(|error| Run {
        stdout: String::new(),
        stderr: format!("learnings: {error}\n"),
        code: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(splitlines("a\r\nb\rc\u{2028}d\n"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert!(splitlines("").is_empty());
    }

    #[test]
    fn names_and_dates() {
        assert_eq!(name_date("2026-01-02-a-b1.md"), Some("2026-01-02"));
        assert_eq!(name_date("2026-01-02-a.md\n"), Some("2026-01-02"));
        assert_eq!(name_date("2026-01-02-a--b.md"), None);
        assert_eq!(name_date("2026-01-02-A.md"), None);
        assert!(valid_date("2024-02-29") && !valid_date("2023-02-29") && !valid_date("0000-01-01"));
    }
}
