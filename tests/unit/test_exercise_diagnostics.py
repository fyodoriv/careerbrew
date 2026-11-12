"""Exercise diagnostic families; process status fixtures are owner observations."""

import json
import subprocess

from careerbrew.tutor import exercises

CANARY = "CANARY-PERSON-7Q"
LINEAR = [{"n": 20000, "ms": 14.6}, {"n": 63246, "ms": 60.0}, {"n": 200000, "ms": 380.0}]
QUADRATIC = [{"n": 2000, "ms": 10.0}, {"n": 6325, "ms": 100.0}, {"n": 20000, "ms": 1000.0}]


def example():
    return {
        "id": "diagnostic-fixture",
        "order": 1,
        "pattern": "arrays and hashing",
        "difficulty": "easy",
        "title": "Synthetic",
        "statement": "Synthetic",
        "function": "solve",
        "signature": "solve(n)",
        "tests": [{"args": [1], "expected": 1}] * 4,
        "solution": "def solve(n):\n    return n\n",
        "hints": ["one", "two"],
        "lesson": "Synthetic",
        "scale": {"generator": "def make(n):\n    return [n]\n", "sizes": [2, 4], "expected": "O(n)"},
    }


def boundary(monkeypatch, output, returncode=0):
    def controlled_run(argv, **kwargs):
        payload = json.loads(kwargs["input"])
        if not payload["scale"]:
            return subprocess.CompletedProcess(argv, 0, json.dumps({"results": [{"i": i, "ok": True} for i in range(4)], "scale": []}), "")
        if isinstance(output, subprocess.TimeoutExpired):
            raise output
        return subprocess.CompletedProcess(argv, returncode, output, CANARY)

    monkeypatch.setattr(exercises.subprocess, "run", controlled_run)


def assert_failure(ex, category, exit_code=None, signal=None):
    out = exercises.run(ex, ex["solution"])
    diagnostic = out["diagnostic"]
    assert diagnostic == {
        "category": category,
        "stage": "scale",
        "exit_code": exit_code,
        "signal": signal,
        "wall_seconds": exercises.SCALE_WALL_SECONDS,
        "cpu_seconds": exercises.SCALE_CPU_SECONDS,
        "memory_bytes": exercises.MEMORY_BYTES,
    }
    assert not exercises._complexity(ex, out["scale"])["ok"]
    problems = exercises.validate(ex)
    assert len(problems) == 1 and category in problems[0]
    assert CANARY not in json.dumps(diagnostic) + "".join(problems)
    return out, problems


def test_wall_timeout_diagnostic(monkeypatch):
    boundary(monkeypatch, subprocess.TimeoutExpired([CANARY], exercises.SCALE_WALL_SECONDS, output=CANARY, stderr=CANARY))
    out, _ = assert_failure(example(), "wall_timeout")
    assert out["scale"][0]["error"] == "too slow"


def test_signal_exit_and_mapping_verdict_preservation(monkeypatch):
    for code in (-9, -24):
        boundary(monkeypatch, "", code)
        _, problems = assert_failure(example(), "signal_exit", signal=-code)
        assert "CPU time limit" not in problems[0] and "OOM" not in problems[0]
    for code in (-9, 17):
        success = {"results": [], "scale": LINEAR, "diagnostic": {"category": CANARY, "exit_code": 0}}
        boundary(monkeypatch, json.dumps(success), code)
        ex = example()
        out = exercises.run(ex, ex["solution"])
        assert exercises._complexity(ex, out["scale"])["ok"] and exercises.validate(ex) == []
        assert out["diagnostic"]["exit_code"] == (code if code >= 0 else None)
        assert out["diagnostic"]["signal"] == (-code if code < 0 else None)
        assert CANARY not in json.dumps(out["diagnostic"])
        boundary(monkeypatch, json.dumps({**success, "scale": [{"n": 2, "error": CANARY}]}), code)
        assert_failure(ex, "signal_exit" if code < 0 else "nonzero_exit", exit_code=code if code >= 0 else None, signal=-code if code < 0 else None)


def test_nonzero_silent_exit_diagnostic(monkeypatch):
    for code in (17, 152):
        boundary(monkeypatch, "", code)
        out, _ = assert_failure(example(), "nonzero_exit", exit_code=code)
        assert out["scale"][0]["reason"]  # Legacy learner errors remain available.


def test_malformed_result_diagnostic(monkeypatch):
    for output, code in ((CANARY, 0), ("", 0), ("[]", 0), ("null", 0), ("42", 0), (CANARY, -9)):
        boundary(monkeypatch, output, code)
        assert_failure(example(), "malformed_result", signal=-code if code < 0 else None, exit_code=code if code >= 0 else None)
    boundary(monkeypatch, json.dumps({"results": [], "scale": LINEAR, "diagnostic": {"category": CANARY}}))
    ex = example()
    assert "diagnostic" not in exercises.run(ex, ex["solution"])
    assert exercises.validate(ex) == []


def test_scale_make_and_target_error_diagnostic():
    for role in ("make", "target"):
        ex = example()
        if role == "make":
            ex["scale"]["generator"] = f"def make(n):\n    raise ValueError({CANARY!r})\n"
        else:
            ex["solution"] = f"def solve(n):\n    if n > 1: raise ValueError({CANARY!r})\n    return n\n"
        assert_failure(ex, "scale_error", exit_code=0)


def test_numeric_complexity_rejection_and_success_preservation(monkeypatch):
    boundary(monkeypatch, json.dumps({"results": [], "scale": QUADRATIC}))
    _, problems = assert_failure(example(), "complexity_rejected", exit_code=0)
    assert "growth n^2.0" in problems[0] and "O(n)" in problems[0]
    boundary(monkeypatch, json.dumps({"results": [], "scale": LINEAR}))
    ex = example()
    assert "diagnostic" not in exercises.run(ex, ex["solution"])
    assert exercises.validate(ex) == []
