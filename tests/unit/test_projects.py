"""Work projects with checked_at per source (issue #32, US-50)."""

import ast
import json
from datetime import datetime
from pathlib import Path

import pytest

from careerbrew import checkup, events, vault
from careerbrew.work import focus, priorities, project_priority, projects


@pytest.fixture
def clock(home, monkeypatch):
    def at(iso):
        monkeypatch.setenv("CAREERBREW_NOW", iso)

    at("2026-10-01T09:00+00:00")
    return at


def test_check_stamps_timezone_aware_utc(clock):
    projects.add("billing", stale_after="7d")
    projects.add_source("billing", "tracker", "PROJ-1")
    assert projects.check("billing", "PROJ-1", "epic 3/5") == 1
    (s,) = projects.show("billing")["sources"]
    when = datetime.fromisoformat(s["checked_at"])
    assert when.tzinfo is not None and when.utcoffset().total_seconds() == 0 and s["status"] == "epic 3/5"


def test_check_all_and_stale_after_n_days(clock):
    projects.add("billing", memory="notes/billing.md", stale_after="1w")
    projects.add_source("billing", "doc", "doc-42")
    assert projects.check("billing") == 2, "no ref: every source"
    clock("2026-10-08T08:00+00:00")
    assert projects.stale_sources() == []
    clock("2026-10-08T10:00+00:00")
    assert {s["ref"] for s in projects.stale_sources()} == {"notes/billing.md", "doc-42"}


def test_unknown_project_names_the_fix_and_duplicates_are_refused(clock):
    with pytest.raises(projects.ProjectError, match="careerbrew work project add nope"):
        projects.check("nope")
    projects.add("billing")
    with pytest.raises(projects.ProjectError, match="already exists"):
        projects.add("billing")
    projects.add_source("billing", "repo", "svc-billing")
    with pytest.raises(projects.ProjectError, match="already a source"):
        projects.add_source("billing", "repo", "svc-billing")
    with pytest.raises(projects.ProjectError, match="unknown source kind"):
        projects.add_source("billing", "email", "x")


def test_list_puts_projects_with_stale_sources_first(clock):
    projects.add("fresh")
    projects.add_source("fresh", "doc", "d1")
    projects.check("fresh")
    projects.add("old")
    projects.add_source("old", "doc", "d2")
    assert [r["name"] for r in projects.list_rows()] == ["old", "fresh"]
    shown = projects.show("old")["sources"]
    assert shown[0]["age_days"] is None, "never-checked sources come first"


def test_record_is_encrypted_at_rest(clock, home):
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-SECRET-1")
    raw = projects.path().read_bytes()
    assert raw.startswith(vault.MAGIC) and b"PROJ-SECRET-1" not in raw


def test_next_steps_are_task_ids_and_stale_shows_in_checkup_and_focus(clock):
    t = priorities.add("finish billing migration", 4, 3, 2, [])
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.link_task("billing", t["id"])
    assert [x["id"] for x in projects.show("billing")["tasks"]] == [t["id"]]
    with pytest.raises(projects.ProjectError, match="unknown task"):
        projects.link_task("billing", "t999")
    assert not checkup.projects_step()["ok"] and checkup.STEPS[0] is checkup.projects_step
    assert focus.build()["stale"][0]["ref"] == "PROJ-1"


def test_work_project_event_keeps_action(clock, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    projects.add("zebra-canary-7q")
    project_priority.set_project("zebra-canary-7q")
    text = (home / "logs" / "events.jsonl").read_text()
    rows = [json.loads(line) for line in text.splitlines()]
    assert {"event": "work.project", "status": "ok", "action": "add"}.items() <= next(r for r in rows if r["event"] == "work.project").items()
    assert next(r for r in rows if r["event"] == "work.priority")["action"] == "set"
    assert "zebra-canary-7q" not in text, "the project name never reaches the log"


def test_work_project_event_fields_are_allow_listed_short_ids():
    """Every `action` passed to `events.emit` is a fixed short id, and no literal string field is dropped silently."""
    actions, dropped, spreads = set(), [], []
    for path in sorted((Path(events.__file__).parent).rglob("*.py")):
        for node in ast.walk(ast.parse(path.read_text(encoding="utf-8"))):
            func = getattr(node, "func", None)
            if not isinstance(node, ast.Call) or getattr(func, "attr", getattr(func, "id", None)) != "emit":
                continue
            for kw in node.keywords:
                where = f"{path.name}:{node.lineno}"
                literal = isinstance(kw.value, ast.Constant) and isinstance(kw.value.value, str)
                if kw.arg is None:
                    spreads.append(where)
                elif kw.arg == "action":
                    assert literal and events._ID.match(kw.value.value), f"{where}: action must be a fixed short id, not free text"
                    actions.add(kw.value.value)
                if kw.arg and literal and kw.arg not in events.ALLOWED_STRING_KEYS:
                    dropped.append(f"{where} {kw.arg}")
    assert {"add", "set", "remove", "import"} <= actions, "the scan found the work.project and work.priority calls"
    assert not spreads, f"emit(**fields) hides which keys reach the log: {spreads}"
    assert not dropped, f"events.clean drops these string fields; add the key to ALLOWED_STRING_KEYS: {dropped}"
