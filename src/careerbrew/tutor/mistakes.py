"""The mistake log: every mistake becomes a lesson, and a repeated mistake is caught.

A mistake has a kind (exception, timeout, edge case, wrong answer, complexity, style,
concept), the pattern it happened in (two pointers, trees, ...), and a prevention tip.
The first time a kind appears in a pattern, careerbrew adds a prevention card to the
`mistakes` deck, so spaced repetition keeps the fix fresh. When the same kind appears
again in the same pattern, the mistake is marked `repeat` and the teacher calls it out.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from .. import calendar, paths, store

TIPS = {
    "exception": "Before you run it, trace the code once with the smallest input; check every index and None access.",
    "timeout": "Estimate the complexity before coding. Nested loops over n are O(n^2): look for a hash map, two pointers, or sorting first.",
    "edge-case": "Write the edge cases first: empty input, one element, duplicates, negatives, and the largest values.",
    "wrong-answer": "Walk through a failing example by hand and compare each step with what your code does.",
    "complexity": "The answer is right but grows too fast: name the bottleneck loop and replace it with a better structure.",
    "style": "Readable code is part of the grade: clear names, no unused variables, simple conditions.",
    "concept": "Re-read the lesson and explain it back in one sentence before trying again.",
}
EDGE_WORDS = re.compile(r"empty|single|one element|duplicate|negative|zero|large|overflow|edge|null|none", re.I)


def _path() -> Path:
    return paths.sub("tutor", "mistakes.jsonl")


def load() -> list[dict[str, Any]]:
    return store.read_jsonl(_path())


def record(source: str, pattern: str, kind: str, detail: str, tip: str | None = None) -> dict[str, Any]:
    from . import cards

    seen = [m for m in load() if m["kind"] == kind and m["pattern"] == pattern]
    m = {
        "at": calendar.now_utc().isoformat(timespec="seconds"),
        "source": source,
        "pattern": pattern,
        "kind": kind,
        "detail": detail[:300],
        "tip": tip or TIPS.get(kind, TIPS["concept"]),
        "repeat": bool(seen),
    }
    store.append_jsonl(_path(), [m])
    if not seen:
        q = f"[{pattern}] You made a {kind.replace('-', ' ')} mistake ({detail[:120]}). What do you do to prevent it?"
        cards.add(q, m["tip"], ["mistakes", pattern], deck="mistakes")
    return m


def from_exercise(ex: dict[str, Any], outcome: dict[str, Any], grade: dict[str, Any]) -> list[dict[str, Any]]:
    src, pat = ex["id"], ex["pattern"]
    pitfalls = ex.get("pitfalls") or {}
    found = []
    if outcome.get("timeout"):
        found.append(record(src, pat, "timeout", outcome.get("error", "time limit"), pitfalls.get("timeout")))
    elif outcome.get("error"):
        found.append(record(src, pat, "exception", outcome["error"], pitfalls.get("exception")))
    else:
        failing = [t for t in outcome.get("results", []) if not t["ok"]]
        if failing:
            t = failing[0]
            note = ex["tests"][t["i"]].get("note", "")
            if t.get("error"):
                found.append(record(src, pat, "exception", f"{t['error']} (test {t['i'] + 1}{': ' + note if note else ''})", pitfalls.get("exception")))
            elif EDGE_WORDS.search(note):
                found.append(record(src, pat, "edge-case", f"failed on {note}", pitfalls.get("edge-case")))
            else:
                found.append(record(src, pat, "wrong-answer", f"test {t['i'] + 1}{': ' + note if note else ''}", pitfalls.get("wrong-answer")))
        elif grade.get("complexity") and not grade["complexity"]["ok"]:
            found.append(record(src, pat, "complexity", f"expected {grade['complexity']['expected']}", pitfalls.get("complexity") or pitfalls.get("timeout")))
    return found


def repeat_rate() -> float | None:
    ms = load()
    return round(sum(1 for m in ms if m["repeat"]) / len(ms), 3) if ms else None


def summary() -> dict[str, Any]:
    ms = load()
    by: dict[str, int] = {}
    for m in ms:
        key = f"{m['pattern']}: {m['kind']}"
        by[key] = by.get(key, 0) + 1
    return {"total": len(ms), "repeat_rate": repeat_rate(), "top": sorted(by.items(), key=lambda kv: -kv[1])[:8], "recent": ms[-10:][::-1]}
