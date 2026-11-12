"""One clock: src/careerbrew reads the time through `calendar.now()` (or `calendar.now_utc()`), which CAREERBREW_NOW pins.

A direct `datetime.now()` or `date.today()` counts the wrong day under a pinned clock, so pinned-clock tests and the
visual pictures stop meaning what they say (task pinned-clock-sweep). A few reads need real elapsed time or a real
write order; each is listed in ALLOWED with its reason. A new read fails here: use `calendar.now()` or
`calendar.now_utc()`, or, when it really needs the wall clock, add an entry with a reason and a comment at the call.
"""

from __future__ import annotations

import ast
from collections import Counter
from pathlib import Path

SRC = Path(__file__).resolve().parents[2] / "src" / "careerbrew"
READS = {"now", "utcnow", "today"}

# "<file>::<function>": (reads, reason). None of these is a time a view shows or a metric counts by.
ALLOWED: dict[str, tuple[int, str]] = {
    "calendar.py::now": (1, "the clock owner: it reads the wall clock when CAREERBREW_NOW is not set"),
    "events.py::emit": (1, "content-free event log: `ts` records when the code really ran"),
    "day/watch.py::run_cycle": (4, "the watcher's own throttles (MIN_GAP_S per source) and cycle stats measure real elapsed time"),
    "day/watch.py::loop": (2, "the watcher process's own start and reload times"),
    "day/watch.py::pause": (1, "the watcher process's own pause time"),
    "teacher.py::_step": (1, "the step timer starts: a pinned clock never moves, so every step would take 0 s"),
    "teacher.py::complete": (2, "the step timer stops: the real seconds the owner spent on the step"),
    "backup.py::create": (1, "backup file names sort by real write time, and retention keeps the newest by that order"),
}


def wall_clock_reads(source: str) -> list[tuple[str, int]]:
    """(enclosing function, line) of each `datetime.now/utcnow/today()` and `date.today()` call, under any import alias."""
    tree = ast.parse(source)
    classes: set[str] = set()  # names bound to datetime.datetime or datetime.date
    modules: set[str] = set()  # names bound to the datetime module
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module == "datetime":
            classes |= {a.asname or a.name for a in node.names if a.name in ("datetime", "date")}
        elif isinstance(node, ast.Import):
            modules |= {a.asname or a.name for a in node.names if a.name == "datetime"}

    def is_clock(value: ast.expr) -> bool:
        if isinstance(value, ast.Name):
            return value.id in classes
        return isinstance(value, ast.Attribute) and value.attr in ("datetime", "date") and isinstance(value.value, ast.Name) and value.value.id in modules

    found: list[tuple[str, int]] = []

    def visit(node: ast.AST, scope: str) -> None:
        for child in ast.iter_child_nodes(node):
            inner = f"{scope}.{child.name}".lstrip(".") if isinstance(child, ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef) else scope
            if isinstance(child, ast.Call) and isinstance(child.func, ast.Attribute) and child.func.attr in READS and is_clock(child.func.value):
                found.append((scope or "<module>", child.lineno))
            visit(child, inner)

    visit(tree, "")
    return found


def _scan() -> dict[str, list[int]]:
    out: dict[str, list[int]] = {}
    for path in sorted(SRC.rglob("*.py")):
        rel = path.relative_to(SRC).as_posix()
        for scope, line in wall_clock_reads(path.read_text(encoding="utf-8")):
            out.setdefault(f"{rel}::{scope}", []).append(line)
    return out


def test_no_wall_clock_read_outside_the_allow_list():
    found = _scan()
    new = [f"src/careerbrew/{key.split('::')[0]}:{line} ({key.split('::')[1]})" for key, lines in found.items() for line in lines if key not in ALLOWED]
    assert not new, "read the pinned clock here: calendar.now() or calendar.now_utc(), not the wall clock:\n" + "\n".join(new)
    counts = {key: len(lines) for key, lines in found.items()}
    changed = {key: (counts.get(key, 0), n) for key, (n, _) in ALLOWED.items() if counts.get(key, 0) != n}
    assert not changed, f"an allowed function's wall-clock reads changed (found, listed): {changed}; move a new read to calendar.now(), or update ALLOWED"


def test_every_allowed_read_names_its_reason():
    assert all(n > 0 and len(reason.split()) >= 5 for n, reason in ALLOWED.values())


def test_the_scan_finds_every_spelling_and_ignores_the_pinned_clock():
    source = """
import datetime
import datetime as dtm
from datetime import date, datetime as dt
from . import calendar
from .day import results

def a():
    return dt.now(), dt.utcnow(), date.today(), datetime.date.today(), dtm.datetime.now()

class B:
    def c(self):
        return dt.today()

def ok():
    return calendar.now(), calendar.now_utc(), results.today(), dt.fromisoformat("2026-01-05")
"""
    found = wall_clock_reads(source)
    assert Counter(scope for scope, _ in found) == {"a": 5, "B.c": 1}
