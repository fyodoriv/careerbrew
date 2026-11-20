"""Gate step `rust-policy-io`: the policy crate's library (`crates/policy`) does no IO.

Usage: python -I scripts/policy-no-io.py [<crate-dir>]   (default: crates/policy)

docs/rust-migration.md, rust-04 "Ownership split": the crate never touches
files, the environment, processes, the network, the clock or randomness.
The caller supplies terms, records, the directory, `now` and `id`.

It checks every `.rs` file under `<crate-dir>/src/` except `src/bin/` (the
`policy-vectors` driver is the one IO edge: it reads the vector file), and
the crate's `[dependencies]` against a fixed pure list. Each finding prints
`<path>:<line>: <rule>`; exit 1 on any finding, 0 when clean, 2 on a usage
error or a missing crate.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

# Each rule: a name and a pattern over one source line (comments removed).
RULES = [
    ("std::fs", re.compile(r"\bstd\s*::\s*fs\b")),
    ("std::env", re.compile(r"\bstd\s*::\s*env\b")),
    ("std::process", re.compile(r"\bstd\s*::\s*process\b")),
    ("std::net", re.compile(r"\bstd\s*::\s*net\b")),
    ("std::io", re.compile(r"\bstd\s*::\s*io\b")),
    ("std::os", re.compile(r"\bstd\s*::\s*os\b")),
    ("std::time", re.compile(r"\bstd\s*::\s*time\b")),
    ("std::thread", re.compile(r"\bstd\s*::\s*thread\b")),
    ("std::{...} import of fs, env, process, net, io, os, time or thread", re.compile(r"\bstd\s*::\s*\{[^}]*\b(?:fs|env|process|net|io|os|time|thread)\b")),
    ("SystemTime", re.compile(r"\bSystemTime\b")),
    ("Instant", re.compile(r"\bInstant\b")),
    ("RNG", re.compile(r"\b(?:rand|getrandom|fastrand|RandomState|thread_rng|OsRng)\b")),
    ("env! or option_env! macro", re.compile(r"\b(?:option_)?env!\s*\(")),
    ("include! macro", re.compile(r"\binclude(?:_str|_bytes)?!\s*\(")),
    ("extern block", re.compile(r"\bextern\s+\"C\"")),
    ("unsafe", re.compile(r"\bunsafe\b")),
    ("print macro", re.compile(r"\b(?:e?print(?:ln)?|dbg)!\s*\(")),
]

PURE_DEPENDENCIES = {"serde_json", "sha2", "unicode-normalization"}

_LINE_COMMENT = re.compile(r"//.*$")
_STRING = re.compile(r'"(?:\\.|[^"\\])*"')


def _code(line: str) -> str:
    """The line without string literals and `//` comments (doc text may name a rule)."""
    return _LINE_COMMENT.sub("", _STRING.sub('""', line))


def check(crate: Path) -> list[str]:
    src = crate / "src"
    findings: list[str] = []
    for path in sorted(src.rglob("*.rs")):
        rel = path.relative_to(crate)
        if rel.parts[:2] == ("src", "bin"):
            continue
        in_block = False
        for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            line = raw
            if in_block:
                if "*/" not in line:
                    continue
                line, in_block = line.split("*/", 1)[1], False
            while "/*" in _code(line):
                head, _, tail = line.partition("/*")
                if "*/" in tail:
                    line = head + " " + tail.split("*/", 1)[1]
                else:
                    line, in_block = head, True
            code = _code(line)
            for name, pattern in RULES:
                if pattern.search(code):
                    findings.append(f"{crate.name}/{rel}:{number}: {name}")
    manifest = tomllib.loads((crate / "Cargo.toml").read_text(encoding="utf-8"))
    for section in ("dependencies", "build-dependencies"):
        for dep in sorted(manifest.get(section, {})):
            if dep not in PURE_DEPENDENCIES:
                findings.append(f"{crate.name}/Cargo.toml: {section} {dep} is not on the pure list")
    if (crate / "build.rs").exists():
        findings.append(f"{crate.name}/build.rs: a build script runs code at build time")
    return findings


def main(argv: list[str]) -> int:
    if len(argv) > 1:
        print("usage: policy-no-io.py [<crate-dir>]", file=sys.stderr)
        return 2
    crate = Path(argv[0]) if argv else Path(__file__).resolve().parents[1] / "crates" / "policy"
    if not (crate / "Cargo.toml").is_file() or not (crate / "src").is_dir():
        print("policy-no-io: the policy crate is missing", file=sys.stderr)
        return 2
    findings = check(crate)
    for line in findings:
        print(line)
    print(f"policy-no-io: {len(findings)} finding(s)")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
