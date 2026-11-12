"""Draft a product requirements document (PRD) for a project, and check it (issue #223).

`scaffold` turns a project record into the fixed headings, the plan slot, and the
reading list. `check` is the deterministic gate for the PRD rules in
`skills/careerbrew-project-explainer/SKILL.md`: requirements and decisions sections, estimates
in dev weeks, and a pushback when the plan slot is smaller than the estimate.
"""

from __future__ import annotations

import re
from typing import Any

from .explain import BULLET, CITE, _base, _sections, _table, source_problems, sources_table

HEADINGS = ["Problem", "Requirements", "Decisions", "Open questions", "Sources"]
OPTIONAL = "Pushback"  # required only when the slot is smaller than the estimate; goes after Decisions
WEEKS = re.compile(r"^(\d+(?:\.\d+)?)\s+dev weeks?$", re.I)
SLOT = re.compile(r"^Plan slot:\s*(.*?)\s*(?:\[S\d+\]\s*)*$", re.M)


def _weeks(n: float) -> str:
    return f"{n:g} dev week" + ("" if n == 1 else "s")


def check(text: str) -> list[str]:
    """Every problem that blocks shipping the PRD; an empty list means it passes."""
    problems: list[str] = []
    secs = _sections(text)
    got = [_base(h) for h, _ in secs]
    with_pushback = [*HEADINGS[:3], OPTIONAL, *HEADINGS[3:]]
    if got not in (HEADINGS, with_pushback):
        problems.append(f"headings must be exactly, in order: {', '.join(HEADINGS)}, with an optional {OPTIONAL} after Decisions (got: {', '.join(got) or 'none'})")
    intro = text.split("\n## ", 1)[0]
    slot: float | None = None
    m = SLOT.search(intro)
    if not m:
        problems.append("Plan slot: add a 'Plan slot: N dev weeks' line above the first section")
    elif not (w := WEEKS.match(m.group(1))):
        problems.append(f"Plan slot: give it in dev weeks, as 'N dev weeks' (got: {m.group(1) or 'empty'})")
    else:
        slot = float(w.group(1))
    src = sources_table(text)
    problems += source_problems(src)
    used: set[str] = set(CITE.findall(intro))
    total = 0.0
    pushback: list[str] | None = None
    for h, lines in secs:
        name = _base(h)
        if name == "Sources":
            continue
        bullets = [m.group(2) for ln in lines if (m := BULLET.match(ln))]
        head, rows = _table(lines)
        for c in bullets + [" | ".join(r) for r in rows]:
            ids = CITE.findall(c)
            used.update(ids)
            if not ids:
                problems.append(f"{name}: no citation on: {c[:60]}")
            elif any(i not in src for i in ids):
                problems.append(f"{name}: cites {', '.join(i for i in ids if i not in src)}, which is not in Sources")
            elif all(src[i].get("kind", "").lower() == "lead" for i in ids):
                problems.append(f"{name}: backed only by leads: {c[:60]}")
        cols = [x.lower() for x in head]
        if name == "Requirements":
            if not rows:
                problems.append("Requirements: a table with Id, Requirement, Estimate, and Source columns is required")
            elif "estimate" not in cols:
                problems.append("Requirements: the table needs an Estimate column")
            else:
                ei = cols.index("estimate")
                for r in rows:
                    est = r[ei] if ei < len(r) else ""
                    if w := WEEKS.match(est):
                        total += float(w.group(1))
                    else:
                        problems.append(f"Requirements {r[0]}: estimate in dev weeks, as 'N dev weeks' (got: {est or 'empty'})")
        if name == "Decisions":
            if not {"state", "who", "when"} <= set(cols):
                problems.append("Decisions: the table needs State, Who, and When columns")
            else:
                si = cols.index("state")
                for r in rows:
                    state = r[si].lower() if si < len(r) else ""
                    if state not in ("decided", "proposed"):
                        problems.append(f"Decisions: State must be decided or proposed (got: {state or 'empty'})")
                    elif "considering" in " ".join(r).lower() and state != "proposed":
                        problems.append("Decisions: 'considering' means proposed, not decided")
        if name == OPTIONAL:
            pushback = bullets
        if name == "Open questions":
            for b in bullets:
                if "who:" not in b.lower():
                    problems.append(f"Open questions: each question names who can answer (Who: ...): {b[:60]}")
    if slot is not None and total > slot:
        if not pushback:
            problems.append(f"Pushback: the requirements need {_weeks(total)} but the plan slot is {_weeks(slot)}; add a Pushback section that names the gap and what to cut or add")
        elif not any("dev week" in b.lower() for b in pushback):
            problems.append(f"Pushback: name the gap in dev weeks ({_weeks(total)} needed, {_weeks(slot)} in the slot)")
    for sid in src:
        if sid not in used:
            problems.append(f"Sources {sid}: unused (cite it or remove it)")
    return problems


def scaffold(name: str, slot: float | None = None) -> str:
    """The headings, the plan slot, and the reading list; it fails `check` until filled."""
    from . import projects

    d: dict[str, Any] = projects.show(name)
    lines = [f"# {name}: PRD", "", f"<!-- reading list from `careerbrew work project show {name}`: re-read each source now and record the date or sha -->", ""]
    lines += [f"Plan slot: {_weeks(slot) if slot is not None else 'TODO dev weeks'}", ""]
    lines += ["## Problem", "", "- TODO", ""]
    lines += ["## Requirements", "", "| Id | Requirement | Estimate | Source |", "|---|---|---|---|", "| R1 | TODO | TODO dev weeks | |", ""]
    lines += ["## Decisions", "", "| Decision | State | Who | When | Source |", "|---|---|---|---|---|", "| TODO | proposed | TODO | TODO | |", ""]
    lines += ["## Open questions", "", "- TODO Who: TODO", ""]
    lines += ["## Sources", "", "| Id | Kind | Locator | Read |", "|---|---|---|---|"]
    for i, s in enumerate(d["sources"], 1):
        lines.append(f"| S{i} | primary | {s['ref']} | TODO |")
    return "\n".join(lines) + "\n"
