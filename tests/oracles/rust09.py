"""rust-09 Python oracle for projects, priorities, promises, focus and team.

The workload uses only an invocation-owned synthetic data root, a pinned clock
and the two checked-in project fixtures. Reports contain typed values and fixed
errors. Random project identities are compared by shape and stability, never by
their random value. No production route or live account is used.
"""

from __future__ import annotations

import contextlib
import csv
import hashlib
import io
import json
import os
import sys
import threading
import time
from datetime import date
from pathlib import Path
from typing import Any

from careerbrew import cli
from careerbrew.work import focus, priorities, project_priority, projects, promises, team

CASES = (
    "project-lifecycle",
    "readiness-gates",
    "priorities-tickets",
    "promise-lifecycle",
    "lock-and-team",
    "cli-handlers",
)
FIX = Path(__file__).resolve().parents[1] / "fixtures" / "projects"


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


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "off",
                "CAREERBREW_NOW": "2026-11-02T09:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
            }
        )


def _project_lifecycle(base: Path) -> dict[str, Any]:
    Workspace(base, "project-lifecycle")
    cutline_raw = (FIX / "cutline.json").read_bytes()
    goals_raw = (FIX / "team-goals.csv").read_bytes()
    cutline = json.loads(cutline_raw)["projects"]
    goal_rows = list(csv.DictReader(io.StringIO(goals_raw.decode())))
    projects.add("search", title=cutline[0]["title"], group=cutline[0]["group"])
    projects.add("checkout", title=cutline[2]["title"])
    projects.add("search-ranking", parent="search", links=["checkout"])
    projects.add_source("search", "tracker", "SRCH-1")
    projects.add_source("search", "url", "https://docs.google.com/document/d/synthetic")
    projects.check("search", "SRCH-1", status="source checked")
    projects.record_update("search", "approval", "SYNTHCANARY approval")
    first_id = projects.records()["search"]["incarnation_id"]
    with projects.locked():
        staged, every, migrated = projects.session_record("search")
        if migrated:
            projects._save(every)
    second_id = staged["incarnation_id"]
    restricted = next(row for row in projects.watch_list() if row["name"] == "search")
    duplicate = _error(lambda: projects.add("search"))
    bad_kind = _error(lambda: projects.record_update("search", "invented", "example"))
    bad_graph = _error(lambda: projects.edit("search", parent="search-ranking"))
    remove_used = _error(lambda: projects.remove("search"))
    shown = projects.show("search")
    return {
        "fixture_sha256": {
            "cutline": hashlib.sha256(cutline_raw).hexdigest(),
            "team_goals": hashlib.sha256(goals_raw).hexdigest(),
            "goal_rows": len(goal_rows),
        },
        "identity": {
            "shape": len(first_id) == 32 and all(c in "0123456789abcdef" for c in first_id),
            "stable": first_id == second_id,
            "migrated": migrated,
        },
        "relations": {
            "parent": projects.records()["search-ranking"]["parent"],
            "links": projects.records()["search-ranking"]["links"],
            "root": projects.root("search-ranking"),
            "notes_path": projects.notes_path("search-ranking"),
        },
        "sources": [{key: row.get(key) for key in ("kind", "ref", "checked_at", "status", "stale", "reference")} for row in shown["sources"]],
        "approval": {
            "kind": restricted["latest_update"]["kind"],
            "restricted_has_text": "text" in restricted["latest_update"],
            "stored": projects.records()["search"]["updates"][-1]["text"] == "SYNTHCANARY approval",
        },
        "errors": {"duplicate": duplicate, "kind": bad_kind, "graph": bad_graph, "remove": remove_used},
        "tree": [(row["name"], row["depth"]) for row in projects.tree()],
        "stale_count": len(projects.stale_sources()),
    }


def _gate_rows(name: str) -> list[dict[str, Any]]:
    readiness = projects.show(name)["prep"]
    return [{key: row[key] for key in ("gate", "done", "at", "note", "fix")} for row in readiness["gates"]]


def _readiness_gates(base: Path) -> dict[str, Any]:
    Workspace(base, "readiness-gates")
    projects.add("demo", kind="prep", event_on="2026-11-03")
    initial = projects.show("demo")["prep"]
    projects.add_source("demo", "url", "https://example.com/rubric")
    projects.add_source("demo", "doc", "pack.md")
    projects.check("demo")
    after_check = _gate_rows("demo")
    projects.gate("demo", "tool")
    projects.gate("demo", "questions")
    first_quiz = projects.quiz("demo", 10, 3)
    short_requiz = _error(lambda: projects.quiz("demo", 2, 0))
    projects.quiz("demo", 3, 1)
    projects.quiz("demo", 1, 0)
    projects.gate("demo", "links", refs=["https://example.com/rubric"])
    try:
        projects.archive("demo")
    except Exception as exc:
        archive_refused = str(exc)
    else:
        archive_refused = "accepted"
    projects.gate("demo", "links", refs=["pack.md"])
    ready = projects.show("demo")["prep"]
    blocks_before = projects.gate_blocks(date(2026, 11, 3))
    projects.archive("demo")
    return {
        "initial": {"done": initial["done"], "total": initial["total"], "ready": initial["ready"], "event_on": initial["event_on"]},
        "after_check": after_check,
        "first_quiz": first_quiz["quiz"],
        "short_requiz": short_requiz,
        "archive_refused": archive_refused,
        "ready": {"done": ready["done"], "ready": ready["ready"], "quizzes": ready["quizzes"], "quiz": ready["quiz"]},
        "blocks_before": [{key: row[key] for key in ("id", "minutes", "gate", "project", "reason")} for row in blocks_before],
        "archived": projects.list_rows()[0]["archived"],
        "blocks_after": projects.gate_blocks(date(2026, 11, 3)),
        "bad_counts": [_error(lambda asked=asked, missed=missed: projects.quiz("demo", asked, missed)) for asked, missed in ((0, 0), (3, 4), (5, -1), (501, 0))],
    }


def _priority_view(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": row["id"],
        "score": row["score"],
        "estimate": priorities.estimate(row),
        "parts": row["parts"],
        "why": focus._task_why(row, {row["id"]: 1}, 1),
    }


def _priorities_tickets(base: Path) -> dict[str, Any]:
    Workspace(base, "priorities-tickets")
    projects.add("billing")
    projects.add("search")
    project_priority.set_project("billing", match_globs=["BILL-*", "api-*"])
    project_priority.set_project("search", match_globs=["SRCH-*"])
    configured = project_priority.keys("Fix UTF-8 and ISO-8601; BILL-42")
    generic = project_priority.keys("Fix UTF-8, ISO-8601 and ABC-123", es=[])
    linked = project_priority.keys("See the ticket", link="https://jira.example.com/browse/OPS-3", es=[])
    bad_hours = [_error(lambda value=value: priorities.hours(value)) for value in (float("nan"), float("inf"), -float("inf"), 0.01, 201)]
    estimates = [priorities.estimate({"estimate_h": value}) for value in (float("nan"), float("inf"), -1, "abc", None, 0.05, 200)]
    first = priorities.add("Fix UTF-8 and BILL-42", 4, 4, 2, [], due="2026-11-03", links=["ticket=https://jira.example.com/browse/BILL-42"])
    second = priorities.add("SRCH-7 ranking", 3, 3, 1, [])
    projects.link_task("billing", first["id"])
    ranked = priorities.rank({}, today=date(2026, 11, 2))
    regraded = priorities.regrade(second["id"], impact=5, urgency=2, estimate_h=1.5)
    impacted = priorities.record_impact(first["id"], expected="Fewer synthetic failures", who="support", metric="failures per day")
    done = priorities.done(first["id"], 1)
    already_done = _error(lambda: priorities.done(first["id"], 1))
    return {
        "entries": project_priority.entries(),
        "configured_keys": configured,
        "generic_keys": generic,
        "linked_keys": linked,
        "ticket_projects": sorted(project_priority.ticket_projects()),
        "repos": [project_priority.repo_of(rid, url) for rid, url in (("pr:acme/api#4", ""), ("git:local:abcdef", ""), ("", "https://github.com/acme/web/pull/2"))],
        "bonus": [project_priority.bonus(rank, 2) for rank in (0, 1, 2, 3)],
        "bad_hours": bad_hours,
        "estimates": estimates,
        "ranked": [_priority_view(row) for row in ranked],
        "regraded": {key: regraded[key] for key in ("id", "impact", "urgency", "estimate_h")},
        "impact": impacted["expected_impact"],
        "done": {key: done[key] for key in ("id", "status", "actual_h", "estimate_h")},
        "already_done": already_done,
        "focus_blocks": focus.BLOCKS,
        "due_text": [focus._due_text(day) for day in (-2, -1, 0, 1, 2)],
    }


def _promise_view(business: bool = True, include_closed: bool = False) -> list[dict[str, Any]]:
    return promises.open_list(business=business, include_closed=include_closed)


def _promise_lifecycle(base: Path) -> dict[str, Any]:
    Workspace(base, "promise-lifecycle")
    projects.add("billing", title="Billing service")
    from careerbrew.work import feed

    feed.ingest(
        [
            {"id": "slack:D1:1", "source": "slack", "kind": "dm", "at": "2026-11-02T08:00:00Z", "title": "DM from Sam", "actor": "other", "to_me": True, "from": "Sam", "thread": "slack:D1", "text": "the synthetic job times out"},
            {
                "id": "slack:D1:2",
                "source": "slack",
                "kind": "dm",
                "at": "2026-11-02T08:05:00Z",
                "title": "You wrote in a DM",
                "actor": "me",
                "thread": "slack:D1",
                "url": "https://slack.example.com/archives/D1/p2",
                "project": "Billing service",
                "text": "Thanks! I'll send the rollout notes tomorrow",
            },
        ]
    )
    initial = _promise_view()
    name = initial[0]["name"]
    restricted = _promise_view(business=False)
    promises.step(name, "research", "read the synthetic log")
    after_research = _promise_view()[0]["next"]
    promises.step(name, "draft", "synthetic follow-up drafted")
    after_draft = _promise_view()[0]["next"]
    bad_step = _error(lambda: promises.step(name, "send", "not allowed"))
    promises.close(name, "done")
    closed = _promise_view(include_closed=True)
    rescan = promises.scan()
    after_rescan = _promise_view(include_closed=True)
    manual_a = promises.add("Send the synthetic notes", to="Kim", due="2026-10-30")
    manual_b = promises.add("Send the synthetic notes", to="Kim", due="2026-10-30")
    projects.archive(manual_a)
    projects.archive(manual_a, on=False)
    positives = [
        "Thanks! I'll look into the timeout.",
        "I will send it tomorrow",
        "ok, I'm going to fix the flaky test by Friday!",
        "Let me check the logs today",
        "i'll write the doc next week",
        "I can review it by 2026-11-04",
        "Sure <@U1>, I'll ping <https://x.example.com/a|the owners> in 2 days",
    ]
    negatives = ["Let me know when it is done", "I'll be out tomorrow", "I will not do that", "I'll check.", "I can review it", "I can do that by Friday?", "The timeout fired again"]
    return {
        "initial": initial,
        "restricted": restricted,
        "after_research": after_research,
        "after_draft": after_draft,
        "bad_step": bad_step,
        "closed": closed,
        "rescan": rescan,
        "after_rescan": after_rescan,
        "manual_distinct": manual_a != manual_b,
        "manual_reopened": next(row for row in _promise_view() if row["name"] == manual_a)["closed"] == "",
        "positive": [promises.detect(text, date(2026, 11, 2)) for text in positives],
        "negative": [promises.detect(text, date(2026, 11, 2)) for text in negatives],
        "stale_sources": projects.stale_sources(),
        "handoff": promises.handoff_items(business=False),
        "bad_close": _error(lambda: promises.close(name, "invented")),
        "bad_due": _error(lambda: promises.add("Send it", due="next friday")),
        "bad_link": _error(lambda: promises.add("Send it", link="javascript:alert(1)")),
    }


def _lock_and_team(base: Path) -> dict[str, Any]:
    Workspace(base, "lock-and-team")
    with projects.locked():
        projects.add("billing")
        projects.record_update("billing", "team", "nested update")
    projects.add_source("billing", "tracker", "BILL-1")

    def writer(name: str) -> None:
        with projects.locked():
            every = projects._load()
            time.sleep(0.05)
            every[name]["updates"] = [*every[name].get("updates", []), {"at": "2026-11-02T09:00:00+00:00", "kind": "project", "text": f"update {name}"}]
            every[name]["sources"][0]["status"] = f"seen {name}"
            projects._save(every)

    projects.add("search")
    projects.add_source("search", "tracker", "SRCH-1")
    threads = [threading.Thread(target=writer, args=(name,)) for name in ("billing", "search")]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(timeout=10)
    assert all(not thread.is_alive() for thread in threads)
    stored = projects._load()
    lock_rows = {
        name: {
            "updates": [row["text"] for row in stored[name]["updates"]],
            "status": stored[name]["sources"][0]["status"],
        }
        for name in ("billing", "search")
    }

    promise_name = promises.add("Close the synthetic promise")
    with projects.locked():
        every = projects._load()
        every[promise_name]["promise"]["owner_note"] = "owner edit kept"
        closer = threading.Thread(target=lambda: promises.close(promise_name, "done"))
        closer.start()
        time.sleep(0.05)
        projects._save(every)
    closer.join(timeout=10)
    assert not closer.is_alive()
    closed = projects.records()[promise_name]

    added = team.add("Sam Lee", chat="U0SAM", email="SAM@example.com", login="@samlee")
    unchanged = team.add("sam lee", chat="U0SAM", email="sam@example.com", login="samlee")
    matcher = team.matcher()
    matches = [
        matcher("U0SAM", "", ""),
        matcher("", "sam@example.com", ""),
        matcher("", "", "Sam Lee owns this"),
        matcher("", "", "<@u0sam> and @SAMLEE"),
        matcher("", "", "max retries"),
    ]
    bad_chat = _error(lambda: team.add("Bad Chat", chat="not-an-id"))
    unknown_remove = _error(lambda: team.remove("Nobody"))
    team.remove("Sam Lee")
    return {
        "nested": stored["billing"]["updates"][0]["text"] == "nested update",
        "lock_rows": lock_rows,
        "owner_closure": {"closed": closed["promise"]["closed"], "owner_kept": closed["promise"].get("owner_note") == "owner edit kept", "archived": bool(closed["archived_at"])},
        "team_added": added,
        "team_unchanged": unchanged,
        "matches": matches,
        "bad_chat": bad_chat,
        "unknown_remove": unknown_remove,
        "team_after_remove": team.load(),
    }


def _invoke(args: list[str]) -> dict[str, Any]:
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        rc = cli.main(args)
    return {"exit": rc, "stdout": output.getvalue()}


def _cli_handlers(base: Path) -> dict[str, Any]:
    Workspace(base, "cli-handlers")
    priorities.add("Synthetic task", 3, 3, 2, [])
    return {
        "priority_set": _invoke(["work", "priority", "set", "billing", "--rank", "1", "--match", "BILL-*"]),
        "priority_show": _invoke(["work", "priority", "show"]),
        "task_set": _invoke(["work", "task-set", "t001", "--impact", "4", "--urgency", "5", "--hours", "1.5"]),
        "impact": _invoke(["work", "impact", "t001", "--expect", "Fewer synthetic failures", "--who", "support", "--metric", "failures per day"]),
        "done": _invoke(["work", "done", "t001", "--hours", "1"]),
        "task_set_empty": _invoke(["work", "task-set", "t001"]),
        "impact_empty": _invoke(["work", "impact", "t001"]),
    }


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust09-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "project-lifecycle": _project_lifecycle,
        "readiness-gates": _readiness_gates,
        "priorities-tickets": _priorities_tickets,
        "promise-lifecycle": _promise_lifecycle,
        "lock-and-team": _lock_and_team,
        "cli-handlers": _cli_handlers,
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
