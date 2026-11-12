"""Explain a tracked project in layers, with a citation on every claim (issue #53).

`scaffold` turns a project record (`work project show`) into the reading list and
the fixed headings. `check` is the deterministic gate: a doc ships only when it
returns no problems. Deep mode (issue #223) adds a cited "How it runs" section. The
method notes live in `skills/careerbrew-project-explainer/references/explain-method.md`.
"""

from __future__ import annotations

import re
from typing import Any

FULL = [
    "One-minute version",
    "Context map",
    "Containers",
    "Codemap",
    "Invariants and boundaries",
    "Why it is this way",
    "Current state",
    "Gotchas",
    "Read next",
    "Check yourself",
    "Glossary",
    "Sources",
]
SHORT = ["One-minute version", "Containers", "Codemap", "Gotchas", "Read next", "Sources"]
# Deep mode runs the project first, so its doc says how it ran, right after the codemap.
DEEP = [*FULL[:4], "How it runs", *FULL[4:]]

CITE = re.compile(r"\[(S\d+)\]")
DATE_OR_SHA = re.compile(r"^(\d{4}-\d{2}-\d{2}|[0-9a-f]{7,40})$")
# URL, ticket key, doc id, repo@sha, or path.
LOCATOR = re.compile(r"^(https?://\S+|[A-Z][A-Z0-9]+-\d+|doc-[\w-]+|[\w./-]+@[0-9a-f]{7,40}|[\w.-]*/[\w./-]*|[\w-]+\.\w+)$")
ACRONYM = re.compile(r"\b[A-Z][A-Z0-9]*[A-Z][A-Z0-9]*s?\b")
ROW = re.compile(r"^\s*\|(.*)\|\s*$")
BULLET = re.compile(r"^(\s*)(?:[-*+]|\d+\.)\s+(.*)$")


def _sections(text: str) -> list[tuple[str, list[str]]]:
    out: list[tuple[str, list[str]]] = []
    for line in text.splitlines():
        if line.startswith("## "):
            out.append((line[3:].strip(), []))
        elif out:
            out[-1][1].append(line)
    return out


def _base(heading: str) -> str:
    """'Context map (C4 level 1)' -> 'Context map'."""
    return re.sub(r"\s*\(.*\)\s*$", "", heading)


def _table(lines: list[str]) -> tuple[list[str], list[list[str]]]:
    rows = [[c.strip() for c in m.group(1).split("|")] for ln in lines if (m := ROW.match(ln))]
    rows = [r for r in rows if not all(re.fullmatch(r":?-{2,}:?", c) for c in r)]
    return (rows[0], rows[1:]) if rows else ([], [])


def _plain(s: str) -> str:
    """Drop code spans, links, citations, and HTML tags before the acronym scan."""
    s = re.sub(r"`[^`]*`", " ", s)
    s = re.sub(r"https?://\S+", " ", s)
    s = re.sub(r"\[[^\]]*\]\([^)]*\)", " ", s)
    s = CITE.sub(" ", s)
    return re.sub(r"<[^>]+>", " ", s)


def sources_table(text: str) -> dict[str, dict[str, str]]:
    for h, lines in _sections(text):
        if _base(h) == "Sources":
            head, rows = _table(lines)
            cols = [c.lower() for c in head]
            return {r[0]: dict(zip(cols, r, strict=False)) for r in rows if r and r[0]}
    return {}


def source_problems(src: dict[str, dict[str, str]]) -> list[str]:
    """The Sources table rules, shared with the PRD checker (`prd.py`)."""
    problems: list[str] = []
    if not src:
        problems.append("Sources: a table with Id, Kind, Locator, Read columns is required")
    for sid, s in src.items():
        if s.get("kind", "").lower() not in ("primary", "lead"):
            problems.append(f"Sources {sid}: Kind must be primary or lead")
        if not LOCATOR.match(s.get("locator", "")):
            problems.append(f"Sources {sid}: Locator must be a URL, ticket key, doc id, repo@sha, or path")
        if not DATE_OR_SHA.match(s.get("read", "")):
            problems.append(f"Sources {sid}: Read must be a date (YYYY-MM-DD) or a commit sha")
    return problems


def check(text: str, short: bool = False, deep: bool = False) -> list[str]:
    """Every problem that blocks shipping the doc; an empty list means it passes."""
    problems: list[str] = []
    want = SHORT if short else DEEP if deep else FULL
    secs = _sections(text)
    got = [_base(h) for h, _ in secs]
    if got != want:
        problems.append(f"headings must be exactly, in order: {', '.join(want)} (got: {', '.join(got) or 'none'})")
    src = sources_table(text)
    problems += source_problems(src)
    used: set[str] = set()
    seen_acr: set[str] = set()
    for h, lines in secs:
        name = _base(h)
        if name == "Sources":
            continue
        claims: list[str] = []
        bullets = [(m.group(1), m.group(2)) for ln in lines if (m := BULLET.match(ln))]
        claims += [b for _, b in bullets]
        head, rows = _table(lines)
        claims += [" | ".join(r) for r in rows]
        for c in claims:
            ids = CITE.findall(c)
            used.update(ids)
            if not ids:
                problems.append(f"{name}: no citation on: {c[:60]}")
                continue
            for i in ids:
                if i not in src:
                    problems.append(f"{name}: cites {i}, which is not in Sources")
            if all(src.get(i, {}).get("kind", "").lower() == "lead" for i in ids if i in src) and all(i in src for i in ids):
                problems.append(f"{name}: backed only by leads: {c[:60]}")
        if name == "One-minute version" and (len(bullets) != 3 or any(ind for ind, _ in bullets)):
            problems.append("One-minute version: exactly 3 flat bullets (what, why, status)")
        if name == "Containers" and "deploys as" not in [x.lower() for x in head]:
            problems.append("Containers: the table needs a 'Deploys as' column")
        if name == "Why it is this way":
            cols = [x.lower() for x in head]
            if not {"state", "who", "when"} <= set(cols):
                problems.append("Why it is this way: the table needs State, Who, and When columns")
            else:
                si = cols.index("state")
                for r in rows:
                    state = r[si].lower() if si < len(r) else ""
                    if state not in ("decided", "proposed"):
                        problems.append(f"Why it is this way: State must be decided or proposed (got: {state or 'empty'})")
                    elif "considering" in " ".join(r).lower() and state != "proposed":
                        problems.append("Why it is this way: 'considering' means proposed, not decided")
        if name == "How it runs":
            if not bullets:
                problems.append("How it runs: list each step you ran, with its command")
            for _, b in bullets:
                if not re.search(r"`[^`]+`", b):
                    problems.append(f"How it runs: name the command each step ran, in backticks: {b[:60]}")
        if name == "Current state":
            for _, b in bullets:
                if "who:" not in b.lower():
                    problems.append(f"Current state: each open question names who can answer (Who: ...): {b[:60]}")
        if name == "Check yourself":
            if len([b for ind, b in bullets if not ind]) != 3:
                problems.append("Check yourself: exactly 3 questions")
            if "\n".join(lines).count("<details>") < 3:
                problems.append("Check yourself: hide each answer in <details>")
        if name != "Glossary":
            for ln in lines:
                plain = _plain(ln)
                for acr in ACRONYM.findall(plain):
                    key = acr.rstrip("s") if acr.endswith("s") and acr[:-1].isupper() else acr
                    if key in seen_acr:
                        continue
                    seen_acr.add(key)
                    if not re.search(rf"{re.escape(acr)}\s*\(|\({re.escape(key)}s?\)", plain):
                        problems.append(f"{name}: define {key} inline at first use, as 'Full name ({key})' or '{key} (full name)'")
    for sid in src:
        if sid not in used:
            problems.append(f"Sources {sid}: unused (cite it or remove it)")
    return problems


def scaffold(name: str, short: bool = False, deep: bool = False) -> str:
    """The reading list (stalest first) and the headings to fill; it fails `check` until filled."""
    from . import projects

    d: dict[str, Any] = projects.show(name)
    lines = [f"# {name}: explained", "", f"<!-- reading list from `careerbrew work project show {name}`: re-read each source now and record the date or sha -->", ""]
    for h in SHORT if short else DEEP if deep else FULL:
        lines += [f"## {h}", "", "- TODO", ""]
    lines[-2:] = ["| Id | Kind | Locator | Read |", "|---|---|---|---|"]
    for i, s in enumerate(d["sources"], 1):
        lines.append(f"| S{i} | primary | {s['ref']} | TODO |")
    return "\n".join(lines) + "\n"
