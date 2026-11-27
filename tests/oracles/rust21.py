"""rust-21 Python oracle for the teacher, goals, practice and mistake tracking.

Every case uses invocation-owned synthetic data and fixed clocks. It drives the
real `teacher`, `goals`, `tutor.mistakes` and `tutor.cards` code. Three seams
stand in for steps that are not merged yet, with synthetic providers that the
Rust component mirrors exactly:

- the exercise runner (`exercises.run`, `exercises.grade`, rust-22) reads
  `tests/fixtures/rust21/runner.json`, keyed by the submitted code;
- interview loops (`interview.loop_for`, rust-19) read
  `tests/fixtures/rust21/loops.json`;
- the day re-plan (`goals.replan`, day planner) returns a fixed marker.

The exercise catalog is the real loader over `tests/fixtures/rust21/exercises/`.
Steps carry a wall-clock `started` stamp; reports drop it, and completed steps
use a future start so their timed seconds are always 0. Reports hold synthetic
values only. No production route, owner file, or live account is used.
"""

from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path
from typing import Any

import fsrs

from careerbrew import goals, interview, machine, paths, store, teacher
from careerbrew.tutor import cards, engine, exercises, mistakes

CASES = (
    "goals-queue",
    "goals-focus-blocks",
    "teacher-local-day",
    "teacher-flow",
    "teacher-cards-validate-resume",
    "teacher-improvements",
    "teacher-drafts",
)
ROOT = Path(__file__).resolve().parents[2]
FIX = ROOT / "tests" / "fixtures" / "rust21"
NOW = "2026-10-19T13:00:00+00:00"
FUTURE = "2099-01-01T00:00:00+00:00"
RUNNER = json.loads((FIX / "runner.json").read_text(encoding="utf-8"))["outcomes"]
LOOPS = json.loads((FIX / "loops.json").read_text(encoding="utf-8"))["loops"]

# Fixed vectors never use random FSRS interval fuzzing (as in rust-20). These
# replacements have process scope only in this oracle invocation.
cards.Scheduler = lambda: fsrs.Scheduler(enable_fuzzing=False)
engine.Scheduler = lambda: fsrs.Scheduler(enable_fuzzing=False)
exercises.run = lambda ex, code, with_scale=True: json.loads(json.dumps(RUNNER[code]["outcome"]))
exercises.grade = lambda ex, code, outcome: json.loads(json.dumps(RUNNER[code]["grade"]))
interview.loop_for = lambda company, seniority=3: {"loop": [{"id": step} for step in LOOPS.get(company, [])]}
goals.replan = lambda: {"synthetic": "replan"}


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
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> Any:
    try:
        fn()
    except Exception as exc:
        return [type(exc).__name__, str(exc)]
    return "accepted"


def _view(step: Any) -> Any:
    """A step without its wall-clock start, recursively through `next`."""
    if not isinstance(step, dict):
        return step
    out = {key: value for key, value in step.items() if key != "started"}
    if "next" in out:
        out["next"] = _view(out["next"])
    return out


def _done(step: dict[str, Any], result: dict[str, Any]) -> Any:
    return _view(teacher.complete({**step, "started": FUTURE}, result))


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.public = self.base / "public"
        self.root.mkdir(parents=True)
        self.root.chmod(0o700)
        self.public.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": NOW,
                "CAREERBREW_PUBLIC_CARDS": str(self.public),
                "CAREERBREW_EXERCISES": str(FIX / "exercises"),
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
            }
        )
        time.tzset()
        store.write_yaml(self.root / "settings.yaml", {"timezone": "UTC"})


def _goals_file() -> Any:
    return store.read_yaml(paths.sub("goals.yaml"), {})


def _goals_queue(base: Path) -> dict[str, Any]:
    Workspace(base, "goals")
    bad = {
        "kind": _error(lambda: goals.set_queue([{"kind": "vacation", "title": "x"}])),
        "title": _error(lambda: goals.set_queue([{"kind": "learn", "title": "  "}])),
        "minutes_text": _error(lambda: goals.set_queue([{"title": "x", "minutes_per_day": "ten"}])),
        "minutes_none": _error(lambda: goals.set_queue([{"title": "x", "minutes_per_day": None}])),
        "minutes_low": _error(lambda: goals.set_queue([{"title": "x", "minutes_per_day": 9}])),
        "minutes_high": _error(lambda: goals.set_queue([{"title": "x", "minutes_per_day": "601"}])),
        "date": _error(lambda: goals.set_queue([{"title": "x", "date": "2026-13-01"}])),
        "duplicate": _error(lambda: goals.set_queue([{"title": "Same goal"}, {"title": "same  GOAL"}])),
    }
    untouched = paths.sub("goals.yaml").exists()
    first = goals.set_queue(
        [
            {"kind": "learn", "title": "  Learn   Arrays  ", "topics": ["Arrays and Hashing", " ", "Heaps"], "minutes_per_day": "45", "date": "2026-11-01"},
            {"kind": "interview", "title": "SynCo onsite", "company": "  SynCo ", "date": "2026-10-25", "minutes_per_day": 60.9},
            {"kind": "custom", "title": "Read more ✓ books!", "id": ""},
            {"kind": "promotion", "title": "Past goal", "date": "2026-10-18"},
        ]
    )
    upcoming = goals.load()["upcoming"]
    replaced = goals.set_upcoming({"kind": "learn", "title": "Learn trees", "date": "2026-10-21", "minutes_per_day": 30})
    removed = goals.remove("learn-trees")
    emptied = goals.set_upcoming(None)
    long_term = goals.set_long_term([{"text": "  Become   staff  "}, {"text": ""}, {"text": "Write a book", "by": "2027-06-01", "id": "book"}])
    bad_by = _error(lambda: goals.set_long_term([{"text": "x", "by": "soon"}]))
    store.write_yaml(paths.sub("goals.yaml"), {"upcoming": {"id": "legacy", "kind": "learn", "title": "Legacy", "date": "2026-12-01", "minutes_per_day": 40}})
    legacy = goals.load()
    many = goals.set_queue([{"title": f"Goal {n}", "date": f"2026-11-{n + 1:02d}"} for n in range(22)])
    for n in range(21):
        goals.remove(f"goal-{n}")
    history = _goals_file()["history"]
    shapes = [
        goals.job_hunt_goals(None),
        goals.job_hunt_goals({}),
        goals.job_hunt_goals(["x"]),
        goals.job_hunt_goals({"queue": [{"kind": "interview"}, {"kind": "learn"}, "x"], "history": [{"kind": "job-search"}], "upcoming": {"kind": "interview"}}),
    ]
    days = {
        "none": goals.days_left({}),
        "undated": goals.days_left({"title": "x"}),
        "ahead": goals.days_left({"date": "2026-10-25"}),
        "past": goals.days_left({"date": "2026-10-01"}),
    }
    Workspace(base, "goals-corporate")
    machine.set_role("corporate")
    corporate = _error(lambda: goals.set_queue([{"kind": "job-search", "title": "Find a job"}]))
    corporate_learn = goals.set_queue([{"kind": "learn", "title": "Learn graphs"}])["upcoming"]["id"]
    return {
        "bad": bad,
        "untouched": untouched,
        "first": first,
        "upcoming": upcoming,
        "replaced": replaced,
        "removed": removed,
        "emptied": emptied,
        "long_term": long_term,
        "bad_by": bad_by,
        "legacy": legacy,
        "many_upcoming": many["upcoming"]["id"],
        "history": [[row["id"], row["replaced_at"]] for row in history],
        "history_count": len(history),
        "shapes": shapes,
        "days": days,
        "corporate": corporate,
        "corporate_learn": corporate_learn,
        "file": _goals_file(),
    }


def _goals_focus_blocks(base: Path) -> dict[str, Any]:
    Workspace(base, "focus")
    goal_rows = {
        "none": None,
        "interview-known": {"kind": "interview", "title": "a", "company": "SynCo", "topics": ["two pointers", "graphs", "kafka"]},
        "interview-quiet": {"kind": "interview", "title": "a", "company": "QuietCo"},
        "interview-design": {"kind": "interview", "title": "a", "company": "DesignCo"},
        "interview-unnamed": {"kind": "interview", "title": "a", "topics": ["rust"]},
        "learn-patterns": {"kind": "learn", "title": "a", "topics": ["trees", "rust"]},
        "learn-plain": {"kind": "learn", "title": "a", "topics": ["rust"]},
        "job-search": {"kind": "job-search", "title": "a", "topics": ["go"]},
        "promotion": {"kind": "promotion", "title": "a"},
        "custom": {"kind": "custom", "title": "a", "topics": ["x"]},
    }
    focus = {name: goals.focus(goal) if goal is not None else goals.focus() for name, goal in goal_rows.items()}
    blocks: dict[str, Any] = {"empty": goals.study_blocks()}
    store.write_jsonl(
        paths.sub("tutor", "exercise-attempts.jsonl"),
        [{"exercise": "syn-001", "at": NOW, "seconds": 30, "hints": 0, "correct": True, "score": 1.0, "passed": 3, "total": 3, "first": True, "complexity_ok": True, "code": "pass"}],
    )
    plans = {
        "interview": [{"kind": "interview", "title": "Onsite", "company": "SynCo", "date": "2026-10-23", "minutes_per_day": 95, "topics": ["arrays and hashing"]}],
        "learn-today": [{"kind": "learn", "title": "Today", "date": "2026-10-19", "minutes_per_day": 50}],
        "learn-undated": [{"kind": "learn", "title": "Someday", "minutes_per_day": 25, "topics": ["trees", "heap", "rust"]}],
        "promotion": [{"kind": "promotion", "title": "Promo", "date": "2026-12-01", "minutes_per_day": 120, "topics": ["impact", "scope", "x"]}],
        "job-search": [{"kind": "job-search", "title": "Search", "date": "2026-11-19", "minutes_per_day": 50}],
        "all-solved": [{"kind": "learn", "title": "Solved", "date": "2026-10-29", "topics": ["arrays and hashing"], "minutes_per_day": 10}],
    }
    for name, queue in plans.items():
        goals.set_queue(queue)
        blocks[name] = goals.study_blocks()
    store.write_jsonl(
        paths.sub("tutor", "exercise-attempts.jsonl"),
        [{"exercise": e, "at": NOW, "seconds": 1, "correct": True, "score": 1.0} for e in ("syn-001", "syn-002")],
    )
    blocks["all-solved-later"] = goals.study_blocks()
    blocks["other-day"] = goals.study_blocks(goals.date(2026, 10, 30))
    Workspace(base, "focus-corporate")
    machine.set_role("corporate")
    store.write_yaml(paths.sub("goals.yaml"), {"queue": [{"id": "x", "kind": "interview", "title": "x", "date": "2026-10-30"}]})
    blocks["corporate"] = goals.study_blocks()
    return {"focus": focus, "blocks": blocks}


def _teacher_local_day(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "local-day")
    os.environ["TZ"] = "Asia/Tokyo"
    time.tzset()
    store.write_yaml(ws.root / "settings.yaml", {"timezone": "America/New_York"})
    os.environ["CAREERBREW_NOW"] = "2026-10-20T03:30:00+00:00"
    store.write_jsonl(
        paths.sub("teach", "steps.jsonl"),
        [
            {"at": "2026-10-19T03:00:00+00:00", "kind": "card", "outcome": "good", "seconds": 900, "ref": "c0", "rules": "x"},
            {"at": "2026-10-19T14:00:00+00:00", "kind": "card", "outcome": "good", "seconds": 600, "ref": "c1", "rules": "x"},
            {"at": "2026-10-20T03:00:00+00:00", "kind": "lesson", "outcome": "confirmed", "seconds": 1259, "ref": "trees", "rules": "x"},
            {"at": "2026-10-20T05:00:00+00:00", "kind": "card", "outcome": "again", "seconds": 300, "ref": "c2", "rules": "x"},
        ],
    )
    goals.set_queue(
        [
            {"kind": "learn", "title": "Yesterday in UTC", "date": "2026-10-19", "minutes_per_day": 30},
            {"kind": "learn", "title": "Learn trees", "date": "2026-10-25", "minutes_per_day": 30, "topics": ["trees"]},
        ]
    )
    upcoming = goals.load()["upcoming"]["id"]
    summary = teacher.summary()
    done = _view(teacher.next_step())
    goals.set_queue([{"kind": "learn", "title": "Learn trees", "date": "2026-10-25", "minutes_per_day": 31, "topics": ["trees"]}])
    under = _view(teacher.next_step())
    os.environ["CAREERBREW_NOW"] = "2026-10-20T05:30:00+00:00"
    late = teacher.summary()
    return {"upcoming": upcoming, "summary": summary, "done": done, "under_budget": under, "after_midnight": late}


def _teacher_flow(base: Path) -> dict[str, Any]:
    Workspace(base, "flow")
    store.write_json(
        paths.sub("approvals.json"),
        [{"id": "ap000001", "title": "Synthetic approval", "detail": "", "by": "agent", "at": "2026-10-19T12:00:00+00:00", "status": "pending", "note": ""}],
    )
    out: dict[str, Any] = {}
    step = teacher.next_step()
    out["approve"] = _view(step)
    out["approve_done"] = _done(step, {"decision": "approved", "note": "  synthetic ok  "})
    out["approvals"] = store.read_json(paths.sub("approvals.json"), [])
    goals.set_queue([{"kind": "learn", "title": "Learn arrays", "topics": ["arrays and hashing", "heaps"], "minutes_per_day": 30, "date": "2026-11-01"}])
    mistakes.record("syn-001", "arrays and hashing", "edge-case", "empty input")
    mistakes.record("syn-001", "arrays and hashing", "edge-case", "empty input again")
    step = teacher.next_step()
    out["mistake"] = _view(step)
    out["mistake_done"] = _done(step, {})
    lesson = teacher.next_step()
    out["lesson_not_yet"] = _done(lesson, {"confirmed": False})
    again = teacher.next_step()
    out["lesson_again"] = _view(again)
    teacher.save_draft("syn-001", "def pair_count(nums):\n    return 0\n")
    out["lesson_confirmed"] = _done(again, {"confirmed": True})
    for code in ("fail-edge", "fail-wrong", "fail-error"):
        step = teacher.next_step()
        out[f"exercise_{code}"] = _done(step, {"code": code})
    step = teacher.next_step()
    out["second_exercise"] = _view(step)
    out["second_pass"] = _done(step, {"code": "pass"})
    os.environ["CAREERBREW_NOW"] = "2026-10-23T13:00:00+00:00"
    step = teacher.next_step()
    out["retry"] = _view(step)
    out["retry_done"] = _done(step, {"code": "pass-slow"})
    step = teacher.next_step()
    out["two_pointers_confirmed"] = _done(step, {"confirmed": True})
    step = teacher.next_step()
    out["two_pointers_timeout"] = _done(step, {"code": "fail-timeout"})
    step = teacher.next_step()
    out["two_pointers_plain"] = _done(step, {"code": "pass-plain"})
    out["state"] = teacher.state()
    out["log"] = teacher.log()
    out["attempts"] = exercises.attempts()
    out["mistakes"] = mistakes.summary()
    out["summary"] = teacher.summary()
    return out


def _teacher_cards_validate_resume(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "cards")
    store.write_yaml(ws.root / "settings.yaml", {"timezone": "UTC", "cards_new_per_day": 0})
    goals.set_queue([{"kind": "promotion", "title": "Promo case", "minutes_per_day": 60}])
    cards.write_deck("mine", [{"id": "m1", "question": "Synthetic mine one?", "answer": "One.", "topic": "promo", "tags": ["promo"]}, {"id": "m2", "question": "Synthetic mine two?", "answer": "Two.", "topic": "promo", "tags": ["promo"]}])
    cards.write_deck("notion", [{"id": "n1", "question": "Synthetic notion?", "answer": "Notion.", "topic": "notes", "tags": ["notes"]}])
    cards.review("m1", "good", at=teacher.calendar.now_utc() - teacher.timedelta(days=30))
    out: dict[str, Any] = {}
    step = teacher.next_step()
    out["card"] = _view(step)
    out["card_done"] = _done(step, {"rating": "again"})
    for decision, extra in (("keep", {}), ("fix", {"answer": "A fixed synthetic answer."}), ("drop", {})):
        step = teacher.next_step()
        out[f"validate_{decision}"] = _view(step)
        out[f"validate_{decision}_done"] = _done(step, {"decision": decision, **extra})
    store.write_json(
        paths.sub("jobs", "strategy.json"),
        {"tiers": {"practice": [{"posting_id": "p1", "company": "SynCo", "skills": ["a", "b", "c", "d", "e", "f"]}], "target": [{"posting_id": "p2", "company": "OtherCo"}], "dream": [{"posting_id": "p3", "company": "synco"}]}},
    )
    for _ in range(10):  # capped: a state that never changes must not hang the oracle
        if (step := teacher.next_step())["kind"] != "validate":
            break
        _done(step, {"decision": "keep"})
    out["resume"] = _view(step)
    out["resume_sent"] = _done(step, {"sent": True})
    step = teacher.next_step()
    out["resume_skip"] = _done(step, {"sent": False})
    out["applications"] = [{k: v for k, v in row.items()} for row in store.read_jsonl(paths.sub("jobs", "applications.jsonl"))]
    out["state"] = teacher.state()
    out["decks"] = {deck: [c["id"] for c in cards.all_cards().values() if c["deck"] == deck] for deck in ("mine", "notion")}
    out["suspended"] = cards.stats()["suspended"]
    return out


def _teacher_improvements(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "improve")
    rows = [{"at": NOW, "kind": "card", "outcome": "good", "seconds": s, "ref": f"c{n}", "rules": "x"} for n, s in enumerate((60, 60, 61, 59, 60, 400))]
    rows += [{"at": NOW, "kind": "lesson", "outcome": "not-yet", "seconds": 30, "ref": ref, "rules": "x"} for ref in ("trees", "heap", "graphs", "stack")]
    rows += [{"at": NOW, "kind": "resume", "outcome": "skipped", "seconds": 5, "ref": "", "rules": "x"} for _ in range(2)]
    store.write_jsonl(paths.sub("teach", "steps.jsonl"), rows)
    store.write_jsonl(
        paths.sub("tutor", "exercise-attempts.jsonl"),
        [{"exercise": e, "at": NOW, "correct": ok, "score": 0.0} for e, ok in (("syn-001", False), ("syn-002", False), ("syn-001", False), ("syn-001", False), ("syn-002", True), ("syn-002", False))],
    )
    for detail in ("first", "second", "third"):
        mistakes.record("syn-003", "two pointers", "exception", detail)
    mistakes.record("lesson:trees", "trees", "concept", "not yet")
    mistakes.record("lesson:trees", "trees", "concept", "not yet")
    mistakes.record("syn-002", "arrays and hashing", "complexity", "expected O(n)")
    cards.write_deck("mine", [{"id": "m1", "question": "Leech?", "answer": "Yes.", "topic": "x", "tags": ["x"]}])
    cards.suspend("m1")
    feedback = [
        teacher.feedback("  Too   many\ncards  ", "card", 1),
        teacher.feedback("x" * 2100, "c" * 40, None),
        teacher.feedback("Cards again", "card", 2),
        teacher.feedback("Old complaint", "lesson", 1),
    ]
    store.write_jsonl(paths.sub("teach", "feedback.jsonl"), [*store.read_jsonl(paths.sub("teach", "feedback.jsonl")), {"at": "2026-09-01T00:00:00+00:00", "text": "old", "step": "lesson", "rating": 1}])
    improvements = teacher.improvements()
    low = sorted(teacher._low_rated_kinds())
    goals.set_queue([{"kind": "learn", "title": "Learn arrays", "topics": ["arrays and hashing"], "minutes_per_day": 600}])
    cards.write_deck("review", [{"id": "r1", "question": "Due?", "answer": "Due.", "topic": "x", "tags": ["x"]}])
    cards.review("r1", "good", at=teacher.calendar.now_utc() - teacher.timedelta(days=30))
    store.write_yaml(ws.root / "settings.yaml", {"timezone": "UTC", "cards_new_per_day": 0})
    teacher._save_state({"confirmed_patterns": ["arrays and hashing"], "last_alarm": "9999"})
    moved = _view(teacher.next_step())
    watch = teacher.watch_out(exercises.get("syn-003"))
    return {"feedback": feedback, "improvements": improvements, "low": low, "moved_later": moved, "watch_out": watch, "rules_version": teacher.rules_version()}


def _teacher_drafts(base: Path) -> dict[str, Any]:
    Workspace(base, "drafts")
    errors = [_error(lambda: teacher.save_draft("", "x")), _error(lambda: teacher.save_draft("syn-001", "x" * 100_001))]
    teacher.save_draft("syn-001", "first")
    teacher.save_draft("syn-002", "x" * 100_000)
    teacher.save_draft("syn-001", "second ✓")
    teacher.save_draft("syn-003", "RUST21CANARYCODE")  # never reported: the privacy canary
    drafts = teacher.drafts()
    return {"errors": errors, "ids": sorted(drafts), "first": drafts["syn-001"], "long": len(drafts["syn-002"]), "no_goal": _view(teacher.next_step())}


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust21-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "goals-queue": _goals_queue,
        "goals-focus-blocks": _goals_focus_blocks,
        "teacher-local-day": _teacher_local_day,
        "teacher-flow": _teacher_flow,
        "teacher-cards-validate-resume": _teacher_cards_validate_resume,
        "teacher-improvements": _teacher_improvements,
        "teacher-drafts": _teacher_drafts,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True) + "\n"}


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
