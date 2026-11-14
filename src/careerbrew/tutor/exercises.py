"""Coding exercises: an ordered curriculum, a local runner, and time + quality measurement.

Exercises live in `knowledge/exercises/<NNN>-<id>.yaml` (public, original statements),
ordered by pattern the way the classic interview roadmaps teach them (arrays and hashing
first, dynamic programming late). Each holds a statement, a function signature, tests,
a reference solution (hidden until solved or the last hint), hints, a short lesson, and
optionally a scaling check that measures complexity.

`run(ex, code)` executes the learner's Python in a child process with CPU, memory, and
wall-clock limits, and returns per-test results. `grade()` turns that into a quality
score: correctness first, then complexity (does CPU time grow like the expected class?),
then readability (ruff findings). Attempts are stored privately with their timing.

Test values can wrap structures: {"list": [1, 2]} builds a linked list (ListNode),
{"tree": [1, null, 2]} builds a binary tree (TreeNode, level order); results are
converted back the same way. Design problems use {"ops": [...], "args": [...]} and
compare every method's return value.
"""

from __future__ import annotations

import copy
import json
import math
import os
import resource
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import yaml

from .. import calendar, events, paths, store

CPU_SECONDS = 6
MEMORY_BYTES = 768 * 1024 * 1024
WALL_SECONDS = 15
# The scaling run is judged in CPU time (CPU_SECONDS kills a slow one), so its wall-clock limit only stops a hang.
# At 15 s, a run with under 3 s of CPU waited past it on a busy machine and failed as too slow (2026-10-22).
# At 120 s the full gate cut off a run that takes 14.5 s wall alone, while it waited for a CPU (2026-10-29). An
# infinite loop burns CPU and dies at SCALE_CPU_SECONDS; this only stops a run that hangs without using CPU.
SCALE_WALL_SECONDS = 600
# The scaling run's 15 timed runs share one process, so they get their own CPU limit: word-search-ii's reference
# solution uses 7.3 s in all (about 1 s the largest run), past the one-run CPU_SECONDS (2026-10-25).
# copy-list-with-random-pointer's uses 13.7 s at load 4 and passed 20 s under the gate's 8 workers (2026-10-29).
SCALE_CPU_SECONDS = 60
REQUIRED = ("id", "order", "pattern", "difficulty", "title", "statement", "function", "signature", "tests", "solution", "hints", "lesson")
PATTERNS = (
    "arrays and hashing",
    "two pointers",
    "sliding window",
    "stack",
    "binary search",
    "linked list",
    "trees",
    "tries",
    "heap",
    "backtracking",
    "graphs",
    "advanced graphs",
    "1-d dynamic programming",
    "2-d dynamic programming",
    "greedy",
    "intervals",
    "math and geometry",
    "bit manipulation",
)
SCALE_REPEATS = 5  # timing runs per input size; the check uses the fastest (issue #30)
# Max growth exponent k in time ~ n^k, fitted on log-log over 3+ sizes. Each limit sits halfway to the next
# class up (O(n) is 1, O(n^2) is 2), so timing noise cannot fail a correct solution, but a wrong class still fails.
# Timing cannot tell O(n) from O(n log n); the check exists to catch a wrong class such as quadratic code.
COMPLEXITY_SLOPE = {"O(1)": 0.5, "O(log n)": 0.5, "O(n)": 1.5, "O(n log n)": 1.6, "O(n^2)": 2.5}

HARNESS = r"""
import json, sys, time, traceback
from collections import deque

class ListNode:
    def __init__(self, val=0, next=None):
        self.val, self.next = val, next

class TreeNode:
    def __init__(self, val=0, left=None, right=None):
        self.val, self.left, self.right = val, left, right

def build(v):
    if isinstance(v, dict) and set(v) == {"list"}:
        head = cur = ListNode()
        for x in v["list"]:
            cur.next = ListNode(x); cur = cur.next
        return head.next
    if isinstance(v, dict) and set(v) == {"tree"}:
        vals = v["tree"]
        if not vals or vals[0] is None:
            return None
        root = TreeNode(vals[0]); q = deque([root]); i = 1
        while q and i < len(vals):
            node = q.popleft()
            if i < len(vals) and vals[i] is not None:
                node.left = TreeNode(vals[i]); q.append(node.left)
            i += 1
            if i < len(vals) and vals[i] is not None:
                node.right = TreeNode(vals[i]); q.append(node.right)
            i += 1
        return root
    return v

def plain(r, like):
    if isinstance(like, dict) and set(like) == {"list"}:
        out, seen = [], 0
        while r is not None and seen < 100000:
            out.append(r.val); r = r.next; seen += 1
        return {"list": out}
    if isinstance(like, dict) and set(like) == {"tree"}:
        out, q = [], deque([r])
        while q:
            n = q.popleft()
            if n is None:
                out.append(None); continue
            out.append(n.val); q.append(n.left); q.append(n.right)
        while out and out[-1] is None:
            out.pop()
        return {"tree": out}
    if isinstance(r, tuple):
        return list(r)
    return r

def same(got, exp, mode):
    if mode == "sorted" and isinstance(got, list) and isinstance(exp, list):
        key = lambda x: json.dumps(x, sort_keys=True)  # outer order free, inner order exact
        return sorted(got, key=key) == sorted(exp, key=key)
    if mode == "set" and isinstance(got, list) and isinstance(exp, list):
        norm = lambda xs: sorted(json.dumps(sorted(x) if isinstance(x, list) else x, sort_keys=True) for x in xs)
        return norm(got) == norm(exp)
    if mode == "float":
        return abs(float(got) - float(exp)) < 1e-6
    return got == exp

payload = json.loads(sys.stdin.read())
ns = {"ListNode": ListNode, "TreeNode": TreeNode, "List": list, "Optional": None}
import typing
ns.update({k: getattr(typing, k) for k in ("List", "Optional", "Dict", "Tuple", "Set")})
results = []
try:
    exec(compile(payload["code"], "solution.py", "exec"), ns)
except Exception as e:
    print(json.dumps({"error": f"{type(e).__name__}: {e}", "line": getattr(e, "lineno", None)})); sys.exit(0)
target = ns.get(payload["function"])
if target is None:
    print(json.dumps({"error": f"define {payload['function']} (see the signature)"})); sys.exit(0)
for i, t in enumerate(payload["tests"]):
    t0 = time.perf_counter()
    try:
        if "ops" in t:
            obj, got = None, []
            for op, a in zip(t["ops"], t["args"]):
                if obj is None:
                    obj = target(*[build(x) for x in a]); got.append(None)
                else:
                    got.append(plain(getattr(obj, op)(*[build(x) for x in a]), None))
            ok = got == t["expected"]
        else:
            got = plain(target(*[build(a) for a in t["args"]]), t.get("expected"))
            ok = same(got, t["expected"], payload.get("compare", "exact"))
        results.append({"i": i, "ok": ok, "got": repr(got)[:300], "ms": round((time.perf_counter() - t0) * 1000, 2)})
    except Exception as e:
        tb = traceback.extract_tb(e.__traceback__)
        line = next((f.lineno for f in reversed(tb) if f.filename == "solution.py"), None)
        results.append({"i": i, "ok": False, "error": f"{type(e).__name__}: {e}"[:300], "line": line, "ms": round((time.perf_counter() - t0) * 1000, 2)})
scale = []
if payload.get("scale"):
    exec(payload["scale"]["generator"], ns)
    import gc
    for n in payload["scale"]["sizes"]:
        runs = []; k = 1
        try:
            while len(runs) < payload["scale"].get("repeats", 1):
                batch = [ns["make"](n) for _ in range(k)]  # fresh inputs: a solution may mutate one
                gc.collect(); gc.disable()
                # CPU time, not wall time (a busy machine's waits read as growth, 2026-10-22). A short run repeats k times: a CPU clock can tick in 1 ms steps.
                t0 = time.process_time()
                try:
                    for args in batch: target(*args)
                finally:
                    runs.append((time.process_time() - t0) * 1000 / k); gc.enable()
                if k == 1 and runs[-1] < 20: k = min(50, int(20 / max(runs[-1], 0.5)) + 1); runs.clear()
        except Exception as e:
            scale.append({"n": n, "error": type(e).__name__}); break
        # The minimum is the run least disturbed by other load (timeit's advice), so noise cannot fail a correct solution.
        scale.append({"n": n, "ms": min(runs), "runs": runs})
print(json.dumps({"results": results, "scale": scale}))
"""


def exercises_dir() -> Path:
    env = os.environ.get("CAREERBREW_EXERCISES")  # tests point it at fixtures
    return Path(env) if env else paths.REPO_ROOT / "knowledge" / "exercises"


# Parsed exercises, keyed by file path, size, and mtime. Scope: process (the server parses ~150 files
# several times per request otherwise). An edited file has a new key, so it is parsed again.
_PARSED: dict[tuple[str, int, int], dict[str, Any]] = {}


def _parse(f: Path) -> dict[str, Any]:
    st = f.stat()
    key = (str(f), st.st_size, st.st_mtime_ns)
    if key not in _PARSED:
        _PARSED[key] = yaml.safe_load(f.read_text(encoding="utf-8"))
    return copy.deepcopy(_PARSED[key])


def load() -> list[dict[str, Any]]:
    out = [_parse(f) for f in sorted(exercises_dir().glob("*.yaml"))] if exercises_dir().is_dir() else []
    return sorted(out, key=lambda e: e["order"])


def get(ex_id: str) -> dict[str, Any]:
    for e in load():
        if e["id"] == ex_id:
            return e
    raise KeyError(f"unknown exercise {ex_id}")


def _limits(cpu: int | None = None) -> None:  # runs in the child before exec
    cpu = cpu or CPU_SECONDS
    resource.setrlimit(resource.RLIMIT_CPU, (cpu, cpu))
    try:
        resource.setrlimit(resource.RLIMIT_AS, (MEMORY_BYTES, MEMORY_BYTES))
    except (ValueError, OSError):
        pass  # macOS ignores RLIMIT_AS; the wall-clock timeout still holds


def run(ex: dict[str, Any], code: str, with_scale: bool = True) -> dict[str, Any]:
    """Run code against every test, then (in a separate process, so a slow solution keeps its test results) the scaling check."""
    out = _child({"code": code, "function": ex["function"], "tests": ex["tests"], "compare": ex.get("compare", "exact"), "scale": None})
    passed = not out.get("error") and all(t["ok"] for t in out.get("results", []))
    if with_scale and ex.get("scale") and passed:
        sc = _child({"code": code, "function": ex["function"], "tests": [], "scale": {**ex["scale"], "sizes": _scale_sizes(ex["scale"]["sizes"]), "repeats": SCALE_REPEATS}}, SCALE_WALL_SECONDS, SCALE_CPU_SECONDS)
        # `reason` keeps the child's own error (CPU limit, wall limit, crash), so a gate failure says which (2026-10-29)
        out["scale"] = sc.get("scale") or [{"n": ex["scale"]["sizes"][-1], "error": "too slow", "reason": sc.get("error", "")}] * 2
        if "diagnostic" in sc:
            out["diagnostic"] = sc["diagnostic"]
    return out


def _child(payload: dict[str, Any], wall: float | None = None, cpu: int | None = None) -> dict[str, Any]:
    wall = wall or WALL_SECONDS

    def annotate(result: dict[str, Any], category: str | None, returncode: int | None = None) -> dict[str, Any]:
        # Invocation scope: only the subprocess owner supplies status and limits.
        result.pop("diagnostic", None)
        if category:
            result["diagnostic"] = {
                "category": category,
                "stage": "scale" if payload.get("scale") else "tests",
                "exit_code": returncode if returncode is not None and returncode >= 0 else None,
                "signal": -returncode if returncode is not None and returncode < 0 else None,
                "wall_seconds": wall,
                "cpu_seconds": cpu or CPU_SECONDS,
                "memory_bytes": MEMORY_BYTES,
            }
        return result

    with tempfile.TemporaryDirectory(prefix="cb-run-") as tmp:
        try:
            proc = subprocess.run(  # noqa: S603 (our harness, the learner's own code, local only)
                [sys.executable, "-I", "-c", HARNESS],
                input=json.dumps(payload),
                capture_output=True,
                text=True,
                timeout=wall,
                cwd=tmp,
                preexec_fn=lambda: _limits(cpu),
                env={"PATH": os.environ.get("PATH", ""), "PYTHONHASHSEED": "0"},
            )
        except subprocess.TimeoutExpired:
            return annotate({"error": f"time limit: the code ran longer than {wall:g} s (an infinite loop, or too slow)", "results": [], "scale": [], "timeout": True}, "wall_timeout")
    status_category = "signal_exit" if proc.returncode < 0 else "nonzero_exit" if proc.returncode else None
    if proc.returncode != 0 and not proc.stdout.strip():
        reason = "CPU time limit (too slow, or an infinite loop)" if proc.returncode in (-9, -24, 152) else (proc.stderr.strip().splitlines() or ["the run crashed"])[-1]
        return annotate({"error": reason[:300], "results": [], "scale": [], "timeout": proc.returncode in (-9, -24, 152)}, status_category, proc.returncode)
    try:
        result = json.loads(proc.stdout.strip().splitlines()[-1])
    except (json.JSONDecodeError, IndexError):
        result = None
    if not isinstance(result, dict):
        return annotate({"error": "the run printed something unexpected; remove print statements and try again", "results": [], "scale": []}, "malformed_result", proc.returncode)
    category = status_category
    if category is None and payload.get("scale"):
        scale = result.get("scale", [])
        cx = _complexity({"scale": payload["scale"]}, scale)
        if cx is not None and not cx["ok"]:
            category = "scale_error" if any("error" in row for row in scale) else "complexity_rejected"
    return annotate(result, category, proc.returncode)


def _complexity(ex: dict[str, Any], scale: list[dict[str, Any]]) -> dict[str, Any] | None:
    spec = ex.get("scale")
    if not spec or not scale:
        return None
    if any("error" in s for s in scale):
        return {"expected": spec["expected"], "slope": None, "ok": False}  # too slow to finish the larger input
    if len(scale) < 2:
        return None
    slope = growth_slope([(x["n"], x["ms"]) for x in scale])
    limit = COMPLEXITY_SLOPE.get(spec["expected"], 1.5)
    return {"expected": spec["expected"], "slope": round(slope, 2), "ok": slope <= limit or scale[-1]["ms"] < 50}


def growth_slope(points: list[tuple[float, float]]) -> float:
    """Least-squares slope of log(ms) against log(n): the k in time ~ n^k."""
    xs = [math.log(n) for n, _ in points]
    ys = [math.log(max(ms, 0.05)) for _, ms in points]
    mx, my = sum(xs) / len(xs), sum(ys) / len(ys)
    den = sum((x - mx) ** 2 for x in xs) or 1.0
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys, strict=True)) / den


def _scale_sizes(sizes: list[int]) -> list[int]:
    """Add the geometric middle size when only two are given, so the fit uses three points."""
    if len(sizes) == 2:
        return [sizes[0], round(math.sqrt(sizes[0] * sizes[1])), sizes[1]]
    return sizes


def _readability(code: str) -> list[str]:
    """ruff findings on the learner's code, as short codes (style is the last thing to fix)."""
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as fh:
        fh.write(code)
        name = fh.name
    try:
        proc = subprocess.run([sys.executable, "-m", "ruff", "check", "--select", "E,F,B,SIM,C4", "--ignore", "E501", "--output-format", "json", name], capture_output=True, text=True, timeout=20)  # noqa: S603
        return sorted({f["code"] for f in json.loads(proc.stdout or "[]")})
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return []
    finally:
        Path(name).unlink(missing_ok=True)


def grade(ex: dict[str, Any], code: str, outcome: dict[str, Any]) -> dict[str, Any]:
    tests = outcome.get("results", [])
    passed = sum(1 for t in tests if t["ok"])
    total = len(ex["tests"])
    correct = not outcome.get("error") and passed == total
    cx = _complexity(ex, outcome.get("scale", [])) if correct else None
    style = _readability(code) if correct else []
    score = 0.0
    if total:
        score = 0.7 * passed / total
    if correct:
        score += 0.2 if (cx is None or cx["ok"]) else 0.0
        score += 0.1 if not style else 0.05
    return {"passed": passed, "total": total, "correct": correct, "complexity": cx, "style": style, "score": round(score, 2)}


def _attempts_path() -> Path:
    return paths.sub("tutor", "exercise-attempts.jsonl")


def attempts(ex_id: str | None = None) -> list[dict[str, Any]]:
    rows = store.read_jsonl(_attempts_path())
    return [r for r in rows if ex_id is None or r["exercise"] == ex_id]


def submit(ex_id: str, code: str, seconds: int | None = None, hints_used: int = 0) -> dict[str, Any]:
    """Run, grade, store the attempt, and log mistakes. Returns everything the teacher needs."""
    from . import mistakes

    ex = get(ex_id)
    outcome = run(ex, code)
    g = grade(ex, code, outcome)
    first = not attempts(ex_id)
    row = {
        "exercise": ex_id,
        "at": calendar.now_utc().isoformat(timespec="seconds"),
        "seconds": seconds,
        "hints": hints_used,
        "correct": g["correct"],
        "score": g["score"],
        "passed": g["passed"],
        "total": g["total"],
        "first": first,
        "complexity_ok": None if g["complexity"] is None else g["complexity"]["ok"],
        "code": code,
    }
    store.append_jsonl(_attempts_path(), [row])
    found = mistakes.from_exercise(ex, outcome, g)
    events.emit("exercise.submit", status="pass" if g["correct"] else "fail", seconds=seconds or 0)
    failing = [t for t in outcome.get("results", []) if not t["ok"]][:3]
    shown = [{**t, "args": ex["tests"][t["i"]].get("args", ex["tests"][t["i"]].get("ops")), "expected": ex["tests"][t["i"]]["expected"]} for t in failing]
    return {"grade": g, "error": outcome.get("error"), "failing": shown, "mistakes": found, "solution": ex["solution"] if g["correct"] else None}


def status() -> dict[str, dict[str, Any]]:
    """Per exercise: solved, best score, attempts, best time, and whether it was synced from an account."""
    out: dict[str, dict[str, Any]] = {}
    synced = set((store.read_json(paths.sub("tutor", "synced-solved.json"), {}) or {}).get("exercises", []))
    for r in attempts():
        s = out.setdefault(r["exercise"], {"attempts": 0, "solved": False, "best": 0.0, "best_seconds": None})
        s["attempts"] += 1
        s["best"] = max(s["best"], r["score"])
        if r["correct"]:
            s["solved"] = True
            if r.get("seconds") and (s["best_seconds"] is None or r["seconds"] < s["best_seconds"]):
                s["best_seconds"] = r["seconds"]
    for ex_id in synced:
        out.setdefault(ex_id, {"attempts": 0, "solved": False, "best": 0.0, "best_seconds": None})["synced"] = True
    return out


def next_exercise(focus_patterns: list[str] | None = None, skip: set[str] | None = None) -> dict[str, Any] | None:
    """The first unsolved exercise in curriculum order, preferring the upcoming goal's patterns. `skip`: ids that
    wait to come back later (the teacher's retry list, issue #185)."""
    st = status()
    todo = [e for e in load() if e["id"] not in (skip or set()) and not (st.get(e["id"], {}).get("solved") or st.get(e["id"], {}).get("synced"))]
    if focus_patterns:
        focused = [e for e in todo if e["pattern"] in focus_patterns]
        todo = focused or todo
    return todo[0] if todo else None


def validate(ex: dict[str, Any]) -> list[str]:
    """Content check for the gate: fields, order, and the reference solution passes every test and the scaling check."""
    problems = [f"missing {k}" for k in REQUIRED if k not in ex]
    if problems:
        return problems
    if ex["pattern"] not in PATTERNS:
        problems.append(f"unknown pattern {ex['pattern']!r}")
    if ex["difficulty"] not in ("easy", "medium", "hard"):
        problems.append("difficulty must be easy, medium, or hard")
    if len(ex["tests"]) < 4:
        problems.append("needs at least 4 tests (include edge cases)")
    if len(ex["hints"]) < 2:
        problems.append("needs at least 2 hints")
    outcome = run(ex, ex["solution"])
    if outcome.get("error"):
        detail = json.dumps(outcome["diagnostic"], sort_keys=True) if "diagnostic" in outcome else outcome["error"]
        problems.append(f"reference solution fails to run: {detail}")
    else:
        bad = [t["i"] for t in outcome["results"] if not t["ok"]]
        if bad:
            problems.append(f"reference solution fails tests {bad}")
        cx = _complexity(ex, outcome.get("scale", []))
        if cx is not None and not cx["ok"]:
            diagnostic = outcome.get("diagnostic")
            if diagnostic and diagnostic["category"] != "complexity_rejected":
                problems.append(f"reference solution scaling failed: {json.dumps(diagnostic, sort_keys=True)}")
            else:
                suffix = " [complexity_rejected]" if diagnostic else ""
                problems.append(f"reference solution misses its own complexity ({cx['expected']}, growth n^{cx.get('slope')}){suffix}")
    return problems


def check_all() -> list[str]:
    """Validate every exercise on each check, including its runtime-dependent behavior."""
    problems: list[str] = []
    seen_ids, seen_orders = set(), set()
    for ex in load():
        if ex.get("id") in seen_ids:
            problems.append(f"{ex.get('id')}: duplicate id")
        if ex.get("order") in seen_orders:
            problems.append(f"{ex.get('id')}: duplicate order {ex.get('order')}")
        seen_ids.add(ex.get("id"))
        seen_orders.add(ex.get("order"))
        problems += [f"{ex.get('id')}: {p}" for p in validate(ex)]
    return problems


def _synced_path() -> Path:
    return paths.sub("tutor", "synced-solved.json")


def mark_solved(slugs_or_ids: list[str], source: str = "manual") -> dict[str, Any]:
    """Mark exercises solved elsewhere (by LeetCode slug or exercise id) so the teacher skips them."""
    by_slug = {e.get("leetcode"): e["id"] for e in load() if e.get("leetcode")} | {e["id"]: e["id"] for e in load()}
    data = store.read_json(_synced_path(), {}) or {}
    have = set(data.get("exercises", []))
    matched = {by_slug[s] for s in slugs_or_ids if s in by_slug}
    data = {"exercises": sorted(have | matched), "source": source, "at": calendar.now_utc().isoformat(timespec="seconds")}
    store.write_json(_synced_path(), data)
    return {"matched": len(matched), "new": len(matched - have), "total": len(data["exercises"]), "unknown": len([s for s in slugs_or_ids if s not in by_slug])}


LEETCODE_GRAPHQL = "https://leetcode.com/graphql"
LEETCODE_QUERY = "query recentAc($username: String!, $limit: Int!) { recentAcSubmissionList(username: $username, limit: $limit) { titleSlug } }"


def sync_leetcode(username: str) -> dict[str, Any]:
    """Opt-in, read-only: the problems a PUBLIC LeetCode profile recently solved (no login, no cookies).

    LeetCode publishes only the most recent ~20 accepted submissions per profile, so run it
    now and then; matches accumulate. Uses the same host policy and fixtures as every connector.
    """
    from .. import connectors

    if not username or not username.replace("-", "").replace("_", "").isalnum():
        raise ValueError("give your public LeetCode username")
    resp = connectors.http_json(LEETCODE_GRAPHQL, {"Referer": "https://leetcode.com", "User-Agent": f"careerbrew/0.2 (personal study sync; {connectors.CONTACT})"}, {"query": LEETCODE_QUERY, "variables": {"username": username, "limit": 20}})
    rows = ((resp or {}).get("data") or {}).get("recentAcSubmissionList") or []
    slugs = sorted({r["titleSlug"] for r in rows if r.get("titleSlug")} | set(_leetcode_all_solved()))
    out = mark_solved(slugs, source="leetcode")
    events.emit("exercise.sync", status="ok", items=out["matched"])
    return {**out, "seen": len(slugs)}


LEETCODE_ALL_QUERY = 'query solved($skip: Int!) { questionList(categorySlug: "", limit: 500, skip: $skip, filters: {status: AC}) { totalNum data { titleSlug } } }'


def _leetcode_all_solved() -> list[str]:
    """Every solved problem, only when the owner opted in with their own session cookie in `.env`.

    LeetCode publishes per-problem history only to the logged-in user. `LEETCODE_SESSION`
    (and `LEETCODE_CSRF`) stay in the local `.env`; they are sent only to leetcode.com.
    """
    import os

    from .. import connectors

    session = os.environ.get("LEETCODE_SESSION", "").strip()
    if not session:
        return []
    csrf = os.environ.get("LEETCODE_CSRF", "").strip()
    headers = {"Referer": "https://leetcode.com/problemset/", "User-Agent": "careerbrew/0.2 (personal study sync)", "Cookie": f"LEETCODE_SESSION={session}" + (f"; csrftoken={csrf}" if csrf else "")}
    if csrf:
        headers["x-csrftoken"] = csrf
    out: list[str] = []
    for skip in range(0, 5000, 500):
        page = ((connectors.http_json(LEETCODE_GRAPHQL, headers, {"query": LEETCODE_ALL_QUERY, "variables": {"skip": skip}}) or {}).get("data") or {}).get("questionList") or {}
        out += [q["titleSlug"] for q in page.get("data") or [] if q.get("titleSlug")]
        if skip + 500 >= int(page.get("totalNum") or 0):
            break
    return out


def leetcode_profile_solved(username: str) -> int | None:
    """Total accepted problems on a public profile, to explain an empty recent list (None if unknown)."""
    from .. import connectors

    q = "query($u:String!){matchedUser(username:$u){submitStatsGlobal{acSubmissionNum{difficulty count}}}}"
    try:
        resp = connectors.http_json(LEETCODE_GRAPHQL, {"Referer": "https://leetcode.com", "User-Agent": f"careerbrew/0.2 (personal study sync; {connectors.CONTACT})"}, {"query": q, "variables": {"u": username}})
    except (OSError, ValueError):
        return None
    user = ((resp or {}).get("data") or {}).get("matchedUser") or {}
    rows = (user.get("submitStatsGlobal") or {}).get("acSubmissionNum") or []
    return next((int(r["count"]) for r in rows if r.get("difficulty") == "All"), None)
