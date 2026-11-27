"""rust-22 Python oracle for the exercise runner core.

It drives the real `careerbrew.tutor.exercises` code on the fixed synthetic
cases in `tests/fixtures/rust22/cases.json`: scaling sizes, the growth slope,
the complexity decision, the harness comparison modes, grading, the child
process lifecycle (CPU, wall and memory limits, crashes, malformed output,
sandbox environment), the tests-then-scaling run and reference validation.

The child program is the synthetic language runner
`exercise-fixture-runner` (a test fixture, not learner code). The oracle
points `exercises.sys.executable` at it for the duration of one call, so
the real `_child` lifecycle runs unchanged. Live timings are reduced to
decision level (`<timed>`); pure decision vectors keep exact numbers.
No owner data, network, model call or production route is used.
"""

from __future__ import annotations

import contextlib
import json
import os
import re
import sys
import types
from pathlib import Path
from typing import Any

from careerbrew.tutor import exercises

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tests" / "fixtures" / "rust22" / "cases.json"
CASES = (
    "decisions",
    "comparison-modes",
    "grading",
    "child-lifecycle",
    "scaling-runs",
    "reference-validation",
)
SLOPE_TEXT = re.compile(r"growth n\^-?[0-9.e+-]+")


def _runner() -> str:
    path = os.environ.get("CAREERBREW_RUST22_RUNNER") or str(ROOT / "target" / "release" / "exercise-fixture-runner")
    if not Path(path).is_file():
        raise FileNotFoundError("the synthetic exercise runner is not built")
    return path


@contextlib.contextmanager
def _language():
    """Point the real `_child` at the synthetic language runner for one call."""
    original = exercises.sys
    exercises.sys = types.SimpleNamespace(executable=_runner())
    try:
        yield
    finally:
        exercises.sys = original


def _harness_same():
    """The real `same` comparison from the learner harness, without its run section."""
    prefix = exercises.HARNESS.split("payload = json.loads(sys.stdin.read())")[0]
    ns: dict[str, Any] = {}
    exec(compile(prefix, "harness.py", "exec"), ns)  # noqa: S102 (the repo's own harness text)
    return ns["same"]


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str | int):
        return value
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    return {"type": type(value).__name__}


def _timed(value: Any) -> Any:
    """Decision-level view of a live run: timings and sample counts only, never numbers."""
    if isinstance(value, list):
        return [_timed(item) for item in value]
    if isinstance(value, dict):
        out = {}
        for key, item in value.items():
            if key == "ms":
                out[key] = "<timed>"
            elif key == "runs" and isinstance(item, list):
                out[key] = len(item)
            elif key == "slope" and item is not None:
                out[key] = "<timed>"
            else:
                out[key] = _timed(item)
        return out
    if isinstance(value, str):
        return SLOPE_TEXT.sub("growth n^<timed>", value)
    return value


def _error(exc: BaseException) -> dict[str, str]:
    return {"error": type(exc).__name__}


def _decisions(data: dict[str, Any]) -> dict[str, Any]:
    return {
        "limits": {
            "cpu_seconds": exercises.CPU_SECONDS,
            "memory_bytes": exercises.MEMORY_BYTES,
            "wall_seconds": exercises.WALL_SECONDS,
            "scale_wall_seconds": exercises.SCALE_WALL_SECONDS,
            "scale_cpu_seconds": exercises.SCALE_CPU_SECONDS,
            "scale_repeats": exercises.SCALE_REPEATS,
            "complexity_slope": exercises.COMPLEXITY_SLOPE,
        },
        "scale_sizes": [exercises._scale_sizes(list(sizes)) for sizes in data["scale_sizes"]],
        "slopes": [exercises.growth_slope([(n, ms) for n, ms in points]) for points in data["slopes"]],
        "complexity": [exercises._complexity({"scale": row["spec"]}, row["rows"]) for row in data["complexity"]],
    }


def _comparisons(data: dict[str, Any]) -> list[Any]:
    same = _harness_same()
    out = []
    for row in data["compare"]:
        try:
            out.append(same(row["got"], row["exp"], row["mode"]))
        except Exception as exc:
            out.append(_error(exc))
    return out


def _grading(data: dict[str, Any]) -> list[Any]:
    original = exercises._readability
    out = []
    try:
        for row in data["grade"]:
            exercises._readability = lambda _code, style=row["style"]: list(style)
            ex = {"tests": [{}] * row["tests"], "scale": row["scale"]}
            out.append(exercises.grade(ex, "synthetic", row["outcome"]))
    finally:
        exercises._readability = original
    return out


def _payload(ex: dict[str, Any], code: str) -> dict[str, Any]:
    return {"code": code, "function": ex["function"], "tests": ex["tests"], "compare": ex.get("compare", "exact"), "scale": None}


def _child_lifecycle(data: dict[str, Any]) -> list[Any]:
    ex = data["exercises"]["linear"]
    out = []
    with _language():
        for row in data["child"]:
            result = exercises._child(_payload(ex, row["code"]), row.get("wall"), row.get("cpu"))
            out.append({"label": row["label"], "result": _timed(result)})
    return out


def _runs(data: dict[str, Any]) -> list[Any]:
    out = []
    with _language():
        for row in data["runs"]:
            ex = data["exercises"][row["exercise"]]
            result = exercises.run(ex, row["code"], row.get("with_scale", True))
            out.append({"exercise": row["exercise"], "code": row["code"], "result": _timed(result)})
    return out


def _validation(data: dict[str, Any]) -> list[Any]:
    out = []
    with _language():
        for name in data["validate"]:
            out.append({"exercise": name, "problems": _timed(exercises.validate(json.loads(json.dumps(data["exercises"][name]))))})
    return out


def run_all(selected: str | None = None) -> str:
    data = json.loads(FIXTURE.read_text(encoding="utf-8"))
    functions = {
        "decisions": _decisions,
        "comparison-modes": _comparisons,
        "grading": _grading,
        "child-lifecycle": _child_lifecycle,
        "scaling-runs": _runs,
        "reference-validation": _validation,
    }
    cases = []
    for name in CASES:
        if selected is None or name == selected:
            try:
                value = functions[name](data)
            except Exception as exc:
                value = _error(exc)
            cases.append({"id": name, "result": _typed(value)})
    return json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":")) + "\n"


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
