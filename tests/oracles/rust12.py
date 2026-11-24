"""rust-12 Python oracle for learning, replay, tours and explainers.

All inputs are synthetic and invocation-owned. Fixed clocks and Git identities
make the typed JSON deterministic. No production route, owner data, network,
model call or learned-repository write is used.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import re
import subprocess
import sys
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from careerbrew import cli, paths
from careerbrew.learn import project as lp
from careerbrew.learn import replay, tour
from careerbrew.tutor import cards
from careerbrew.work import explain, prd, projects

CASES = (
    "source-staging",
    "concept-validation",
    "learning-schedule",
    "replay",
    "tours",
    "explainers-cli",
)
T0 = datetime(2026, 10, 26, 9, 0, tzinfo=UTC)
GIT = [
    "git",
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "commit.gpgsign=false",
    "-c",
    "user.name=Synthetic",
    "-c",
    "user.email=synthetic@example.com",
]


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, set):
        return _typed(sorted(value))
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    if isinstance(value, Path):
        return str(value)
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


def _invoke(argv: list[str]) -> dict[str, Any]:
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
        try:
            code = cli.main(argv)
        except Exception as exc:
            code = 99
            print(type(exc).__name__)
    return {"exit": code, "stdout": out.getvalue()}


def _json_invoke(argv: list[str]) -> dict[str, Any]:
    result = _invoke(argv)
    return {"exit": result["exit"], "json": json.loads(result["stdout"])}


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.root.mkdir(parents=True)
        (self.base / "home").mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "off",
                "CAREERBREW_LLM": "off",
                "CAREERBREW_NOW": "2026-10-26T09:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
                "GIT_AUTHOR_DATE": "2026-10-26T09:00:00+00:00",
                "GIT_COMMITTER_DATE": "2026-10-26T09:00:00+00:00",
            }
        )
        _invoke(["init"])


def _at(value: datetime | str) -> None:
    os.environ["CAREERBREW_NOW"] = value.isoformat() if isinstance(value, datetime) else value


def _git(repo: Path, *args: str) -> str:
    done = subprocess.run([*GIT, "-C", str(repo), *args], capture_output=True, text=True, check=True)
    return done.stdout.strip()


def _commit(repo: Path, message: str, files: dict[str, str]) -> str:
    for rel, text in files.items():
        path = repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    _git(repo, "add", "-A")
    _git(repo, "commit", "-q", "-m", message)
    return _git(repo, "rev-parse", "HEAD")


def _learning_repo(base: Path) -> Path:
    repo = base / "toyrepo"
    (repo / "src").mkdir(parents=True)
    (repo / "docs").mkdir()
    (repo / "README.md").write_text("# Toy\nA queue and a worker.\n")
    (repo / "docs" / "design.md").write_text("# Design\nLEARNCANARY local only.\n")
    (repo / "src" / "queue.py").write_text("class Queue: ...\n")
    subprocess.run([*GIT, "init", "-q", "-b", "main", str(repo)], check=True, capture_output=True)
    _commit(repo, "chore: start", {})
    return repo


def _q(bloom: str, prompt: str = "Why?", answer: str = "Because.") -> dict[str, Any]:
    return {"bloom": bloom, "prompt": prompt, "answer": answer}


def _concepts() -> list[dict[str, Any]]:
    return [
        {"id": "queue", "section": "core", "title": "The queue", "cites": ["repo:toyrepo/src/queue.py"], "questions": [_q("understand"), _q("apply")]},
        {"id": "worker", "section": "core", "title": "The worker", "prereqs": ["queue"], "cites": ["note:01-core.md"], "questions": [_q("understand"), _q("analyze")]},
        {"id": "deploy", "section": "ops", "title": "Deploys", "cites": ["doc:runbook"], "questions": [_q("apply")]},
    ]


def _stage(ws: Workspace) -> Path:
    repo = _learning_repo(ws.base)
    doc = ws.base / "runbook.md"
    doc.write_text("Deploy with make ship. LEARNCANARY\n")
    lp.start("Toy", [("repo", str(repo)), ("doc", f"runbook:{doc}")], consume=True)
    lp.note("Toy", "01-core.md", "# Core\nQueue then worker. LEARNCANARY\n")
    return repo


def _source_staging(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "source-staging")
    os.environ["CAREERBREW_ENCRYPT"] = "on"
    repo = _learning_repo(ws.base)
    before = _git(repo, "status", "--porcelain", "--ignored")
    doc = ws.base / "runbook.md"
    doc.write_text("Deploy with make ship. LEARNCANARY\n")
    first = lp.start("Toy", [("repo", str(repo)), ("doc", f"runbook:{doc}")], consume=True)
    stored = lp.folder("Toy") / "inputs" / "doc" / "runbook.md"
    raw = stored.read_bytes()
    repo_unchanged = before == _git(repo, "status", "--porcelain", "--ignored")
    repeat = lp.start("Toy", [("repo", str(repo))])
    (repo / "README.md").write_text("# Toy\nNow with retries.\n")
    changed = lp.start("Toy", [("repo", str(repo))])
    lp.note("Toy", "01-core.md", "# Core\nLEARNCANARY note\n")
    note_ok = lp.note("Toy", "01-core.md").endswith("note\n")
    return {
        "first": {"keys": sorted(s["key"] for s in first["sources"]), "docs": [s.get("docs") for s in first["sources"]], "freshness": first["freshness"]},
        "privacy": {
            "consumed": not doc.exists(),
            "sealed": raw.startswith(b"CBV1\n"),
            "plain_absent": b"LEARNCANARY" not in raw,
            "repo_unchanged": repo_unchanged,
        },
        "repeat": repeat["freshness"],
        "changed": {"freshness": changed["freshness"], "stale": changed["stale"], "stable_hash": changed["sources"][0]["hash"] == first["sources"][0]["hash"]},
        "note": {"read": note_ok, "bad_path": _error(lambda: lp.note("Toy", "../bad.md", "x")), "missing_project": _error(lambda: lp.note("Missing", "x.md"))},
        "projects": lp.projects(),
        "errors": [_error(lambda: lp.start("Empty", [])), _error(lambda: lp.start("Toy", [("bad", "x")])), _error(lambda: lp.start("Toy", [("doc", "missing")]))],
        "cli": _invoke(["learn", "start", "Toy", "--repo", str(repo)]),
    }


def _concept_validation(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "concept-validation")
    _stage(ws)
    bad = [
        {"id": "a", "section": "s", "title": "A", "prereqs": ["b"], "cites": ["repo:toyrepo/missing.py"], "questions": [_q("remember")]},
        {"id": "b", "section": "s", "title": "B", "prereqs": ["a"], "cites": ["note:01-core.md"], "questions": [{**_q("apply"), "options": ["x", "y", "z"], "misconceptions": ["one"]}]},
    ]
    problems = lp.check_concepts("Toy", bad)
    refused = _error(lambda: lp.import_concepts("Toy", bad))
    escape = [{**_concepts()[0], "cites": ["repo:toyrepo/../outside.md"]}]
    outside = lp.check_concepts("Toy", escape)
    imported = lp.import_concepts("Toy", _concepts())
    lp.answer("Toy", "queue", "correct", "high", "understand", mode="teach", at=T0)
    lp.answer("Toy", "queue", "correct", "high", "apply", at=T0)
    reduced = lp.import_concepts("Toy", [_concepts()[2]])
    status = lp.status("Toy", at=T0 + timedelta(days=2))
    next_after_remove = lp.next_step("Toy", at=T0 + timedelta(days=2))["concept"]
    cards.add("Interview?", "Public card.", ["algo"])
    with cards.deck_root(lp.folder("Toy") / "srs"):
        learn_ids = {c["id"] for c in cards.all_cards().values()}
    interview_ids = {c["id"] for c in cards.all_cards().values()}
    bad_file = ws.base / "bad.yaml"
    import yaml

    bad_file.write_text(yaml.safe_dump({"concepts": bad}, sort_keys=False))
    good_file = ws.base / "good.yaml"
    good_file.write_text(yaml.safe_dump({"concepts": _concepts()}, sort_keys=False))
    return {
        "bad": problems,
        "refused": refused,
        "outside": outside,
        "imported": imported,
        "reduced": reduced,
        "removed": {"status_due": status["due_retests"], "next": next_after_remove, "retests": lp._model("Toy").get("retests", {})},
        "decks": {"learn_only": learn_ids <= {"deploy"}, "interview_separate": "deploy" not in interview_ids},
        "cli": {"bad": _invoke(["learn", "concepts", "Toy", str(bad_file), "--check"]), "good": _invoke(["learn", "concepts", "Toy", str(good_file)]), "list": _invoke(["learn", "list"])},
    }


def _learning_schedule(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "learning-schedule")
    _stage(ws)
    lp.import_concepts("Toy", _concepts())
    first = lp.next_step("Toy", at=T0)
    pretest = lp.answer("Toy", "queue", "dontknow", "low", "understand", mode="pretest", at=T0)
    second = lp.next_step("Toy", at=T0)
    lp.answer("Toy", "deploy", "wrong", "low", "apply", mode="pretest", at=T0)
    third = lp.next_step("Toy", at=T0)
    evidence = [
        lp.answer("Toy", "queue", "correct", "high", "apply", mode="pretest", at=T0)["mastered"],
        lp.answer("Toy", "queue", "correct", "low", "apply", at=T0)["mastered"],
        lp.answer("Toy", "queue", "correct", "high", "understand", at=T0)["mastered"],
    ]
    mastered = lp.answer("Toy", "queue", "correct", "medium", "apply", at=T0)
    early = lp.status("Toy", at=T0 + timedelta(hours=1))
    day1 = T0 + timedelta(days=1, minutes=1)
    due = lp.next_step("Toy", at=day1)
    missed = lp.answer("Toy", "queue", "wrong", "high", "apply", mode="retest", at=day1)
    after_miss = lp.status("Toy", at=day1)
    lp.answer("Toy", "queue", "correct", "high", "understand", at=day1)
    lp.answer("Toy", "queue", "correct", "high", "apply", at=day1)
    remastered_at = day1 + timedelta(days=1, minutes=1)
    remastered = lp.next_step("Toy", at=remastered_at)
    lp.answer("Toy", "queue", "correct", "high", "apply", mode="retest", at=remastered_at)
    day8 = day1 + timedelta(days=7, minutes=2)
    lp.answer("Toy", "queue", "wrong", "high", "apply", mode="retest", at=day8)
    final = lp.status("Toy", at=day8)
    for n in range(8):
        at = T0 + timedelta(days=n * 2)
        lp.answer("Toy", "worker", "correct", "high", "apply", at=at)
        leech_row = lp.answer("Toy", "worker", "wrong", "high", "apply", mode="review", at=at + timedelta(days=1))
    with cards.deck_root(lp.folder("Toy") / "srs"):
        suspended = "worker" in cards._suspended()
        queue_ids = [c["id"] for c in cards.queue(day8, new_per_day=0)]
    return {
        "steps": [{key: row.get(key) for key in ("mode", "concept", "section", "level")} for row in (first, second, third, due, remastered)],
        "pretest": pretest["mastered"],
        "evidence": evidence,
        "mastered": {"value": mastered["mastered"], "retests": lp._model("Toy").get("retests", {})},
        "early": {"due_retests": early["due_retests"], "scheduled_share": early["scheduled_share"]},
        "missed": {"mastered": missed["mastered"], "section_mastered": after_miss["sections"]["core"]["mastered"], "due_retests": after_miss["due_retests"]},
        "final": {"retest_retained_7d": final["retest_retained_7d"], "brier": final["brier"], "answers": final["answers"], "taught": final["taught"]},
        "leech": {"flag": leech_row.get("leech"), "suspended": suspended, "reviews": len([r for r in lp._log("Toy") if r["concept"] == "worker" and r["mode"] == "review"])},
        "queue": queue_ids,
        "cli": {
            "answer": _invoke(["learn", "answer", "Toy", "deploy", "--result", "wrong", "--confidence", "high", "--level", "apply"]),
            "status": _json_invoke(["learn", "status", "Toy", "--json"]),
            "next": _invoke(["learn", "next", "Toy"]),
        },
    }


def _replay_repo(base: Path) -> Path:
    repo = base / "toyrepo"
    repo.mkdir()
    subprocess.run([*GIT, "init", "-q", "-b", "main", str(repo)], check=True, capture_output=True)
    _commit(repo, "chore: start", {"README.md": "# Toy\n", "src/toy/__init__.py": "def add(a, b):\n    return a + b\n"})
    _commit(
        repo,
        "feat: double a number (#7)",
        {"src/toy/__init__.py": "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return add(n, n)\n", "tests/test_double.py": "from toy import double\n\n\ndef test_double():\n    assert double(4) == 8\n"},
    )
    _commit(repo, "docs: say hello (#8)", {"README.md": "# Toy\nHello.\n"})
    _commit(repo, "a direct push, not a PR", {"README.md": "# Toy\nHi.\n"})
    return repo


def _replay_case(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "replay")
    repo = _replay_repo(ws.base)
    lp.start("Toy", [("repo", str(repo))])
    rows = replay.candidates(repo)
    before = _git(repo, "status", "--porcelain", "--ignored")
    refs = sorted(p.name for p in (repo / ".git").iterdir())
    python = os.environ.get("CAREERBREW_RUST12_PYTHON") or sys.executable
    task = replay.start("Toy", 7, python=python)
    work = Path(task["dir"])
    parent_has_no_double = "double" not in (work / "src" / "toy" / "__init__.py").read_text()
    initial = replay.check("Toy", 7)
    (work / "src" / "toy" / "__init__.py").write_text("def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n")
    passed = replay.check("Toy", 7)
    measured = replay.measure(repo, limit=50, python=python)
    cli_check = _invoke(["learn", "replay", "check", "Toy", "7"])
    return {
        "candidates": rows,
        "task": {"pr": task["pr"], "title": task["title"], "tests": task["tests"], "inside_data": work.is_relative_to(paths.data_root()), "parent_has_no_double": parent_has_no_double},
        "checks": [{"passed": initial["passed"], "tests": initial["tests"]}, {"passed": passed["passed"], "tests": passed["tests"]}],
        "repo_untouched": before == _git(repo, "status", "--porcelain", "--ignored") and refs == sorted(p.name for p in (repo / ".git").iterdir()),
        "no_tests": _error(lambda: replay.start("Toy", 8, python=python)),
        "measure": measured,
        "cli": {"list": _invoke(["learn", "replay", "list", "Toy"]), "check": {"exit": cli_check["exit"], "passed": "PR #7: pass" in cli_check["stdout"]}, "bad": _invoke(["learn", "replay", "start", "Toy", "8", "--python", python])},
    }


def _tours(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "tours")
    repo = _replay_repo(ws.base)
    lp.start("Toy", [("repo", str(repo))])
    lp.import_concepts("Toy", [{"id": "double", "section": "core", "title": "Doubling", "cites": ["repo:toyrepo/src/toy/__init__.py"], "questions": [_q("apply", "double(3)?", "6")]}])
    valid = {
        "repo": "toyrepo",
        "title": "How doubling works",
        "steps": [{"file": "src/toy/__init__.py", "line": 5, "description": "double reuses add.", "concept": "double"}, {"file": "README.md", "line": 1, "description": "The project name."}],
    }
    bad = {"repo": "toyrepo", "title": "x", "steps": [{"file": "missing.py", "line": 1, "description": "d"}, {"file": "README.md", "line": 99, "description": ""}, {"file": "README.md", "line": 1, "description": "d", "concept": "nope"}]}
    problems = tour.check("Toy", bad)
    os.environ["CAREERBREW_ENCRYPT"] = "on"
    added = tour.add("Toy", "doubling", valid)
    raw = (lp.folder("Toy") / "tours" / "doubling.json").read_bytes()
    out, unreviewed = tour.export("Toy", "doubling")
    unreviewed_note = json.loads(out.read_text())["steps"][0]["description"].startswith("**No maintainer")
    for level in ("understand", "apply"):
        lp.answer("Toy", "double", "correct", "high", level, at=T0)
    approved = tour.approve("Toy", "doubling", "maintainer")
    out, faded = tour.export("Toy", "doubling")
    faded_text = json.loads(out.read_text())["steps"][0]["description"]
    _commit(repo, "fix: double with a multiply (#9)", {"src/toy/__init__.py": "def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n"})
    _, stale = tour.export("Toy", "doubling")
    source = ws.base / "tour.json"
    source.write_text(json.dumps(valid))
    return {
        "bad": problems,
        "added": {"approved": added["approved"], "steps": len(added["steps"]), "plain_absent": b"The project name" not in raw},
        "unreviewed": {"info": unreviewed, "note": unreviewed_note},
        "approved": {"sha_len": len(approved["approved"]["sha"]), "info": faded, "faded_text": faded_text},
        "stale": stale,
        "errors": [_error(lambda: tour.add("Toy", "Bad Name", valid)), _error(lambda: tour._load("Toy", "missing"))],
        "cli": {
            "add": _invoke(["learn", "tour", "add", "Toy", "second", str(source)]),
            "approve": _invoke(["learn", "tour", "approve", "Toy", "second", "--by", "maintainer"]),
            "export": _invoke(["learn", "tour", "export", "Toy", "second"]),
        },
    }


def _explainers(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "explainers-cli")
    root = Path(os.environ.get("CAREERBREW_RUST12_REPO") or Path.cwd())
    skill = root / "skills" / "careerbrew-project-explainer" / "references"
    example = (skill / "example-careerbrew.md").read_text()
    example_prd = (skill / "example-prd.md").read_text()
    deep_text = example.replace("## Invariants and boundaries", "## How it runs\n\n- Start everything with `uv run careerbrew`: it starts the watcher and the web app [S1].\n\n## Invariants and boundaries")
    parts = re.split(r"(?m)^(?=## )", example)
    short_text = "".join(p for p in parts if not p.startswith("## ") or explain._base(p[3:].split("\n")[0].strip()) in set(explain.SHORT)).replace("| S2 | primary | docs/VISION.md@c334531 | c334531 |\n", "")
    bad_source = example.replace("| S5 | primary | src/careerbrew/work/ | 2026-10-21 |", "| S5 | lead | src/careerbrew/work/ | 2026-10-21 |\n| S6 | maybe | ??? | |")
    no_pushback = re.sub(r"## Pushback\n\n.*?\n\n(?=## )", "", example_prd, flags=re.S)
    projects.add("billing", "", "7d")
    projects.add_source("billing", "tracker", "PROJ-1")
    scaffold = explain.scaffold("billing", deep=True)
    prd_scaffold = prd.scaffold("billing", slot=4)
    explain_path = ws.base / "explain.md"
    prd_path = ws.base / "prd.md"
    explain_path.write_text(example)
    prd_path.write_text(example_prd)
    return {
        "full": explain.check(example),
        "short": explain.check(short_text, short=True),
        "deep": {"pass": explain.check(deep_text, deep=True), "missing": explain.check(example, deep=True), "command": explain.check(deep_text.replace("with `uv run careerbrew`", "with the start command"), deep=True)},
        "source_rules": explain.check(bad_source),
        "structure": {
            "order": explain.check(example.replace("## Gotchas", "## Tmp").replace("## Read next", "## Gotchas").replace("## Tmp", "## Read next")),
            "citation": explain.check(example.replace("flags) [S5].", "flags).\n  - see [S5]")),
            "acronym": explain.check(example.replace("KEEP (met its target) state", "KEEP state")),
        },
        "prd": {
            "pass": prd.check(example_prd),
            "headings": prd.check(example_prd.replace("## Requirements", "## Asks").replace("## Decisions", "## Choices")),
            "decision": prd.check(example_prd.replace("| decided | the owner | 2026-10-27 | [S1] |\n| Estimates", "| maybe | the owner | 2026-10-27 | [S1] |\n| Estimates")),
            "units": [prd.check(example_prd.replace("| 0.5 dev weeks |", f"| {unit} |")) for unit in ("3 days", "2 sprints", "5 points", "40 hours", "2 weeks")],
            "slot": prd.check(example_prd.replace("Plan slot: 1 dev week [S1]\n", "")),
            "pushback": prd.check(no_pushback),
            "fits": prd.check(no_pushback.replace("Plan slot: 1 dev week", "Plan slot: 3 dev weeks")),
            "open": prd.check(example_prd.replace("Who: the owner", "ask around")),
        },
        "scaffolds": {
            "deep": "## How it runs" in scaffold and "| S1 | primary | PROJ-1 | TODO |" in scaffold,
            "deep_fails": bool(explain.check(scaffold, deep=True)),
            "prd": "Plan slot: 4 dev weeks" in prd_scaffold and "| S1 | primary | PROJ-1 | TODO |" in prd_scaffold,
            "prd_fails": bool(prd.check(prd_scaffold)),
        },
        "cli": {
            "explain": _invoke(["work", "project", "explain", "billing"]),
            "explain_check": _invoke(["work", "project", "explain", "billing", "--check", str(explain_path)]),
            "deep_check": _invoke(["work", "project", "explain", "billing", "--deep", "--check", str(explain_path)]),
            "prd": _invoke(["work", "project", "prd", "billing", "--slot", "2"]),
            "prd_check": _invoke(["work", "project", "prd", "billing", "--check", str(prd_path)]),
            "unknown": _invoke(["work", "project", "explain", "missing"]),
        },
    }


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust12-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "source-staging": _source_staging,
        "concept-validation": _concept_validation,
        "learning-schedule": _learning_schedule,
        "replay": _replay_case,
        "tours": _tours,
        "explainers-cli": _explainers,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":")) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
