#!/usr/bin/env python3
"""The learnings log: one file per change in docs/learnings/, read as one table.

  uv run python scripts/learnings-table.py          print every learning as one Markdown table, newest first
  uv run python scripts/learnings-table.py --check  gate step: fail on a learning file that breaks the format,
                                                    or on a table row written into docs/learnings.md
  uv run python scripts/learnings-table.py --split  move table rows out of docs/learnings.md into their own files
                                                    (a row that already has its file is only removed)

Each change writes its own file, so two pull requests never edit the same file. One shared
table made every open PR conflict on GitHub after each merge: GitHub ignores a
`.gitattributes` union merge when it decides whether a PR can merge.

A learning file is docs/learnings/<YYYY-MM-DD>-<slug>.md:

    # <what changed>

    ## Predicted

    <text>

    ## Observed

    <text>

    ## Lesson

    <text>

Line breaks inside a section become spaces in the table, and `|` is escaped.
"""

from __future__ import annotations

import argparse
import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FOLDER = Path("docs") / "learnings"
INDEX = Path("docs") / "learnings.md"
NAME = re.compile(r"^(\d{4}-\d{2}-\d{2})-([a-z0-9]+(?:-[a-z0-9]+)*)\.md$")
SECTIONS = ("Predicted", "Observed", "Lesson")
HEADER = "| Date | Change | Predicted | Observed | Lesson |\n|---|---|---|---|---|"
# A pipe that is not escaped yet ends a table cell, even inside a code span.
BARE_PIPE = re.compile(r"(?<!\\)\|")
STOP_WORDS = {"a", "an", "and", "as", "at", "by", "for", "from", "in", "is", "it", "its", "of", "on", "or", "the", "to", "with"}


def cell(lines: list[str]) -> str:
    return BARE_PIPE.sub(r"\\|", " ".join(line.strip() for line in lines if line.strip()))


def parse(path: Path) -> tuple[list[str], list[str]]:
    """Return (the row's cells, problems) for one learning file."""
    name = NAME.match(path.name)
    if not name:
        return [], [f"{path.name}: name it <YYYY-MM-DD>-<slug>.md (lower-case words joined by -)"]
    problems: list[str] = []
    try:
        datetime.date.fromisoformat(name.group(1))
    except ValueError:
        problems.append(f"{path.name}: {name.group(1)} is not a date")
    change = ""
    parts: dict[str, list[str]] = {}
    current: list[str] | None = None
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith("# ") and not change and not parts:
            change = line[2:].strip()
        elif line.startswith("## "):
            title = line[3:].strip()
            if title not in SECTIONS or title in parts:
                problems.append(f"{path.name}:{n}: unexpected heading '{line.strip()}' (use {', '.join(SECTIONS)}, once each)")
                current = None
                continue
            current = parts[title] = []
        elif current is not None:
            current.append(line)
        elif line.strip():
            problems.append(f"{path.name}:{n}: text outside a section; start the file with '# <what changed>', then the sections")
    if not change:
        problems.append(f"{path.name}: no '# <what changed>' line at the top")
    if list(parts) != [s for s in SECTIONS if s in parts] or set(parts) != set(SECTIONS):
        problems.append(f"{path.name}: needs the sections {', '.join(f'## {s}' for s in SECTIONS)} in that order")
    for title, body in parts.items():
        if not cell(body):
            problems.append(f"{path.name}: '## {title}' is empty")
    cells = [name.group(1), BARE_PIPE.sub(r"\\|", change), *(cell(parts.get(s, [])) for s in SECTIONS)]
    return cells, problems


def entries(root: Path = ROOT) -> tuple[list[list[str]], list[str]]:
    """All learnings, newest first (then by slug), and every problem found."""
    found: list[tuple[str, str, list[str]]] = []
    problems: list[str] = []
    folder = root / FOLDER
    for path in sorted(folder.iterdir()) if folder.is_dir() else []:
        cells, bad = parse(path)
        problems += bad
        if not bad:
            found.append((cells[0], path.name, cells))
    found.sort(key=lambda e: e[1])
    found.sort(key=lambda e: e[0], reverse=True)
    return [cells for _, _, cells in found], problems


def table(rows: list[list[str]]) -> str:
    return "\n".join([HEADER, *("| " + " | ".join(cells) + " |" for cells in rows)]) + "\n"


def stray_rows(root: Path = ROOT) -> list[tuple[int, str]]:
    """Table rows in docs/learnings.md (line number, line): each belongs in its own file."""
    path = root / INDEX
    lines = path.read_text(encoding="utf-8").splitlines() if path.exists() else []
    return [(n, line) for n, line in enumerate(lines, 1) if line.startswith("|") and not re.match(r"^\|\s*(Date\s*\||-)", line)]


def slug(change: str) -> str:
    """A short file slug from the Change text: a task id in parentheses, else an issue number and the first words."""
    task = re.search(r"\((?:tasks? |TASKS )?`?([a-z][a-z0-9]*(?:-[a-z0-9]+)+)`?[,) ]", change)
    if task:
        return task.group(1)
    words = [w for w in re.sub(r"[^a-z0-9]+", " ", re.sub(r"\(.*?\)", " ", change.lower())).split() if w not in STOP_WORDS]
    issue = re.search(r"#(\d+)", change)
    if issue:
        words = [issue.group(1), *(w for w in words if w != issue.group(1))]
    return "-".join(words[:6]) or "learning"


def unescaped(row: str) -> str:
    return row.strip().replace("\\|", "|")


def split(root: Path = ROOT) -> list[Path]:
    """Move each table row of docs/learnings.md into docs/learnings/<date>-<slug>.md; return the new files.

    A row that already has its file is only removed: a merge of a branch from before the split
    can bring every old row back, and each must not become a second copy.
    """
    path = root / INDEX
    rows = dict(stray_rows(root))
    known = {unescaped(row) for row in table(entries(root)[0]).splitlines()[2:]}
    made: list[Path] = []
    for line in rows.values():
        if unescaped(line) in known:
            continue
        known.add(unescaped(line))
        cells = [c.strip() for c in BARE_PIPE.split(line)[1:-1]]
        if len(cells) > 5:  # pipes inside a code span split the last cell; they belong to it
            cells = [*cells[:4], "|".join(cells[4:])]
        if len(cells) != 5:
            raise SystemExit(f"learnings: cannot split a row with {len(cells)} cells (needs 5): {line[:80]}")
        date, change, *texts = cells
        base = root / FOLDER / f"{date}-{slug(change)}"
        target = base.with_suffix(".md")
        n = 2
        while target.exists():
            target = base.with_name(f"{base.name}-{n}.md")
            n += 1
        target.parent.mkdir(parents=True, exist_ok=True)
        body = [f"# {change}", ""] + [part for title, text in zip(SECTIONS, texts, strict=True) for part in (f"## {title}", "", text, "")]
        target.write_text("\n".join(body[:-1]) + "\n", encoding="utf-8")
        made.append(target)
    if rows:  # the rows go, and with them the table's header lines
        kept = [line for line in path.read_text(encoding="utf-8").splitlines() if not line.startswith("|")]
        path.write_text("\n".join(kept).rstrip("\n") + "\n", encoding="utf-8")
    return made


def check(root: Path = ROOT) -> list[str]:
    rows, problems = entries(root)
    for n, line in stray_rows(root):
        problems.append(f"{INDEX}:{n}: a table row; each learning is its own file now. Run `uv run python scripts/learnings-table.py --split` ({line[:60]})")
    for n, line in enumerate(table(rows).splitlines()[2:], 1):
        if len(BARE_PIPE.split(line)) != 7:
            problems.append(f"learnings: row {n} does not have 5 cells: {line[:60]}")
    return problems


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="fail on a learning that breaks the format or a row in docs/learnings.md")
    mode.add_argument("--split", action="store_true", help="move table rows from docs/learnings.md into their own files")
    args = ap.parse_args(argv)
    if args.split:
        before = len(stray_rows())
        made = split()
        print(f"learnings: moved {len(made)} row(s) into {FOLDER}/, removed {before - len(made)} that already had a file")
        return 0
    if args.check:
        problems = check()
        rows, _ = entries()
        for p in problems:
            print(f"learnings: {p}")
        print(f"learnings: {len(rows)} learning file(s), {len(problems)} problem(s)")
        return 1 if problems else 0
    rows, problems = entries()
    for p in problems:
        print(f"learnings: skipped {p}", file=sys.stderr)
    sys.stdout.write(table(rows))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
