//! Differential test of the native PyYAML emitter (`careerbrew_cli::pyyaml`)
//! against real PyYAML: seeded random synthetic values, each dumped by both
//! sides. Every value the native side writes must match byte for byte, and
//! the native side must write at least 90% of the corpus.
use careerbrew_cli::pyyaml::safe_dump;
use careerbrew_vault::canon::Canon;
use std::path::Path;
use std::process::Command;

/// Values per seed, and the seeds: 24,000 values in all.
const CASES: usize = 8000;
const SEEDS: [u64; 3] = [
    0x5EED_CAFE_F00D_0001,
    0x0DDB_A115_5EED_0002,
    0x7E57_DA7A_0000_0003,
];
const MIN_SHARE: f64 = 0.90;

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Text the YAML 1.1 resolver, the scalar analyzer, or the writers treat
/// specially.
const PIECES: &[&str] = &[
    "a",
    "word",
    "Hello",
    "x_y",
    "foo-bar",
    "CamelCase",
    "yes",
    "Yes",
    "YES",
    "no",
    "No",
    "y",
    "n",
    "Y",
    "N",
    "true",
    "True",
    "TRUE",
    "false",
    "on",
    "On",
    "off",
    "OFF",
    "null",
    "Null",
    "NULL",
    "~",
    "",
    "1",
    "0",
    "-0",
    "+1",
    "007",
    "0o7",
    "0b101",
    "0b2",
    "0x1F",
    "0xg",
    "1_000",
    "1.0",
    "-1.5",
    "1.",
    ".5",
    "1e5",
    "1.5e+3",
    "1.5e3",
    "1.0e-07",
    ".inf",
    "-.Inf",
    "+.INF",
    ".nan",
    ".NaN",
    "12:30",
    "08:30",
    "19:00",
    "1:2:3",
    "190:20:30",
    "1:60",
    "1:59.5",
    "-1:30",
    "2026-11-28",
    "2026-1-2",
    "2026-11-28 10:00:00",
    "2026-1-2t3:04:05.5Z",
    "2026-11-28T10:00:00+05:30",
    "2001-12-14 21:59:43.10 -5",
    "<<",
    "=",
    "-",
    "- x",
    "--",
    "---",
    "...",
    "....",
    "?",
    "? x",
    ":",
    ": x",
    "a: b",
    "a:b",
    "a #b",
    "a#b",
    "#x",
    "[a]",
    "]",
    "{a}",
    "}",
    ",",
    "a,b",
    "&a",
    "*a",
    "!t",
    "|",
    ">",
    "'",
    "it's",
    "\"",
    "say \"hi\"",
    "%",
    "@",
    "`",
    "\\",
    "a\\b",
    "\t",
    "a\tb",
    "\n",
    "a\nb",
    "a\n\nb",
    " \n",
    "\n ",
    "a \nb",
    "a\n b",
    "\r",
    "\r\n",
    "\u{85}",
    "\u{2028}",
    "\u{2029}",
    "\u{A0}",
    "\u{FEFF}",
    "\u{0}",
    "\u{7}",
    "\u{1B}",
    "\u{7F}",
    "\u{80}",
    "\u{9F}",
    "\u{D7FF}",
    "\u{E000}",
    "\u{FFFD}",
    "\u{FFFE}",
    "\u{1F600}",
    "\u{10FFFF}",
    "\u{10000}",
    "\u{4E2D}\u{6587}",
    "\u{416}\u{438}\u{437}\u{43D}\u{44C}",
    "e\u{301}",
    "caf\u{E9}",
    " ",
    "  ",
    "x ",
    " x",
];

fn words(rng: &mut Rng, max: usize) -> String {
    let mut out = String::new();
    let target = 1 + rng.below(max);
    while out.chars().count() < target {
        if !out.is_empty() {
            out.push_str(if rng.chance(85) { " " } else { "  " });
        }
        if rng.chance(10) {
            out.push_str(rng.pick(PIECES));
        } else {
            let len = 1 + rng.below(12);
            for _ in 0..len {
                out.push((b'a' + rng.below(26) as u8) as char);
            }
        }
    }
    out
}

fn text(rng: &mut Rng) -> String {
    match rng.below(10) {
        // One special piece alone.
        0..=3 => rng.pick(PIECES).to_string(),
        // Long prose, for the 80-column folding of each style.
        4..=5 => {
            let body = words(rng, 260);
            match rng.below(6) {
                0 => format!(" {body}"),
                1 => format!("{body}\u{7}{}", words(rng, 60)),
                2 => format!("{body}\n{}", words(rng, 120)),
                3 => format!("{body} \n {}", words(rng, 120)),
                4 => format!("'{body}'"),
                _ => body,
            }
        }
        // Pieces glued together.
        _ => {
            let mut out = String::new();
            for _ in 0..1 + rng.below(5) {
                out.push_str(rng.pick(PIECES));
                out.push_str(rng.pick(&["", "", " ", "\n", "  ", ":", "#", " #", ": "]));
            }
            out
        }
    }
}

fn float(rng: &mut Rng) -> f64 {
    match rng.below(4) {
        0 => f64::from_bits(rng.next()),
        1 => *rng.pick(&[
            0.0,
            -0.0,
            1.0,
            0.1,
            1e16,
            1e15,
            1e17,
            1e-4,
            1e-5,
            123456789012345680.0,
            f64::MAX,
            f64::MIN_POSITIVE,
            5e-324,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ]),
        _ => {
            let n = rng.next() as i64 % 10_000_000;
            n as f64 / 10f64.powi(rng.below(12) as i32)
        }
    }
}

fn int(rng: &mut Rng) -> Canon {
    let text = match rng.below(4) {
        0 => (rng.below(10)).to_string(),
        1 => (rng.next() as i64).to_string(),
        2 => format!("-{}", rng.below(1000)),
        _ => format!(
            "{}{:019}",
            rng.next(),
            rng.next() % 10_000_000_000_000_000_000
        ),
    };
    Canon::Int(text.parse().unwrap())
}

fn date(rng: &mut Rng) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        1 + rng.below(9999),
        1 + rng.below(12),
        1 + rng.below(28)
    )
}

fn datetime(rng: &mut Rng) -> String {
    let mut out = format!(
        "{}T{:02}:{:02}:{:02}",
        date(rng),
        rng.below(24),
        rng.below(60),
        rng.below(60)
    );
    if rng.chance(30) {
        out.push_str(&format!(".{:06}", 1 + rng.below(999_999)));
    }
    match rng.below(4) {
        0 => out.push_str("+00:00"),
        1 => out.push_str(&format!("-{:02}:{:02}", 1 + rng.below(12), rng.below(60))),
        // A zone with seconds: the native side refuses it.
        2 if rng.chance(10) => out.push_str("+05:30:15"),
        _ => {}
    }
    out
}

fn scalar(rng: &mut Rng) -> Canon {
    match rng.below(20) {
        0 => Canon::Null,
        1 => Canon::Bool(rng.chance(50)),
        2 | 3 => int(rng),
        4 | 5 => Canon::Float(float(rng)),
        6 => Canon::Date(date(rng)),
        7 => Canon::DateTime(datetime(rng)),
        _ => Canon::Str(text(rng)),
    }
}

fn key(rng: &mut Rng) -> Canon {
    match rng.below(40) {
        0 => Canon::Int(rng.below(100).into()),
        1 => Canon::Null,
        2 => Canon::Date(date(rng)),
        // A key PyYAML writes as `? key` (empty, multi-line, or long).
        3 => Canon::Str(rng.pick(&["", "a\nb", "\n"]).to_string()),
        4 => Canon::Str(words(rng, 200)),
        5 => Canon::Bool(rng.chance(50)),
        6..=15 => Canon::Str(text(rng)),
        _ => Canon::Str(words(rng, 12).replace(' ', "_")),
    }
}

fn value(rng: &mut Rng, depth: usize) -> Canon {
    let leaf = depth >= 4 || rng.chance(45);
    if leaf {
        return scalar(rng);
    }
    if rng.chance(50) {
        let len = if rng.chance(10) { 0 } else { 1 + rng.below(5) };
        Canon::List((0..len).map(|_| value(rng, depth + 1)).collect())
    } else {
        let len = if rng.chance(10) { 0 } else { 1 + rng.below(5) };
        let mut pairs: Vec<(Canon, Canon)> = Vec::new();
        for _ in 0..len {
            let k = key(rng);
            // Python dict keys are unique, and `True == 1`.
            let clash = |a: &Canon, b: &Canon| {
                let id = |c: &Canon| match c {
                    Canon::Bool(b) => Canon::Int(u8::from(*b).into()),
                    other => other.clone(),
                };
                id(a) == id(b)
            };
            if pairs.iter().any(|(seen, _)| clash(seen, &k)) {
                continue;
            }
            pairs.push((k, value(rng, depth + 1)));
        }
        Canon::Map(pairs)
    }
}

/// The oracle's answers: per value, its UTF-8 byte length, a newline, then
/// the text.
fn framed(mut bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    while !bytes.is_empty() {
        let nl = bytes.iter().position(|b| *b == b'\n').unwrap();
        let len: usize = std::str::from_utf8(&bytes[..nl]).unwrap().parse().unwrap();
        let body = &bytes[nl + 1..nl + 1 + len];
        out.push(String::from_utf8(body.to_vec()).unwrap());
        bytes = &bytes[nl + 1 + len..];
    }
    out
}

#[test]
fn native_emitter_matches_pyyaml() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases: Vec<Canon> = SEEDS
        .iter()
        .flat_map(|seed| {
            let mut rng = Rng(*seed);
            (0..CASES).map(move |_| value(&mut rng, 0))
        })
        .collect();
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("cases.json");
    let typed: Vec<String> = cases.iter().map(|c| c.to_json().to_string()).collect();
    std::fs::write(&input, format!("[{}]", typed.join(","))).unwrap();
    let run = Command::new(repo.join(".venv/bin/python"))
        .arg(repo.join("tests/oracles/pyyaml_emitter.py"))
        .arg(&input)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let want = framed(&run.stdout);
    assert_eq!(want.len(), cases.len());
    let mut written = 0;
    let mut differ = Vec::new();
    for (i, (case, want)) in cases.iter().zip(&want).enumerate() {
        if let Some(got) = safe_dump(case) {
            written += 1;
            if &got != want {
                differ.push(format!("case {i}: {case:?}\n want {want:?}\n  got {got:?}"));
            }
        }
    }
    let share = written as f64 / cases.len() as f64;
    eprintln!(
        "pyyaml emitter: {written}/{} written natively ({:.1}%)",
        cases.len(),
        share * 100.0
    );
    assert!(
        differ.is_empty(),
        "{} of {written} differ; first:\n{}",
        differ.len(),
        differ
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        share >= MIN_SHARE,
        "native share {share:.3} is under {MIN_SHARE}"
    );
}

/// Negative control: a value the native side refuses still dumps in Python.
#[test]
fn refused_values_are_refused() {
    let float_key = Canon::Map(vec![(Canon::Float(1.5), Canon::Null)]);
    assert_eq!(safe_dump(&float_key), None);
    let zone_seconds = Canon::DateTime("2026-11-28T10:00:00+05:30:15".into());
    assert_eq!(safe_dump(&zone_seconds), None);
}
