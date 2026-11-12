"""Earliest-start stage views and live readiness for the work-day runner.

Stages describe a possible start order, not barriers. Effective dependencies keep
explicit after links, startup, unknown-scope serial safety, sign-in delays and
finite work that must release a long-running step's resources. ready() selects
one safe batch from invocation-supplied daily rows. These functions own no state.
"""

from __future__ import annotations

import heapq
from typing import Any

from .steps import StepError

ALONE = "declares no scope or uses: runs alone, in its declared place"
LONG = "long-running: starts when its own prerequisites and resources are ready"
SIGNIN = "needs sign-in, and today's sign-in probe failed"


def _inside(a: str, b: str) -> bool:
    a, b = a.rstrip("/") or "/", b.rstrip("/") or "/"
    return a == b or a.startswith(b + "/") or b.startswith(a + "/")


def clash(s: dict[str, Any], o: dict[str, Any]) -> str:
    """Why `s` and `o` may not share a stage, or "" when they may."""
    shared = [u for u in s["uses"] if u in o["uses"]]
    if shared:
        return f"shares {shared[0]} with {o['step']}"
    for a in s["scope"]:
        for b in o["scope"]:
            if _inside(a, b):
                return f"scope {a} overlaps {o['step']}'s {b}"
    return ""


def _alone(s: dict[str, Any]) -> bool:
    return not s["scope"] and not s["uses"]


def _order(group: list[dict[str, Any]], index: dict[str, int]) -> list[dict[str, Any]]:
    """Declared order, moved only as far as `after` needs (Kahn's algorithm, ties by declared place)."""
    names = {s["step"] for s in group}
    deps = {s["step"]: [a for a in s["after"] if a in names] for s in group}
    waiting = {n: len(d) for n, d in deps.items()}
    ready = [(index[n], n) for n, w in waiting.items() if w == 0]
    heapq.heapify(ready)
    by = {s["step"]: s for s in group}
    out: list[dict[str, Any]] = []
    while ready:
        _, n = heapq.heappop(ready)
        out.append(by[n])
        for m, d in deps.items():
            if n in d:
                waiting[m] -= 1
                if waiting[m] == 0:
                    heapq.heappush(ready, (index[m], m))
    if len(out) < len(group):
        loop = sorted(n for n, w in waiting.items() if w)
        raise StepError(f"work steps {', '.join(loop)} wait for each other through `after`: drop one `after` entry")
    return out


def _late_closure(group: list[dict[str, Any]], late: set[str]) -> set[str]:
    """The late steps plus every step that runs `after` one of them (it cannot start before them)."""
    out = {s["step"] for s in group if s["step"] in late}
    grew = True
    while grew:
        grew = False
        for s in group:
            if s["step"] not in out and any(a in out for a in s["after"]):
                out.add(s["step"])
                grew = True
    return out


def _dependencies(declared: list[dict[str, Any]], late: set[str]) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Effective dependencies, including explicit safety barriers; no status/file reads."""
    by = {s["step"]: s for s in declared}
    deps = {n: [a for a in s["after"] if a in by] for n, s in by.items()}
    why: dict[str, list[str]] = {n: [f"after {a}: not in today's plan, ignored" for a in s["after"] if a not in by] for n, s in by.items()}
    operators = [n for n, s in by.items() if s.get("operator_turn")]
    longs = [s for s in declared if s["long_running"]]
    for s in declared:
        for a in deps[s["step"]]:
            if by[a]["long_running"]:
                raise StepError(f"work step {s['step']} runs after {a}, which is long-running and never ends: drop that `after` entry")
            if s.get("operator_turn") and a != "/careerbrew" and a not in operators:
                raise StepError(f"operatorTurn step {s['step']} waits for background step {a}; remove that after entry or mark its quick dispatcher operatorTurn: true")
    for i, s in enumerate(longs):
        for o in longs[:i]:
            if _alone(s) or _alone(o) or clash(s, o):
                raise StepError(f"long-running work steps {s['step']} and {o['step']} share an unknown or named resource; each never ends: declare separate scope/uses")

    def add(name: str, dependency: str, reason: str) -> None:
        if name != dependency and dependency not in deps[name]:
            deps[name].append(dependency)
            why[name].append(reason)

    finite = _order([s for s in declared if not s["long_running"] and s["step"] not in operators], {s["step"]: i for i, s in enumerate(declared)})
    waits = _late_closure(declared, late)
    finite = [s for s in finite if s["step"] not in waits] + [s for s in finite if s["step"] in waits]
    for i, s in enumerate(finite):
        for o in finite[:i]:
            if _alone(s) or _alone(o):
                add(s["step"], o["step"], f"after {o['step']}, which runs alone" if _alone(o) else ALONE)
    for s in declared:
        n = s["step"]
        if "/careerbrew" in by and n != "/careerbrew":
            add(n, "/careerbrew", "after /careerbrew starts the day")
        if n not in operators and n != "/careerbrew":
            for operator in operators:
                if operator in waits:
                    continue  # unavailable initial-turn dispatch is recorded blocked, not run after background waits
                add(n, operator, f"after operator-turn dispatcher {operator}")
        if s["long_running"]:
            for o in finite:
                if _alone(s) or _alone(o) or clash(s, o):
                    add(n, o["step"], f"after {o['step']}: finite resource work must finish before this long-running step")
    for s in declared:
        if s["step"] in waits:
            if s.get("operator_turn"):
                why[s["step"]].append(SIGNIN + ": record this initial-turn dispatcher blocked")
                continue
            for o in finite:
                if o["step"] not in waits:
                    add(s["step"], o["step"], SIGNIN)
    return deps, why


def build(declared: list[dict[str, Any]], late: set[str] | None = None) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """Earliest-start stage view plus the actual readiness/resource contract per step."""
    index = {s["step"]: n for n, s in enumerate(declared)}
    deps, why = _dependencies(declared, late or set())
    signin_waits = _late_closure(declared, late or set())
    ordered = _order([{**s, "after": deps[s["step"]]} for s in declared], index)
    groups: list[list[dict[str, Any]]] = []
    stage_of: dict[str, int] = {}
    for s in ordered:
        name = s["step"]
        st = max((stage_of[a] + 1 for a in deps[name]), default=1)
        for a in s["after"]:
            alone_reason = f"after {a}, which runs alone"
            if alone_reason in why[name]:
                why[name][why[name].index(alone_reason)] += f" (stage {stage_of[a]})"
            else:
                why[name].append(f"after {a} (stage {stage_of[a]})")
        while st <= len(groups):
            hit = next((c for o in groups[st - 1] if (c := clash(s, o))), "")
            if not hit and not _alone(s) and not any(_alone(o) for o in groups[st - 1]):
                break
            if hit:
                why[name].append(f"{hit} (stage {st})")
            st += 1
        while len(groups) < st:
            groups.append([])
        groups[st - 1].append(s)
        stage_of[name] = st
        if _alone(s):
            why[name].append(ALONE)
        if s["long_running"]:
            why[name].append(LONG)
    found = [
        {
            **s,
            "stage": stage_of[s["step"]],
            "why": "; ".join(why[s["step"]]) or "no unmet prerequisites",
            "ready_when": {"after": deps[s["step"]], "scope_free": s["scope"], "resources_free": s["uses"], "runs_alone": _alone(s), "signin_required": bool(s.get("operator_turn") and s["step"] in signin_waits)},
        }
        for s in declared
    ]
    view = [{"stage": n, "steps": [s["step"] for s in group], "long_running": bool(group) and all(s["long_running"] for s in group)} for n, group in enumerate(groups, 1)]
    return found, view


def ready(declared: list[dict[str, Any]], rows: list[dict[str, Any]]) -> list[str]:
    """One safe dispatch batch from the current day's status snapshot; writes nothing."""
    states = {r["step"]: r["status"] for r in rows}
    running = [s for s in declared if states.get(s["step"]) == "running"]
    selected: list[dict[str, Any]] = []
    for s in declared:
        if states.get(s["step"], "waiting") != "waiting":
            continue
        if s["ready_when"].get("signin_required"):
            continue
        if any(states.get(a) not in ("done", "blocked") for a in s["ready_when"]["after"]):
            continue
        if any(_alone(s) or _alone(o) or clash(s, o) for o in running + selected):
            continue
        selected.append(s)
    return [s["step"] for s in selected]
